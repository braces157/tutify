use super::*;

pub(super) fn fixture(dir: &std::path::Path, body: &str) -> Tools {
    let script = dir.join("tool.ps1");
    std::fs::write(
        &script,
        format!("\u{feff}[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)\n{body}\n"),
    )
    .unwrap();
    Tools {
        music: None,
        ytdlp: PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe"),
        deno: dir.join("deno.exe"),
        ffmpeg: dir.join("ffmpeg.exe"),
        prefix: vec![
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            script.to_string_lossy().into_owned(),
        ],
    }
}

#[tokio::test]
async fn search_process_preserves_arguments_and_paginates_without_a_shell() {
    let dir = tempfile::tempdir().unwrap();
    let args = dir.path().join("args.json");
    let value = serde_json::json!({"id":"dQw4w9WgXcQ", "title":"Unicode เพลง", "channel":"Artist", "duration":200});
    let tools = fixture(
        dir.path(),
        &format!(
            "ConvertTo-Json -InputObject @($args) | Set-Content -LiteralPath '{}' -Encoding utf8\n1..21 | ForEach-Object {{ [Console]::WriteLine('{}') }}",
            args.display().to_string().replace('\'', "''"),
            value.to_string().replace('\'', "''")
        ),
    );
    let query = "เพลง 'quoted' & $(throw 'must stay literal')";
    let page = tools.page(&Browse::Search(query.into()), 20).await.unwrap();
    let Rows::Tracks(tracks) = page.rows else {
        panic!("expected tracks")
    };
    assert_eq!(tracks.len(), PAGE_SIZE);
    assert_eq!(page.next, Some(40));
    assert_eq!(tracks[0].name, "Unicode เพลง");
    let captured_bytes = std::fs::read(args).unwrap();
    let captured: Vec<String> = serde_json::from_slice(
        captured_bytes
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(&captured_bytes),
    )
    .unwrap();
    assert_eq!(captured.last().unwrap(), &format!("ytsearch41:{query}"));
    let start = captured
        .iter()
        .position(|arg| arg == "--playlist-start")
        .unwrap();
    assert_eq!(captured[start + 1], "21");
    assert!(captured.iter().any(|arg| arg == "--ignore-config"));
}

#[tokio::test]
async fn malformed_and_failed_tool_responses_are_actionable_and_redacted() {
    let dir = tempfile::tempdir().unwrap();
    let tools = fixture(dir.path(), "[Console]::WriteLine('not json')");
    let error = tools.json(&[]).await.unwrap_err().to_string();
    assert!(error.contains("invalid metadata"));
    let tools = fixture(
        dir.path(),
        "[Console]::Error.WriteLine('ERROR: Private video https://signed.test/?token=secret'); exit 1",
    );
    let error = tools.json(&[]).await.unwrap_err().to_string();
    assert!(error.contains("private"));
    assert!(!error.contains("secret"));
    assert!(!error.contains("signed.test"));
}

#[tokio::test]
async fn cancellation_kills_the_owned_tool_process_and_timeout_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("pid.txt");
    let tools = fixture(
        dir.path(),
        &format!(
            "[IO.File]::WriteAllText('{}', [string]$PID)\nStart-Sleep -Seconds 60",
            marker.display().to_string().replace('\'', "''")
        ),
    );
    let worker_tools = tools.clone();
    let task = tokio::spawn(async move { worker_tools.json(&[]).await });
    tokio::time::timeout(Duration::from_secs(15), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let pid: u32 = std::fs::read_to_string(&marker).unwrap().parse().unwrap();
    task.abort();
    let _ = task.await;
    use windows::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            let status = WaitForSingleObject(handle, 5000);
            CloseHandle(handle).unwrap();
            assert_eq!(status, WAIT_OBJECT_0, "cancelled tool remained alive");
        }
    }
    let error = tools
        .json_with_timeout(&[], Duration::from_millis(300))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("timed out"));
}

