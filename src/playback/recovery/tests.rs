use super::*;
use crate::model::Track;

const ID: &str = "youtube:dQw4w9WgXcQ";

#[tokio::test]
#[ignore = "Full live song through production providers, muted Windows output, no queue/stat writes"]
async fn live_complete_track_diagnostic() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    let choice = match std::env::var("TUITIFY_LIVE_SOURCE").as_deref() {
        Ok("youtube") => crate::source::Choice::Youtube,
        Ok("volta") => crate::source::Choice::Volta,
        Ok("spotify") => crate::source::Choice::Spotify,
        _ => crate::source::Choice::Auto,
    };
    let selection = crate::source::resolve(choice, false).await?;
    let mut config = crate::storage::Storage::local_read_only()?.config()?;
    config.automatic_audio = choice == crate::source::Choice::Auto;
    config.volume = 0;
    config.discord_rpc = false;
    let visualizer = crate::visualizer::AudioVisualizer::new();
    let mut services = crate::providers::open(selection.source, &config, visualizer.clone())?;
    let id = std::env::var("TUITIFY_LIVE_TRACK").unwrap_or_else(|_| "deezer:2493747201".into());
    let track = services.catalog.track(&id).await?;
    println!(
        "LIVE {} / {} | {} | {} | expected {:.3}s",
        track.name,
        track.artists,
        id,
        selection.source.label(),
        f64::from(track.duration_ms) / 1000.0
    );
    let started = std::time::Instant::now();
    let preload = std::env::var("TUITIFY_LIVE_PRELOAD").ok();
    services.playback.commands.send(Command::Load {
        id,
        position_ms: 0,
        generation: 101,
    })?;
    tokio::time::timeout(
        Duration::from_millis(u64::from(track.duration_ms) + 150_000),
        async {
            let mut position = 0;
            let mut progress_log = 0;
            let mut recoveries = 0;
            let mut preloaded = false;
            loop {
                match services
                    .playback
                    .events
                    .recv()
                    .await
                    .context("Playback worker stopped")?
                {
                    Event::Metadata { track, .. } => println!(
                        "{:6.2}s METADATA {} / {} | {}ms",
                        started.elapsed().as_secs_f64(),
                        track.name,
                        track.artists,
                        track.duration_ms
                    ),
                    Event::Fallback { source, .. } => {
                        println!("{:6.2}s FALLBACK {source}", started.elapsed().as_secs_f64())
                    }
                    Event::Quality { quality, .. } => println!(
                        "{:6.2}s QUALITY {}",
                        started.elapsed().as_secs_f64(),
                        quality.label()
                    ),
                    Event::Playing { position_ms, .. } => println!(
                        "{:6.2}s PLAYING at {position_ms}ms",
                        started.elapsed().as_secs_f64()
                    ),
                    Event::Position { position_ms, .. } => {
                        position = position_ms;
                        if position >= progress_log {
                            println!(
                                "{:6.2}s PROGRESS {position}ms",
                                started.elapsed().as_secs_f64()
                            );
                            progress_log = position.saturating_add(15_000);
                        }
                    }
                    Event::TimeToPreload { .. } if !preloaded => {
                        if let Some(id) = &preload {
                            println!("{:6.2}s PRELOAD {id}", started.elapsed().as_secs_f64());
                            services
                                .playback
                                .commands
                                .send(Command::Preload { id: id.clone() })?;
                            preloaded = true;
                        }
                    }
                    Event::Recovering {
                        position_ms,
                        attempt,
                        ..
                    } => {
                        recoveries += 1;
                        println!(
                            "{:6.2}s RECOVERY {attempt} at {position_ms}ms",
                            started.elapsed().as_secs_f64()
                        );
                        preloaded = false;
                    }
                    Event::Completed(_) => {
                        println!(
                            "{:6.2}s COMPLETED {position} / {}ms | {recoveries} recoveries",
                            started.elapsed().as_secs_f64(),
                            track.duration_ms
                        );
                        ensure!(
                            position.saturating_add(END_TOLERANCE_MS) >= track.duration_ms,
                            "Song completed early"
                        );
                        ensure!(
                            visualizer.has_audio_samples(),
                            "No PCM reached Windows output"
                        );
                        break;
                    }
                    Event::Error(message) | Event::TrackError { message, .. } => anyhow::bail!(
                        "{:6.2}s FAILED at {position}ms: {message}",
                        started.elapsed().as_secs_f64()
                    ),
                    _ => (),
                }
            }
            Ok::<_, anyhow::Error>(())
        },
    )
    .await
    .context("Full-track diagnostic timed out")??;
    services.playback.commands.send(Command::Stop)?;
    Ok(())
}

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

