//! Keep catalog identities and controls intact when Volta/YouTube cannot play a song.
use super::{Command, Event, Playback};
use tokio::sync::mpsc;

struct Intent {
    id: String,
    generation: u64,
    position: u32,
    paused: bool,
    using_fallback: bool,
    primary_error: String,
}

pub(super) fn spawn(primary: Playback, secondary: Playback) -> Playback {
    spawn_with_source(primary, secondary, "Spotify", crate::model::valid_id)
}

pub(super) fn spawn_deezer(
    primary: Playback,
    fallback: Playback,
    source: &'static str,
) -> Playback {
    spawn_with_source(primary, fallback, source, |id| {
        crate::deezer::key(id).is_some()
    })
}

pub(super) fn spawn_any(primary: Playback, fallback: Playback, source: &'static str) -> Playback {
    spawn_with_source(primary, fallback, source, crate::model::valid_track_id)
}

fn spawn_with_source(
    primary: Playback,
    fallback: Playback,
    source: &'static str,
    accepts: fn(&str) -> bool,
) -> Playback {
    let (commands, rx) = mpsc::unbounded_channel();
    let (tx, events) = mpsc::unbounded_channel();
    let task = tokio::spawn(route(primary, fallback, rx, tx, source, accepts));
    Playback {
        commands,
        events,
        task,
    }
}

fn generation(event: &Event) -> Option<u64> {
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
        | Event::Completed(generation) => Some(*generation),
        _ => None,
    }
}

async fn route(
    mut primary: Playback,
    mut secondary: Playback,
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<Event>,
    source: &'static str,
    accepts: fn(&str) -> bool,
) {
    let mut intent: Option<Intent> = None;
    let mut primary_open = true;
    let mut fallback_available = true;
    let mut fallback_open = true;
    loop {
        tokio::select! {
            biased;
            command = commands.recv() => {
                let Some(command) = command else { break; };
                match &command {
                    Command::Load { id, position_ms, generation } => {
                        let _ = secondary.commands.send(Command::Stop);
                        intent = Some(Intent { id: id.clone(), generation: *generation, position: *position_ms, paused: false, using_fallback: false, primary_error: String::new() });
                        if !primary_open {
                            if fallback_available && accepts(id) {
                                switch(intent.as_mut().unwrap(), &primary, &secondary, &events, source, "Primary audio worker stopped".into());
                            } else {
                                let _ = events.send(Event::TrackError { generation: *generation, message: "Audio playback worker stopped; restart Tuitify".into() });
                            }
                            continue;
                        }
                    }
                    Command::Stop => {
                        intent = None;
                        let _ = primary.commands.send(Command::Stop);
                        let _ = secondary.commands.send(Command::Stop);
                        continue;
                    }
                    Command::Volume(_) => {
                        let _ = primary.commands.send(command.clone());
                        let _ = secondary.commands.send(command);
                        continue;
                    }
                    Command::Preload { .. } | Command::CancelPreload => {
                        let _ = primary.commands.send(command.clone());
                        if intent.as_ref().is_some_and(|current| current.using_fallback) {
                            let _ = secondary.commands.send(command);
                        }
                        continue;
                    }
                    Command::Pause => { if let Some(current) = &mut intent { current.paused = true; } }
                    Command::Resume => { if let Some(current) = &mut intent { current.paused = false; } }
                    Command::Seek(position) => { if let Some(current) = &mut intent { current.position = *position; } }
                    #[cfg(test)]
                    Command::SimulateDisconnect => (),
                }
                let backend = if intent.as_ref().is_some_and(|current| current.using_fallback) { &secondary } else { &primary };
                let _ = backend.commands.send(command);
            }
            event = primary.events.recv(), if primary_open => {
                let Some(event) = event else {
                    primary_open = false;
                    if let Some(current) = &mut intent && !current.using_fallback {
                        if fallback_available && accepts(&current.id) {
                            switch(current, &primary, &secondary, &events, source, "Primary audio worker stopped".into());
                        } else {
                            let _ = events.send(Event::TrackError { generation: current.generation, message: "Audio playback worker stopped; restart Tuitify".into() });
                        }
                    }
                    continue;
                };
                // Both backends receive volume commands; acknowledge exactly once.
                if matches!(event, Event::Volume(_)) {
                    let _ = events.send(event);
                    continue;
                }
                if intent.as_ref().is_some_and(|current| current.using_fallback) { continue; }
                if generation(&event).is_some_and(|g| intent.as_ref().is_none_or(|current| current.generation != g)) { continue; }
                if matches!(event, Event::TrackError { .. } | Event::Error(_))
                    && let Some(current) = &mut intent
                    && fallback_available && accepts(&current.id)
                {
                    let message = match event { Event::TrackError { message, .. } | Event::Error(message) => message, _ => unreachable!() };
                    switch(current, &primary, &secondary, &events, source, message);
                    continue;
                }
                update_position(&mut intent, &event);
                let _ = events.send(event);
            }
            event = secondary.events.recv(), if fallback_open => {
                let Some(event) = event else {
                    fallback_available = false;
                    fallback_open = false;
                    if let Some(current) = intent.as_ref().filter(|current| current.using_fallback) {
                        let _ = events.send(Event::TrackError { generation: current.generation, message: format!("{source} fallback worker stopped; restart Tuitify") });
                    }
                    continue;
                };
                if matches!(event, Event::Volume(_)) {
                    if !primary_open { let _ = events.send(event); }
                    continue;
                }
                let Some(current) = &intent else { continue; };
                if !current.using_fallback || generation(&event).is_some_and(|g| g != current.generation) { continue; }
                let event = match event {
                    Event::PremiumRequired => {
                        fallback_available = false;
                        Event::TrackError { generation: current.generation, message: format!("{}; Spotify fallback requires Premium", current.primary_error) }
                    }
                    Event::TrackError { message, .. } | Event::Error(message) => Event::TrackError {
                        generation: current.generation,
                        message: format!("{}; {source} fallback failed: {message}", current.primary_error),
                    },
                    other => other,
                };
                update_position(&mut intent, &event);
                let _ = events.send(event);
            }
        }
    }
}