#[tokio::test]
#[ignore = "Live YouTube, optional tools and Windows audio output; muted, no saved-state writes"]
async fn live_transport_pause_seek_skip_and_completion() {
    use crate::playback::{Command, Event};
    use tokio::sync::mpsc;
    async fn until(
        events: &mut mpsc::UnboundedReceiver<Event>,
        accept: impl Fn(&Event) -> bool,
    ) -> Event {
        tokio::time::timeout(Duration::from_secs(100), async {
            loop {
                let event = events.recv().await.expect("audio worker exited");
                if let Event::Error(message) | Event::TrackError { message, .. } = &event {
                    panic!("{message}");
                }
                if accept(&event) {
                    return event;
                }
            }
        })
        .await
        .expect("audio event timeout")
    }
    let visualizer = crate::visualizer::AudioVisualizer::new();
    let mut playback =
        crate::playback::Playback::spawn_youtube(Tools::discover().unwrap(), 0, visualizer.clone());
    let id = "youtube:dQw4w9WgXcQ".to_owned();
    playback
        .commands
        .send(Command::Load {
            id: id.clone(),
            position_ms: 0,
            generation: 10,
        })
        .unwrap();
    playback.commands.send(Command::Pause).unwrap();
    until(&mut playback.events, |event| {
        matches!(event, Event::Paused { generation: 10, .. })
    })
    .await;
    playback.commands.send(Command::Seek(1000)).unwrap();
    until(&mut playback.events, |event| {
        matches!(event, Event::Metadata { generation: 10, .. })
    })
    .await;
    tokio::time::sleep(Duration::from_millis(350)).await;
    while let Ok(event) = playback.events.try_recv() {
        assert!(
            !matches!(event, Event::Playing { .. }),
            "paused load resumed itself"
        );
    }
    playback.commands.send(Command::Resume).unwrap();
    let event = until(&mut playback.events, |event| {
        matches!(event, Event::Playing { generation: 10, .. })
    })
    .await;
    assert!(matches!(event, Event::Playing { position_ms, .. } if position_ms >= 1000));
    until(&mut playback.events, |event| matches!(event, Event::Position { generation: 10, position_ms } if *position_ms >= 1600)).await;
    assert!(visualizer.has_audio_samples());
    playback.commands.send(Command::Pause).unwrap();
    until(&mut playback.events, |event| {
        matches!(event, Event::Paused { generation: 10, .. })
    })
    .await;
    playback.commands.send(Command::Seek(5000)).unwrap();
    until(&mut playback.events, |event| {
        matches!(event, Event::Metadata { generation: 10, .. })
    })
    .await;
    tokio::time::sleep(Duration::from_millis(350)).await;
    while let Ok(event) = playback.events.try_recv() {
        assert!(
            !matches!(event, Event::Playing { .. }),
            "paused seek resumed itself"
        );
    }
    playback.commands.send(Command::Resume).unwrap();
    let event = until(&mut playback.events, |event| {
        matches!(event, Event::Playing { generation: 10, .. })
    })
    .await;
    assert!(matches!(event, Event::Playing { position_ms, .. } if position_ms >= 5000));
    playback
        .commands
        .send(Command::Load {
            id: id.clone(),
            position_ms: 0,
            generation: 11,
        })
        .unwrap();
    playback
        .commands
        .send(Command::Load {
            id,
            position_ms: 0,
            generation: 12,
        })
        .unwrap();
    let event = until(&mut playback.events, |event| {
        matches!(event, Event::Metadata { generation: 12, .. })
    })
    .await;
    let Event::Metadata { track, .. } = event else {
        unreachable!()
    };
    until(&mut playback.events, |event| {
        matches!(event, Event::Playing { generation: 12, .. })
    })
    .await;
    playback
        .commands
        .send(Command::Seek(track.duration_ms - 1500))
        .unwrap();
    until(&mut playback.events, |event| {
        matches!(event, Event::Completed(12))
    })
    .await;
    playback.commands.send(Command::Stop).unwrap();
    println!(
        "Verified muted real playback: pause before resolution, paused seek, resume, seek with cached stream, rapid skip, generation filtering, visualizer and natural completion."
    );
}

#[tokio::test]
#[ignore = "Live YouTube and Windows audio output; measures muted preloaded skipping without saved-state writes"]
async fn live_preloaded_skip_latency() {
    use crate::playback::{Command, Event};
    async fn playing(events: &mut tokio::sync::mpsc::UnboundedReceiver<Event>, generation: u64) {
        tokio::time::timeout(Duration::from_secs(90), async {
            while let Some(event) = events.recv().await {
                match event {
                    Event::Playing {
                        generation: actual, ..
                    } if actual == generation => return,
                    Event::Error(message) | Event::TrackError { message, .. } => {
                        panic!("{message}")
                    }
                    _ => (),
                }
            }
            panic!("audio worker closed");
        })
        .await
        .unwrap();
    }
    let mut playback = crate::playback::Playback::spawn_youtube(
        Tools::discover().unwrap(),
        0,
        crate::visualizer::AudioVisualizer::new(),
    );
    let begin = std::time::Instant::now();
    playback
        .commands
        .send(Command::Load {
            id: "youtube:dQw4w9WgXcQ".into(),
            position_ms: 0,
            generation: 1,
        })
        .unwrap();
    playing(&mut playback.events, 1).await;
    println!(
        "Cold playback to first audio: {:.3}s",
        begin.elapsed().as_secs_f64()
    );
    playback
        .commands
        .send(Command::Preload {
            id: "youtube:aqz-KE-bpKQ".into(),
        })
        .unwrap();
    // Allow preparation as a listener would during the current song.
    tokio::time::sleep(Duration::from_secs(8)).await;
    let begin = std::time::Instant::now();
    playback
        .commands
        .send(Command::Load {
            id: "youtube:aqz-KE-bpKQ".into(),
            position_ms: 0,
            generation: 2,
        })
        .unwrap();
    playing(&mut playback.events, 2).await;
    println!(
        "Preloaded next track to first audio: {:.3}s",
        begin.elapsed().as_secs_f64()
    );
    assert!(
        begin.elapsed() < Duration::from_secs(3),
        "preloaded skip was unexpectedly slow"
    );
    playback.commands.send(Command::Stop).unwrap();
}

