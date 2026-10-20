//! Translate catalog IDs without blocking controls or changing queue identities.
use super::{Command, Event, Playback};
use crate::{catalog::Catalog, model::Track};
use std::collections::HashMap;
use tokio::{sync::mpsc, task::JoinHandle};

struct Pending {
    id: String,
    task: JoinHandle<anyhow::Result<(Track, String)>>,
    preload: bool,
}

#[cfg(test)]
mod tests;
impl Drop for Pending {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Intent {
    id: String,
    generation: u64,
    position: u32,
    paused: bool,
    resolving: bool,
    metadata: Option<Track>,
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

fn resolve(catalog: &Catalog, id: String, preload: bool) -> Pending {
    let client = catalog.clone();
    let query = id.clone();
    Pending {
        id,
        preload,
        task: tokio::spawn(async move { client.resolve_playback_track(&query).await }),
    }
}

fn load(
    inner: &Playback,
    events: &mpsc::UnboundedSender<Event>,
    intent: &mut Intent,
    resolved: (Track, String),
) {
    let (track, id) = resolved;
    intent.resolving = false;
    intent.metadata = Some(track.clone());
    let _ = events.send(Event::Metadata {
        generation: intent.generation,
        track,
    });
    let _ = inner.commands.send(Command::Load {
        id,
        position_ms: intent.position,
        generation: intent.generation,
    });
    if intent.paused {
        let _ = inner.commands.send(Command::Pause);
    }
}

async fn route(
    mut inner: Playback,
    catalog: Catalog,
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<Event>,
) {
    let mut pending: Option<Pending> = None;
    let mut intent: Option<Intent> = None;
    let mut matches: HashMap<String, (Track, String)> = HashMap::new();
    loop {
        tokio::select! {
            biased;
            command = commands.recv() => {
                let Some(command) = command else { break; };
                match command {
                    Command::Load { id, position_ms, generation } => {
                        intent = Some(Intent { id: id.clone(), generation, position: position_ms, paused: false,
                            resolving: crate::deezer::key(&id).is_some(), metadata: None });
                        if crate::deezer::key(&id).is_none() {
                            pending = None;
                            let _ = inner.commands.send(Command::Load { id, position_ms, generation });
                        } else if let Some(resolved) = matches.get(&id).cloned() {
                            pending = None;
                            load(&inner, &events, intent.as_mut().unwrap(), resolved);
                        } else {
                            let _ = inner.commands.send(Command::Stop);
                            if let Some(job) = pending.as_mut().filter(|job| job.id == id) {
                                job.preload = false;
                            } else {
                                pending = Some(resolve(&catalog, id, false));
                            }
                        }
                    }
                    Command::Preload { id } => {
                        if intent.as_ref().is_some_and(|current| current.resolving) { continue; }
                        if crate::deezer::key(&id).is_none() {
                            pending = None;
                            let _ = inner.commands.send(Command::Preload { id });
                        } else if let Some((_, audio)) = matches.get(&id) {
                            pending = None;
                            let _ = inner.commands.send(Command::Preload { id: audio.clone() });
                        } else if pending.as_ref().is_none_or(|job| job.id != id) {
                            let _ = inner.commands.send(Command::CancelPreload);
                            pending = Some(resolve(&catalog, id, true));
                        }
                    }
                    Command::CancelPreload => {
                        if pending.as_ref().is_some_and(|job| job.preload) { pending = None; }
                        let _ = inner.commands.send(Command::CancelPreload);
                    }
                    Command::Stop => {
                        pending = None;
                        intent = None;
                        let _ = inner.commands.send(Command::Stop);
                    }
                    command => {
                        if let Some(current) = &mut intent {
                            match command {
                                Command::Pause => { current.paused = true;
                                    if current.resolving { let _ = events.send(Event::Paused { generation: current.generation, position_ms: current.position }); }
                                }
                                Command::Resume => current.paused = false,
                                Command::Seek(position) => { current.position = position;
                                    if current.resolving { let _ = events.send(Event::Position { generation: current.generation, position_ms: position }); }
                                }
                                _ => (),
                            }
                            if current.resolving && !matches!(command, Command::Volume(_)) { continue; }
                        }
                        let _ = inner.commands.send(command);
                    }
                }
            }
            resolved = async { (&mut pending.as_mut().unwrap().task).await }, if pending.is_some() => {
                let job = pending.take().unwrap();
                match resolved {
                    Ok(Ok(resolved)) => {
                        if matches.len() >= 128 { matches.clear(); }
                        matches.insert(job.id.clone(), resolved.clone());
                        if job.preload {
                            let _ = inner.commands.send(Command::Preload { id: resolved.1 });
                        } else if let Some(current) = intent.as_mut().filter(|current| current.id == job.id) {
                            load(&inner, &events, current, resolved);
                        }
                    }
                    failure if !job.preload => {
                        if let Some(current) = intent.as_mut().filter(|current| current.id == job.id) {
                            current.resolving = false;
                            let message = match failure { Ok(Err(error)) => format!("{error:#}"), _ => "Recording lookup worker stopped".into() };
                            let _ = events.send(Event::TrackError { generation: current.generation, message });
                        }
                    }
                    _ => (), // A failed preload must not interrupt the playing song.
                }
            }
            event = inner.events.recv() => {
                let Some(mut event) = event else { break; };
                if let Event::Metadata { generation, track } = &mut event
                    && let Some(original) = intent.as_ref().filter(|current| current.generation == *generation).and_then(|current| current.metadata.as_ref()) {
                    *track = original.clone();
                }
                if let Some(current) = &mut intent {
                    match &event {
                        Event::Playing { generation, position_ms } | Event::Paused { generation, position_ms }
                        | Event::Position { generation, position_ms } if *generation == current.generation => current.position = *position_ms,
                        _ => (),
                    }
                }
                let _ = events.send(event);
            }
        }
    }
}