async fn load(
    player: &Playback,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    id: &str,
    external: u64,
) -> u64 {
    player
        .commands
        .send(Command::Load {
            id: id.into(),
            position_ms: 0,
            generation: external,
        })
        .unwrap();
    match recv(commands).await {
        Command::Load {
            id: loaded,
            generation,
            position_ms: 0,
        } => {
            assert_eq!(loaded, id);
            generation
        }
        command => panic!("unexpected {command:?}"),
    }
}

async fn playing(
    player: &mut Playback,
    events: &mpsc::UnboundedSender<Event>,
    backend: u64,
    position: u32,
    duration: u32,
) {
    events
        .send(Event::Metadata {
            generation: backend,
            track: Track {
                id: ID.into(),
                duration_ms: duration,
                ..Default::default()
            },
        })
        .unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Metadata { .. }
    ));
    events
        .send(Event::Playing {
            generation: backend,
            position_ms: position,
        })
        .unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Playing { .. }
    ));
}

#[tokio::test]
async fn premature_eof_resumes_original_identity_and_rejects_old_decoder_events() {
    let (inner, mut commands, events) = backend();
    let mut player = spawn(inner, Catalog::mock("http://127.0.0.1:9"));
    // The translated audio ID is irrelevant to recovery: reload the original ID.
    let first = load(&player, &mut commands, "deezer:142986200", 72).await;
    playing(&mut player, &events, first, 90_125, 240_000).await;
    events.send(Event::Completed(first)).unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Recovering {
            generation: 72,
            position_ms: 90_125,
            attempt: 1
        }
    ));
    assert!(matches!(recv(&mut commands).await, Command::Stop));
    let second = match recv(&mut commands).await {
        Command::Load {
            id,
            position_ms: 90_125,
            generation,
        } => {
            assert_eq!(id, "deezer:142986200");
            assert_ne!(generation, first);
            generation
        }
        command => panic!("unexpected {command:?}"),
    };
    events.send(Event::Completed(first)).unwrap();
    events
        .send(Event::TrackError {
            generation: first,
            message: "late decoder error".into(),
        })
        .unwrap();
    events
        .send(Event::Position {
            generation: first,
            position_ms: 240_000,
        })
        .unwrap();
    events
        .send(Event::Playing {
            generation: second,
            position_ms: 90_125,
        })
        .unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Playing {
            generation: 72,
            position_ms: 90_125
        }
    ));
    events
        .send(Event::Position {
            generation: second,
            position_ms: 239_900,
        })
        .unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Position { generation: 72, .. }
    ));
    events.send(Event::Completed(second)).unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Completed(72)
    ));
    assert!(commands.try_recv().is_err());
}

#[tokio::test]
async fn repeated_cuts_are_bounded_and_never_complete_the_song() {
    let (inner, mut commands, events) = backend();
    let mut player = spawn(inner, Catalog::mock("http://127.0.0.1:9"));
    let mut attempt = load(&player, &mut commands, ID, 8).await;
    playing(&mut player, &events, attempt, 80_000, 200_000).await;
    for retry in 1..=2 {
        events.send(Event::Completed(attempt)).unwrap();
        assert!(
            matches!(recv(&mut player.events).await, Event::Recovering { generation: 8, attempt, .. } if attempt == retry)
        );
        assert!(matches!(recv(&mut commands).await, Command::Stop));
        attempt = match recv(&mut commands).await {
            Command::Load {
                generation,
                position_ms: 80_000,
                ..
            } => generation,
            command => panic!("unexpected {command:?}"),
        };
    }
    events.send(Event::Completed(attempt)).unwrap();
    assert!(matches!(recv(&mut player.events).await,
        Event::TrackError { generation: 8, message } if message.contains("two reconnect attempts")));
    // Duplicate EOF after failure cannot advance the queue either.
    events.send(Event::Completed(attempt)).unwrap();
    events.send(Event::Volume(41)).unwrap();
    assert!(matches!(recv(&mut player.events).await, Event::Volume(41)));
    assert!(commands.try_recv().is_err());
}