#[test]
fn video_links_are_namespaced_and_hosts_are_checked() {
    for input in [
        "dQw4w9WgXcQ",
        "youtube:dQw4w9WgXcQ",
        "https://youtu.be/dQw4w9WgXcQ?si=ignored",
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=ignored",
        "music.youtube.com/watch?v=dQw4w9WgXcQ",
        "https://m.youtube.com/shorts/dQw4w9WgXcQ",
        "https://youtube.com/embed/dQw4w9WgXcQ",
    ] {
        assert_eq!(
            video_id(input).as_deref(),
            Some("youtube:dQw4w9WgXcQ"),
            "{input}"
        );
    }
    for input in [
        "https://youtube.com.evil.test/watch?v=dQw4w9WgXcQ",
        "https://evil.test/youtube.com/watch?v=dQw4w9WgXcQ",
        "https://youtube.com@evil.test/watch?v=dQw4w9WgXcQ",
        "file:///dQw4w9WgXcQ",
        "https://youtu.be/dQw4w9WgXcQ/more",
        "https://youtube.com/watch?v=bad",
        "https://youtube.com/playlist?list=dQw4w9WgXcQ",
        "youtube:dQw4w9WgXcQ!",
        "https://youtube.com:123/watch?v=dQw4w9WgXcQ",
    ] {
        assert!(video_id(input).is_none(), "{input}");
    }
    assert!(crate::model::valid_track_id("youtube:dQw4w9WgXcQ"));
    assert!(!crate::model::valid_id("youtube:dQw4w9WgXcQ"));
    assert!(!crate::model::valid_track_id("dQw4w9WgXcQ"));
}

#[test]
fn metadata_uses_music_fields_and_rejects_paid_or_live_content() {
    let mut value = serde_json::json!({"id":"dQw4w9WgXcQ", "title":"A music video", "track":"Song", "artist":"Artist - Topic", "duration":213.25});
    let track = parse_track(&value).unwrap();
    assert_eq!(track.name, "Song");
    assert_eq!(track.artists, "Artist");
    assert_eq!(track.duration_ms, 213250);
    assert!(track.playable);
    value["is_live"] = true.into();
    assert!(!parse_track(&value).unwrap().playable);
    value["is_live"] = false.into();
    value["availability"] = "premium_only".into();
    assert!(!parse_track(&value).unwrap().playable);
    value["id"] = "bad".into();
    assert!(parse_track(&value).is_none());
}

#[test]
fn signed_streams_and_upstream_errors_are_not_exposed() {
    assert!(
        validate_stream_url("https://rr1---sn-test.googlevideo.com/videoplayback?token=secret")
            .is_ok()
    );
    for input in [
        "http://rr1.googlevideo.com/audio",
        "https://googlevideo.com.evil.test/audio",
        "https://127.0.0.1/audio",
        "file:///music",
        "https://user:pass@rr1.googlevideo.com/audio",
    ] {
        assert!(validate_stream_url(input).is_err());
    }
    let error = tool_error(
        b"ERROR: Sign in to confirm you're not a bot https://private.test/?token=secret",
    );
    assert!(error.contains("blocked"));
    assert!(!error.contains("secret"));
    assert!(!error.contains("private.test"));
}

#[test]
fn youtube_queue_and_stats_roundtrip_without_changing_spotify() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::storage::Storage {
        root: dir.path().to_owned(),
    };
    let mut spotify_queue = crate::queue::Queue::default();
    spotify_queue.replace(vec!["0".repeat(22)], 0, false);
    store.save_queue(&spotify_queue).unwrap();
    let youtube = store.youtube().unwrap();
    let mut queue = crate::queue::Queue::default();
    queue.replace(vec!["youtube:dQw4w9WgXcQ".into()], 0, false);
    youtube.save_queue(&queue).unwrap();
    assert_eq!(
        youtube.queue().unwrap().current(),
        Some("youtube:dQw4w9WgXcQ")
    );
    assert_eq!(
        store.queue().unwrap().current(),
        Some("0".repeat(22).as_str())
    );
    let mut stats = crate::stats::SongStats::default();
    stats.add_play("youtube:dQw4w9WgXcQ", "Song", "Artist");
    youtube.save_stats(&stats).unwrap();
    assert_eq!(
        youtube.stats().unwrap().tracks["youtube:dQw4w9WgXcQ"].play_count,
        1
    );
}
