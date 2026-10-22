//! Treat an interrupted stream as a recoverable failure, never a finished song.
use super::{Command, Event, Playback};
use crate::catalog::Catalog;
use futures_util::future::BoxFuture;
use std::time::Duration;
use tokio::sync::mpsc;

const MAX_RETRIES: u8 = 2;
// Allow codec padding and small differences between catalog and audio durations.
const END_TOLERANCE_MS: u32 = 5_000;

struct Intent {
    id: String,
    generation: u64,
    attempt_generation: u64,
    position: u32,
    duration: u32,
    paused: bool,
    played: bool,
    retries: u8,
}

impl Intent {
    fn ended_early(&self) -> bool {
        self.duration > 0 && self.position.saturating_add(END_TOLERANCE_MS) < self.duration
    }

    fn load(&self, inner: &Playback) {
        let _ = inner.commands.send(Command::Load {
            id: self.id.clone(),
            position_ms: self.position,
            generation: self.attempt_generation,
        });
        if self.paused {
            let _ = inner.commands.send(Command::Pause);
        }
    }
}

// Each attempt has its own backend generation. Late EOF/failure/position events
// from the previous decoder must not finish or move the replacement stream.
fn generation(event: &mut Event) -> Option<&mut u64> {
    match event {
        Event::Fallback { generation, .. }
        | Event::Quality { generation, .. }
        | Event::Metadata { generation, .. }
        | Event::Playing { generation, .. }
        | Event::Paused { generation, .. }
        | Event::Position { generation, .. }
        | Event::Recovering { generation, .. }
        | Event::TimeToPreload { generation }
        | Event::TrackError { generation, .. }
        | Event::Completed(generation) => Some(generation),
        _ => None,
    }
}

pub(super) fn spawn(inner: Playback, catalog: Catalog) -> Playback {
    let (commands, rx) = mpsc::unbounded_channel();
    let (tx, events) = mpsc::unbounded_channel();
    let task = tokio::spawn(route(inner, catalog, rx, tx));
    Playback {
        commands,
        events,
        task,
    }
}

async fn route(
    mut inner: Playback,
    catalog: Catalog,
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<Event>,
) {
    let mut intent: Option<Intent> = None;
    let mut serial = 0u64;
    let mut metadata: Option<BoxFuture<'static, Option<u32>>> = None;
    let mut deferred = None;
    loop {
        let release_deferred = deferred.is_some() && metadata.is_none();
        tokio::select! {
            biased;
            command = commands.recv() => {
                let Some(command) = command else { break; };
                match command {
                    Command::Load { id, position_ms, generation } => {
                        serial = serial.wrapping_add(1);
                        metadata = None;
                        deferred = None;
                        // Deezer translation and YouTube resolution already emit
                        // metadata. Native Spotify/Volta need a cancellable lookup.
                        if crate::deezer::key(&id).is_none() && crate::youtube::video_key(&id).is_none() {
                            let catalog = catalog.clone();
                            let id = id.clone();
                            metadata = Some(Box::pin(async move {
                                tokio::time::timeout(Duration::from_secs(10), catalog.track(&id))
                                    .await.ok().and_then(Result::ok).map(|track| track.duration_ms)
                            }));
                        }
                        let current = Intent { id, generation, attempt_generation: serial,
                            position: position_ms, duration: 0, paused: false, played: false, retries: 0 };
                        current.load(&inner);
                        intent = Some(current);
                    }
                    Command::Stop => {
                        intent = None;
                        metadata = None;
                        deferred = None;
                        let _ = inner.commands.send(Command::Stop);
                    }
                    command => {
                        if let Some(current) = &mut intent {
                            match command {
                                Command::Seek(position) => current.position = position,
                                Command::Pause => current.paused = true,
                                Command::Resume => current.paused = false,
                                _ => (),
                            }
                        }
                        let _ = inner.commands.send(command);
                    }
                }
            }
            duration = async { metadata.as_mut().unwrap().await }, if metadata.is_some() => {
                metadata = None;
                if let Some(current) = &mut intent && let Some(duration) = duration {
                    current.duration = duration;
                }
            }
            event = async {
                if release_deferred {
                    deferred.take()
                } else {
                    inner.events.recv().await
                }
            } => {
                let Some(mut event) = event else { break; };
                if let Some(backend_generation) = generation(&mut event) {
                    let Some(current) = &mut intent else { continue; };
                    if *backend_generation != current.attempt_generation { continue; }
                    *backend_generation = current.generation;
                    match &event {
                        Event::Metadata { track, .. } if track.duration_ms > 0 => {
                            current.duration = track.duration_ms; metadata = None;
                        }
                        Event::Playing { position_ms, .. } | Event::Position { position_ms, .. }
                        | Event::Paused { position_ms, .. } => {
                            current.played |= *position_ms > current.position || matches!(event, Event::Playing { .. });
                            current.position = *position_ms;
                        }
                        _ => (),
                    }
                    // Hold an EOF until a pending duration lookup completes,
                    // while keeping pause, seek, skip and stop responsive.
                    if matches!(event, Event::Completed(_)) && current.duration == 0 && metadata.is_some() {
                        *generation(&mut event).unwrap() = current.attempt_generation;
                        deferred = Some(event);
                        continue;
                    }
                    let early = matches!(event, Event::Completed(_)) && current.ended_early();
                    let interrupted = matches!(event, Event::TrackError { .. }) && current.played;
                    if early || interrupted {
                        #[cfg(test)]
                        if std::env::var_os("TUITIFY_LIVE_TRACE").is_some() {
                            eprintln!("BACKEND TERMINAL at {} / {} ms: {event:?}", current.position, current.duration);
                        }
                        if current.retries < MAX_RETRIES {
                            current.retries += 1;
                            serial = serial.wrapping_add(1);
                            current.attempt_generation = serial;
                            let _ = events.send(Event::Recovering { generation: current.generation,
                                position_ms: current.position, attempt: current.retries });
                            // Stop clears preloads and the failed decoder. Load
                            // resolves a fresh link using the original queue ID.
                            let _ = inner.commands.send(Command::Stop);
                            current.load(&inner);
                            continue;
                        }
                        if early {
                            event = Event::TrackError { generation: current.generation,
                                message: "Audio stream ended before the song finished after two reconnect attempts. Queue preserved; press Space to retry or n for the next song.".into() };
                        }
                    }
                    if matches!(event, Event::Completed(_) | Event::TrackError { .. }) {
                        intent = None;
                        metadata = None;
                    }
                }
                if matches!(event, Event::Error(_) | Event::PremiumRequired) {
                    intent = None;
                    metadata = None;
                    deferred = None;
                }
                let _ = events.send(event);
            }
        }
    }
}

#[cfg(test)]
mod tests;