#[tokio::test]
async fn matched_duration_drift_and_codec_padding_finish_without_recovery() {
    let (inner, mut commands, events) = backend();
    let mut player = spawn(inner, Catalog::mock("http://127.0.0.1:9"));
    for (generation, shortfall) in [(11, 5500), (12, 6000), (13, 6999)] {
        let attempt = load(&player, &mut commands, ID, generation).await;
        playing(&mut player, &events, attempt, 300_000 - shortfall, 300_000).await;
        events.send(Event::Completed(attempt)).unwrap();
        assert!(matches!(recv(&mut player.events).await, Event::Completed(g) if g == generation));
        assert!(commands.try_recv().is_err());
    }
}

#[tokio::test]
async fn recovery_preserves_pause_and_seek_and_manual_skip_cancels_it() {
    let (inner, mut commands, events) = backend();
    let mut player = spawn(inner, Catalog::mock("http://127.0.0.1:9"));
    let first = load(&player, &mut commands, ID, 5).await;
    playing(&mut player, &events, first, 50_000, 180_000).await;
    player.commands.send(Command::Pause).unwrap();
    assert!(matches!(recv(&mut commands).await, Command::Pause));
    player.commands.send(Command::Seek(70_000)).unwrap();
    assert!(matches!(recv(&mut commands).await, Command::Seek(70_000)));
    events
        .send(Event::TrackError {
            generation: first,
            message: "connection lost".into(),
        })
        .unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Recovering {
            generation: 5,
            position_ms: 70_000,
            attempt: 1
        }
    ));
    assert!(matches!(recv(&mut commands).await, Command::Stop));
    let second = match recv(&mut commands).await {
        Command::Load {
            position_ms: 70_000,
            generation,
            ..
        } => generation,
        command => panic!("unexpected {command:?}"),
    };
    assert!(matches!(recv(&mut commands).await, Command::Pause));
    let next = load(&player, &mut commands, ID, 6).await;
    events.send(Event::Completed(second)).unwrap();
    events
        .send(Event::Playing {
            generation: next,
            position_ms: 0,
        })
        .unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Playing { generation: 6, .. }
    ));
    player.commands.send(Command::Stop).unwrap();
    assert!(matches!(recv(&mut commands).await, Command::Stop));
    events.send(Event::Completed(next)).unwrap();
    events.send(Event::Volume(20)).unwrap();
    assert!(matches!(recv(&mut player.events).await, Event::Volume(20)));
}

#[tokio::test]
async fn natural_end_short_tracks_unknown_duration_and_initial_failure_do_not_retry() {
    let (inner, mut commands, events) = backend();
    let mut player = spawn(inner, Catalog::mock("http://127.0.0.1:9"));
    for (external, duration, position) in [(1, 180_000, 179_950), (2, 2_000, 1_950), (3, 0, 50_000)]
    {
        let attempt = load(&player, &mut commands, ID, external).await;
        playing(&mut player, &events, attempt, position, duration).await;
        events.send(Event::Completed(attempt)).unwrap();
        assert!(matches!(recv(&mut player.events).await, Event::Completed(g) if g == external));
        assert!(commands.try_recv().is_err());
    }
    let attempt = load(&player, &mut commands, ID, 4).await;
    events
        .send(Event::TrackError {
            generation: attempt,
            message: "HTTP 429".into(),
        })
        .unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::TrackError { generation: 4, .. }
    ));
    assert!(commands.try_recv().is_err());
}

#[tokio::test]
async fn native_duration_lookup_defers_eof_without_blocking_controls() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
    let server = MockServer::start().await;
    let id = "1".repeat(22);
    Mock::given(path(format!("/tracks/{id}")))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(100)).set_body_json(
            serde_json::json!({"id":id,"name":"Song","artists":[{"name":"Artist"}],"duration_ms":180000,"is_playable":true})))
        .expect(1).mount(&server).await;
    let (inner, mut commands, events) = backend();
    let mut player = spawn(inner, Catalog::mock(&server.uri()));
    let attempt = load(&player, &mut commands, &id, 9).await;
    events
        .send(Event::Playing {
            generation: attempt,
            position_ms: 60_000,
        })
        .unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Playing { .. }
    ));
    events.send(Event::Completed(attempt)).unwrap();
    player.commands.send(Command::Pause).unwrap();
    assert!(matches!(recv(&mut commands).await, Command::Pause));
    events
        .send(Event::Paused {
            generation: attempt,
            position_ms: 60_000,
        })
        .unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Paused { generation: 9, .. }
    ));
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Recovering { generation: 9, .. }
    ));
    assert!(matches!(recv(&mut commands).await, Command::Stop));
    assert!(matches!(
        recv(&mut commands).await,
        Command::Load {
            position_ms: 60_000,
            ..
        }
    ));
    assert!(matches!(recv(&mut commands).await, Command::Pause));
    server.verify().await;
}