fn switch(
    current: &mut Intent,
    primary: &Playback,
    secondary: &Playback,
    events: &mpsc::UnboundedSender<Event>,
    source: &'static str,
    message: String,
) {
    current.primary_error = message;
    current.using_fallback = true;
    let _ = primary.commands.send(Command::Stop);
    let _ = events.send(Event::Fallback {
        generation: current.generation,
        source,
    });
    let _ = secondary.commands.send(Command::Load {
        id: current.id.clone(),
        position_ms: current.position,
        generation: current.generation,
    });
    if current.paused {
        let _ = secondary.commands.send(Command::Pause);
    }
}

fn update_position(intent: &mut Option<Intent>, event: &Event) {
    if let Some(current) = intent
        && let Event::Position { position_ms, .. }
        | Event::Playing { position_ms, .. }
        | Event::Paused { position_ms, .. } = event
    {
        current.position = *position_ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    mod deezer;
    mod order;

    fn backend() -> (
        Playback,
        mpsc::UnboundedReceiver<Command>,
        mpsc::UnboundedSender<Event>,
    ) {
        let (commands, rx) = mpsc::unbounded_channel();
        let (tx, events) = mpsc::unbounded_channel();
        (
            Playback {
                commands,
                events,
                task: tokio::spawn(std::future::pending()),
            },
            rx,
            tx,
        )
    }
    async fn recv<T>(rx: &mut mpsc::UnboundedReceiver<T>) -> T {
        tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn fallback_keeps_identity_position_pause_and_rejects_old_events() {
        let (primary, mut primary_commands, primary_events) = backend();
        let (spotify, mut spotify_commands, spotify_events) = backend();
        let mut player = spawn(primary, spotify);
        let id = "1".repeat(22);
        player
            .commands
            .send(Command::Load {
                id: id.clone(),
                position_ms: 0,
                generation: 7,
            })
            .unwrap();
        assert!(matches!(
            recv(&mut primary_commands).await,
            Command::Load { generation: 7, .. }
        ));
        assert!(matches!(recv(&mut spotify_commands).await, Command::Stop));
        primary_events
            .send(Event::Playing {
                generation: 7,
                position_ms: 1234,
            })
            .unwrap();
        assert!(matches!(
            recv(&mut player.events).await,
            Event::Playing { generation: 7, .. }
        ));
        player.commands.send(Command::Pause).unwrap();
        assert!(matches!(recv(&mut primary_commands).await, Command::Pause));
        primary_events
            .send(Event::TrackError {
                generation: 6,
                message: "old".into(),
            })
            .unwrap();
        primary_events
            .send(Event::TrackError {
                generation: 7,
                message: "Unavailable".into(),
            })
            .unwrap();
        assert!(matches!(
            recv(&mut player.events).await,
            Event::Fallback {
                generation: 7,
                source: "Spotify"
            }
        ));
        assert!(matches!(recv(&mut primary_commands).await, Command::Stop));
        assert!(
            matches!(recv(&mut spotify_commands).await, Command::Load { id: actual, position_ms: 1234, generation: 7 } if actual == id)
        );
        assert!(matches!(recv(&mut spotify_commands).await, Command::Pause));
        primary_events.send(Event::Completed(7)).unwrap();
        spotify_events
            .send(Event::Paused {
                generation: 7,
                position_ms: 1234,
            })
            .unwrap();
        assert!(matches!(
            recv(&mut player.events).await,
            Event::Paused { generation: 7, .. }
        ));
        player.commands.send(Command::Volume(37)).unwrap();
        assert!(matches!(
            recv(&mut primary_commands).await,
            Command::Volume(37)
        ));
        assert!(matches!(
            recv(&mut spotify_commands).await,
            Command::Volume(37)
        ));
        primary_events.send(Event::Volume(37)).unwrap();
        spotify_events.send(Event::Volume(37)).unwrap();
        assert!(matches!(recv(&mut player.events).await, Event::Volume(37)));
        player
            .commands
            .send(Command::Load {
                id: "2".repeat(22),
                position_ms: 0,
                generation: 8,
            })
            .unwrap();
        assert!(matches!(
            recv(&mut primary_commands).await,
            Command::Load { generation: 8, .. }
        ));
        assert!(matches!(recv(&mut spotify_commands).await, Command::Stop));
        spotify_events.send(Event::Completed(7)).unwrap();
        primary_events
            .send(Event::Playing {
                generation: 8,
                position_ms: 0,
            })
            .unwrap();
        assert!(matches!(
            recv(&mut player.events).await,
            Event::Playing { generation: 8, .. }
        ));
    }

    #[tokio::test]
    async fn premium_failure_stays_in_the_same_session_and_disables_spotify_retry() {
        let (primary, mut primary_commands, primary_events) = backend();
        let (spotify, mut spotify_commands, spotify_events) = backend();
        let mut player = spawn(primary, spotify);
        player
            .commands
            .send(Command::Load {
                id: "1".repeat(22),
                position_ms: 0,
                generation: 1,
            })
            .unwrap();
        recv(&mut primary_commands).await;
        recv(&mut spotify_commands).await;
        primary_events
            .send(Event::TrackError {
                generation: 1,
                message: "No matching song".into(),
            })
            .unwrap();
        assert!(matches!(
            recv(&mut player.events).await,
            Event::Fallback { .. }
        ));
        recv(&mut spotify_commands).await;
        spotify_events.send(Event::PremiumRequired).unwrap();
        assert!(
            matches!(recv(&mut player.events).await, Event::TrackError { generation: 1, message } if message.contains("requires Premium") && message.contains("No matching song"))
        );
        player
            .commands
            .send(Command::Load {
                id: "2".repeat(22),
                position_ms: 0,
                generation: 2,
            })
            .unwrap();
        recv(&mut primary_commands).await; // Stop from the first fallback.
        recv(&mut primary_commands).await;
        recv(&mut spotify_commands).await;
        primary_events
            .send(Event::TrackError {
                generation: 2,
                message: "Still missing".into(),
            })
            .unwrap();
        assert!(matches!(
            recv(&mut player.events).await,
            Event::TrackError { generation: 2, .. }
        ));
        assert!(spotify_commands.try_recv().is_err());
    }

    #[tokio::test]
    async fn stop_cancels_both_backends_and_ignores_late_errors() {
        let (primary, mut primary_commands, primary_events) = backend();
        let (spotify, mut spotify_commands, _) = backend();
        let mut player = spawn(primary, spotify);
        player
            .commands
            .send(Command::Load {
                id: "1".repeat(22),
                position_ms: 0,
                generation: 1,
            })
            .unwrap();
        recv(&mut primary_commands).await;
        recv(&mut spotify_commands).await;
        player.commands.send(Command::Stop).unwrap();
        assert!(matches!(recv(&mut primary_commands).await, Command::Stop));
        assert!(matches!(recv(&mut spotify_commands).await, Command::Stop));
        primary_events
            .send(Event::TrackError {
                generation: 1,
                message: "Late failure".into(),
            })
            .unwrap();
        primary_events.send(Event::Ready).unwrap();
        assert!(matches!(recv(&mut player.events).await, Event::Ready));
        assert!(spotify_commands.try_recv().is_err());
    }
}
