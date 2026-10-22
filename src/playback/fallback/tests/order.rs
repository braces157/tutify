use super::*;

#[tokio::test]
async fn closed_primary_worker_keeps_fallback_alive_for_current_and_next_tracks() {
    let (mono, mut mono_commands, mono_events) = backend();
    let (volta, mut volta_commands, volta_events) = backend();
    let mut player = mono.with_fallback(volta, "Volta");
    player
        .commands
        .send(Command::Load {
            id: "deezer:1".into(),
            position_ms: 1000,
            generation: 7,
        })
        .unwrap();
    loaded(&mut mono_commands, "deezer:1", 1000, 7).await;
    mono_events
        .send(Event::TrackError {
            generation: 7,
            message: "worker failed".into(),
        })
        .unwrap();
    drop(mono_events);
    super::deezer::fallback_event(&mut player.events, "Volta").await;
    loaded(&mut volta_commands, "deezer:1", 1000, 7).await;
    volta_events
        .send(Event::Playing {
            generation: 7,
            position_ms: 1000,
        })
        .unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Playing { generation: 7, .. }
    ));
    player
        .commands
        .send(Command::Load {
            id: "deezer:2".into(),
            position_ms: 0,
            generation: 8,
        })
        .unwrap();
    loaded(&mut volta_commands, "deezer:2", 0, 8).await;
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Fallback {
            generation: 8,
            source: "Volta"
        }
    ));
    volta_events
        .send(Event::Playing {
            generation: 8,
            position_ms: 0,
        })
        .unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Playing { generation: 8, .. }
    ));
    player.commands.send(Command::Volume(25)).unwrap();
    assert!(matches!(
        recv(&mut volta_commands).await,
        Command::Volume(25)
    ));
    volta_events.send(Event::Volume(25)).unwrap();
    assert!(matches!(recv(&mut player.events).await, Event::Volume(25)));
}

async fn loaded(
    commands: &mut mpsc::UnboundedReceiver<Command>,
    id: &str,
    position: u32,
    generation: u64,
) {
    loop {
        if let Command::Load {
            id: actual,
            position_ms,
            generation: actual_generation,
        } = recv(commands).await
        {
            assert_eq!(actual, id);
            assert_eq!(position_ms, position);
            assert_eq!(actual_generation, generation);
            return;
        }
    }
}

#[tokio::test]
async fn monochrome_volta_spotify_youtube_order_preserves_controls_and_restarts_at_primary() {
    for id in [
        "deezer:1",
        "0000000000000000000001",
        "volta:USABC2400001",
        "youtube:aaaaaaaaaaa",
    ] {
        let (mono, mut mono_commands, mono_events) = backend();
        let (volta, mut volta_commands, volta_events) = backend();
        let (spotify, mut spotify_commands, spotify_events) = backend();
        let (youtube, mut youtube_commands, youtube_events) = backend();
        let mut player = mono
            .with_fallback(volta, "Volta")
            .with_fallback(spotify, "Spotify")
            .with_fallback(youtube, "YouTube");
        player
            .commands
            .send(Command::Load {
                id: id.into(),
                position_ms: 1234,
                generation: 7,
            })
            .unwrap();
        loaded(&mut mono_commands, id, 1234, 7).await;
        player.commands.send(Command::Pause).unwrap();
        player.commands.send(Command::Seek(5000)).unwrap();
        assert!(matches!(recv(&mut mono_commands).await, Command::Pause));
        assert!(matches!(
            recv(&mut mono_commands).await,
            Command::Seek(5000)
        ));
        mono_events
            .send(Event::TrackError {
                generation: 7,
                message: "Monochrome HTTP 429".into(),
            })
            .unwrap();
        super::deezer::fallback_event(&mut player.events, "Volta").await;
        loaded(&mut volta_commands, id, 5000, 7).await;
        assert!(matches!(recv(&mut volta_commands).await, Command::Pause));
        assert!(!matches!(
            spotify_commands.try_recv(),
            Ok(Command::Load { .. })
        ));
        volta_events
            .send(Event::TrackError {
                generation: 7,
                message: "Volta unavailable".into(),
            })
            .unwrap();
        super::deezer::fallback_event(&mut player.events, "Spotify").await;
        loaded(&mut spotify_commands, id, 5000, 7).await;
        assert!(matches!(recv(&mut spotify_commands).await, Command::Pause));
        spotify_events.send(Event::PremiumRequired).unwrap();
        super::deezer::fallback_event(&mut player.events, "YouTube").await;
        loaded(&mut youtube_commands, id, 5000, 7).await;
        assert!(matches!(recv(&mut youtube_commands).await, Command::Pause));
        mono_events.send(Event::Completed(7)).unwrap();
        volta_events.send(Event::Completed(7)).unwrap();
        spotify_events.send(Event::Completed(7)).unwrap();
        youtube_events
            .send(Event::Paused {
                generation: 7,
                position_ms: 5000,
            })
            .unwrap();
        assert!(matches!(
            recv(&mut player.events).await,
            Event::Paused {
                generation: 7,
                position_ms: 5000
            }
        ));
        player
            .commands
            .send(Command::Load {
                id: id.into(),
                position_ms: 0,
                generation: 8,
            })
            .unwrap();
        loaded(&mut mono_commands, id, 0, 8).await;
        youtube_events.send(Event::Completed(7)).unwrap();
        mono_events
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
}