#[tokio::test]
#[ignore = "Installed FFmpeg and Windows audio device; local truncated FLAC, muted output, no saved-state writes"]
async fn real_truncated_flac_reconnects_and_finishes_same_song() -> anyhow::Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
    let ffmpeg = crate::volta::playback::ffmpeg()?;
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("full.flac");
    let generated = crate::youtube::hidden_command(&ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=8",
            "-ac",
            "2",
            "-c:a",
            "flac",
        ])
        .arg(&file)
        .output()
        .await?;
    anyhow::ensure!(
        generated.status.success(),
        "Could not generate FLAC fixture"
    );
    let full = std::fs::read(file)?;
    let cut = full[..full.len() / 5].to_vec();
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/oauth/token")).respond_with(ResponseTemplate::new(200).set_body_json(
        serde_json::json!({"access_token":"volta_at_test","scope":"catalog:read stream:read","expires_in":3600})))
        .mount(&server).await;
    Mock::given(path("/api/v1/track/cut")).respond_with(ResponseTemplate::new(200).set_body_json(
        serde_json::json!({"id":"cut","name":"Local fixture","artist":"Fixture","duration_ms":8000})))
        .mount(&server).await;
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    Mock::given(path("/api/v1/stream"))
        .respond_with(move |_: &wiremock::Request| {
            let link = if count.fetch_add(1, Ordering::SeqCst) == 0 {
                "/cut"
            } else {
                "/full"
            };
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"stream_url":link,"sample_rate":48000}))
        })
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path("/cut"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(cut))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/full"))
        .respond_with(move |request: &wiremock::Request| {
            let range = request
                .headers
                .get("range")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("bytes="))
                .and_then(|value| value.split_once('-'))
                .and_then(|(start, _)| start.parse::<usize>().ok());
            match range {
                Some(start) if start < full.len() => ResponseTemplate::new(206)
                    .insert_header("accept-ranges", "bytes")
                    .insert_header(
                        "content-range",
                        format!("bytes {start}-{}/{}", full.len() - 1, full.len()),
                    )
                    .set_body_bytes(full[start..].to_vec()),
                _ => ResponseTemplate::new(200)
                    .insert_header("accept-ranges", "bytes")
                    .set_body_bytes(full.clone()),
            }
        })
        .expect(1..)
        .mount(&server)
        .await;
    let client = crate::volta::Client::mock(&format!("{}/api/v1", server.uri()));
    let inner =
        Playback::spawn_volta(client.clone(), 0, crate::visualizer::AudioVisualizer::new())?;
    let mut player = spawn(inner, Catalog::volta(client)?);
    player.commands.send(Command::Load {
        id: "volta:cut".into(),
        position_ms: 0,
        generation: 47,
    })?;
    tokio::time::timeout(Duration::from_secs(35), async {
        let mut recovered_at = None;
        let mut resumed = false;
        let mut position = 0;
        loop {
            match player.events.recv().await.unwrap() {
                Event::Recovering {
                    generation: 47,
                    position_ms,
                    attempt: 1,
                } => {
                    assert!(position_ms > 0 && position_ms < 3_000);
                    assert!(recovered_at.replace(position_ms).is_none());
                }
                Event::Playing {
                    generation: 47,
                    position_ms,
                } if recovered_at.is_some() => {
                    assert!(position_ms >= recovered_at.unwrap());
                    resumed = true;
                }
                Event::Position {
                    generation: 47,
                    position_ms,
                } => position = position_ms,
                Event::Completed(47) => {
                    assert!(recovered_at.is_some() && resumed);
                    assert!(position >= 7_900, "only {position} ms of the song played");
                    break;
                }
                Event::Error(message) | Event::TrackError { message, .. } => panic!("{message}"),
                Event::Recovering {
                    attempt,
                    position_ms,
                    ..
                } => panic!("unexpected recovery {attempt} at {position_ms} ms"),
                _ => (),
            }
        }
    })
    .await?;
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    player.commands.send(Command::Stop)?;
    server.verify().await;
    Ok(())
}
