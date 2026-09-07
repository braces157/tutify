use super::*;

#[test]
fn discord_does_not_publish_restored_unknown_or_stopped_tracks() {
    let mut queue = Queue::default();
    queue.replace(vec![test_track(1).id], 0, false);
    let mut app = App::new(Config::default(), queue);
    assert!(app.discord_snapshot().track.is_none());
    let (tx, _rx) = mpsc::unbounded_channel();
    app.media_action(MediaAction::Play, &tx);
    assert!(app.discord_snapshot().track.is_none());
    app.cache.insert(test_track(1).id, test_track(1));
    assert!(crate::discord::build_activity(&app.discord_snapshot(), 1000, None).is_none());
    app.playback_event(
        Event::Playing {
            generation: app.generation,
            position_ms: 0,
        },
        &tx,
    );
    assert!(crate::discord::build_activity(&app.discord_snapshot(), 1000, None).is_some());
    app.media_action(MediaAction::Pause, &tx);
    assert!(crate::discord::build_activity(&app.discord_snapshot(), 1000, None).is_some());
    app.stop(&tx);
    assert!(app.discord_snapshot().track.is_none());
}
#[test]
fn media_actions_work_during_text_entry_and_are_idempotent() {
    let mut queue = Queue::default();
    queue.replace(vec![test_track(1).id, test_track(2).id], 0, false);
    let mut app = App::new(Config::default(), queue);
    app.catalog.editing = true;
    app.catalog.filtering = true;
    app.catalog.query = "my search".into();
    app.catalog.filter = "my filter".into();
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.media_action(MediaAction::Pause, &tx);
    assert!(rx.try_recv().is_err());
    app.media_action(MediaAction::Play, &tx);
    assert!(matches!(
        rx.try_recv().unwrap(),
        Command::Load { position_ms: 0, .. }
    ));
    app.media_action(MediaAction::Play, &tx);
    assert!(rx.try_recv().is_err());
    app.media_action(MediaAction::Pause, &tx);
    assert!(matches!(rx.try_recv().unwrap(), Command::Pause));
    app.media_action(MediaAction::Pause, &tx);
    assert!(rx.try_recv().is_err());
    app.media_action(MediaAction::Play, &tx);
    assert!(matches!(rx.try_recv().unwrap(), Command::Resume));
    assert_eq!(app.catalog.query, "my search");
    assert_eq!(app.catalog.filter, "my filter");
    assert!(app.catalog.editing && app.catalog.filtering);
}

#[test]
fn media_navigation_preserves_manual_repeat_and_previous_behavior() {
    let mut queue = Queue::default();
    queue.replace(vec![test_track(1).id, test_track(2).id], 0, false);
    let mut app = App::new(Config::default(), queue);
    app.config.repeat = crate::model::Repeat::Track;
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.media_action(MediaAction::Next, &tx);
    assert_eq!(app.queue.current(), Some(test_track(2).id.as_str()));
    assert!(matches!(rx.try_recv().unwrap(), Command::Load { .. }));
    app.queue.position_ms = 5000;
    app.media_action(MediaAction::Previous, &tx);
    assert_eq!(app.queue.current(), Some(test_track(2).id.as_str()));
    assert!(matches!(
        rx.try_recv().unwrap(),
        Command::Load { position_ms: 0, .. }
    ));
    app.media_action(MediaAction::Previous, &tx);
    assert_eq!(app.queue.current(), Some(test_track(1).id.as_str()));
    assert!(matches!(rx.try_recv().unwrap(), Command::Load { .. }));
    app.media_action(MediaAction::Next, &tx);
    rx.try_recv().unwrap();
    app.media_action(MediaAction::Next, &tx);
    assert!(matches!(rx.try_recv().unwrap(), Command::Stop));
    assert_eq!(app.state, State::Paused);
}
#[test]
fn filter_cache_reuses_indices_until_rows_or_query_change() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Liked;
    app.catalog.rows = Rows::Tracks(vec![test_track(1)]);
    app.catalog.filter = "artist".into();
    let first = app.filtered_indices();
    assert!(Arc::ptr_eq(&first, &app.filtered_indices()));
    app.catalog.filter = "missing".into();
    assert!(app.filtered_indices().is_empty());
    app.catalog.rows = Rows::Tracks(vec![Track {
        name: "Missing".into(),
        ..test_track(2)
    }]);
    app.catalog.rows_revision += 1;
    assert_eq!(*app.filtered_indices(), vec![0]);
}
#[test]
fn animation_is_suspended_when_paused_or_too_small_and_slower_without_bars() {
    let mut app = App::new(Config::default(), Queue::default());
    assert_eq!(app.animation_interval(), None);
    app.state = State::Loading;
    assert_eq!(app.animation_interval(), None);
    app.state = State::Playing;
    assert_eq!(app.animation_interval(), Some(Duration::from_millis(33)));
    app.ui.render.borrow_mut().terminal_size = (40, 20);
    assert_eq!(app.animation_interval(), Some(Duration::from_millis(250)));
    app.ui.overlay = Overlay::Visualizer;
    assert_eq!(app.animation_interval(), Some(Duration::from_millis(33)));
    app.ui.render.borrow_mut().terminal_size = (20, 6);
    assert_eq!(app.animation_interval(), None);
}
#[test]
fn playback_position_uses_elapsed_time_and_paused_position_is_stable() {
    let mut app = App::new(Config::default(), Queue::default());
    app.position_anchor = Some((Instant::now() - Duration::from_millis(1200), 500));
    app.state = State::Playing;
    app.interpolate_position();
    assert!(app.queue.position_ms >= 1700);
    app.state = State::Paused;
    let paused = app.queue.position_ms;
    app.interpolate_position();
    assert_eq!(paused, app.queue.position_ms);
}
#[test]
fn restored_queue_starts_paused_and_stale_completion_is_ignored() {
    let mut q = Queue::default();
    q.replace(vec!["0".repeat(22), "1".repeat(22)], 0, false);
    q.position_ms = 12345;
    let mut app = App::new(Config::default(), q);
    let (tx, mut rx) = mpsc::unbounded_channel();
    assert_eq!(app.state, State::Paused);
    assert!(!app.loaded);
    assert_eq!(app.queue.position_ms, 12345);
    app.generation = 4;
    app.playback_event(
        Event::TrackError {
            generation: 3,
            message: "stale failure".into(),
        },
        &tx,
    );
    assert_eq!(app.state, State::Paused);
    app.playback_event(Event::Completed(3), &tx);
    assert_eq!(app.queue.cursor, Some(0));
    assert!(rx.try_recv().is_err());
    app.playback_event(Event::Completed(4), &tx);
    assert_eq!(app.queue.cursor, Some(1));
    assert!(matches!(rx.try_recv().unwrap(), Command::Load { .. }));
}
#[test]
fn unavailable_stops_without_skipping_loop() {
    let mut q = Queue::default();
    q.replace(vec!["0".repeat(22), "1".repeat(22)], 0, false);
    let mut app = App::new(Config::default(), q);
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.playback_event(Event::Error("unavailable".into()), &tx);
    assert_eq!(app.queue.cursor, Some(0));
    assert_eq!(app.state, State::Failed);
    assert!(matches!(rx.try_recv().unwrap(), Command::Stop));
    assert!(rx.try_recv().is_err());
}
#[test]
fn filter_tracks_and_playlists() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Liked;
    app.catalog.rows = Rows::Tracks(vec![
        Track {
            id: "1".into(),
            name: "Bohemian Rhapsody".into(),
            artists: "Queen".into(),
            duration_ms: 354000,
            playable: true,
            ..Default::default()
        },
        Track {
            id: "2".into(),
            name: "Yellow".into(),
            artists: "Coldplay".into(),
            duration_ms: 269000,
            playable: true,
            ..Default::default()
        },
        Track {
            id: "3".into(),
            name: "Under Pressure".into(),
            artists: "Queen, David Bowie".into(),
            duration_ms: 248000,
            playable: true,
            ..Default::default()
        },
    ]);

    assert_eq!(app.len(), 3);
    assert!(!app.is_filtered());

    // Filter by artist
    app.catalog.filter = "queen".into();
    assert!(app.is_filtered());
    assert_eq!(app.len(), 2);
    assert_eq!(*app.filtered_indices(), vec![0, 2]);

    // Filter by title
    app.catalog.filter = "yellow".into();
    assert_eq!(app.len(), 1);
    assert_eq!(*app.filtered_indices(), vec![1]);

    // Filter by multiple terms across title and artist
    app.catalog.filter = "bowie pressure".into();
    assert_eq!(app.len(), 1);
    assert_eq!(*app.filtered_indices(), vec![2]);

    // Non-matching filter
    app.catalog.filter = "nonexistent".into();
    assert_eq!(app.len(), 0);
    assert_eq!(*app.filtered_indices(), Vec::<usize>::new());

    // Empty filter
    app.catalog.filter.clear();
    assert!(!app.is_filtered());
    assert_eq!(app.len(), 3);

    // Test Playlists filtering
    app.catalog.view = View::Playlists;
    app.catalog.rows = Rows::Playlists(vec![
        crate::model::Playlist {
            id: "p1".into(),
            name: "Rock Classics".into(),
            owner: "Spotify".into(),
        },
        crate::model::Playlist {
            id: "p2".into(),
            name: "Chill Lofi Beats".into(),
            owner: "ChilledCow".into(),
        },
    ]);

    app.catalog.filter = "chilled".into();
    assert!(app.is_filtered());
    assert_eq!(app.len(), 1);
    assert_eq!(*app.filtered_indices(), vec![1]);

    app.catalog.filter = "classics".into();
    assert_eq!(app.len(), 1);
    assert_eq!(*app.filtered_indices(), vec![0]);
}
#[test]
fn window_title_formats() {
    let mut app = App::new(Config::default(), Queue::default());
    assert_eq!(app.window_title(), "Tuitify");

    let track = Track {
        id: "t1".into(),
        name: "牵丝戏".into(),
        artists: "银临, Aki阿杰".into(),
        duration_ms: 239000,
        playable: true,
        ..Default::default()
    };
    app.cache.insert("t1".into(), track);
    app.queue.replace(vec!["t1".into()], 0, false);

    app.state = State::Playing;
    assert_eq!(app.window_title(), "Tuitify • 牵丝戏 - 银临, Aki阿杰");

    app.state = State::Paused;
    assert_eq!(app.window_title(), "|| Tuitify • 牵丝戏 - 银临, Aki阿杰");

    app.state = State::Loading;
    assert_eq!(app.window_title(), "... Tuitify • 牵丝戏 - 银临, Aki阿杰");

    app.state = State::Failed;
    assert_eq!(app.window_title(), "! Tuitify • 牵丝戏 - 银临, Aki阿杰");
}

#[test]
fn regression_continuous_accounting_after_queue_mutation() {
    let mut app = App::new(Config::default(), Queue::default());
    let id = "0".repeat(22);
    app.queue.replace(vec![id.clone()], 0, false);
    let start = Instant::now();
    app.generation = 1;
    app.state = State::Playing;
    app.accounting
        .start_generation(1, Some(id.clone()), 200_000);
    app.accounting
        .on_playing(1, Some(id.clone()), 200_000, start);

    // Account 5 seconds
    app.account_playback_time(start + Duration::from_secs(5));
    assert_eq!(app.stats.tracks.get(&id).unwrap().listened_ms, 5_000);

    // Queue mutation calls remember_queue
    app.remember_queue();
    // Accounting continuity is preserved: last_accounted_at must not be None
    assert!(app.accounting.last_accounted_at.is_some());

    // Play for another 5 seconds
    app.account_playback_time(start + Duration::from_secs(10));
    assert_eq!(app.stats.tracks.get(&id).unwrap().listened_ms, 10_000);
}

#[tokio::test]
async fn regression_switching_queued_tracks_finalizes_generation_pinned_old_track() {
    let mut app = App::new(Config::default(), Queue::default());
    let id1 = "1".repeat(22);
    let id2 = "2".repeat(22);
    app.queue.replace(vec![id1.clone(), id2.clone()], 0, false);
    let start = Instant::now();
    app.generation = 1;
    app.state = State::Playing;
    app.accounting
        .start_generation(1, Some(id1.clone()), 200_000);
    app.accounting
        .on_playing(1, Some(id1.clone()), 200_000, start);

    // Advance 10 seconds of playback time in 5s chunks
    for i in 1..=2 {
        app.account_playback_time(start + Duration::from_secs(i * 5));
    }
    assert_eq!(app.stats.tracks.get(&id1).unwrap().listened_ms, 10_000);

    // User selects track 2 in queue
    app.queue.select(1);
    assert_eq!(app.queue.current(), Some(id2.as_str()));

    // Call load for new track: it should finalize track 1, not track 2!
    let (tx, _rx) = mpsc::unbounded_channel();
    app.load(&tx);

    assert_eq!(app.stats.tracks.get(&id1).unwrap().listened_ms, 10_000);
    assert!(!app.stats.tracks.contains_key(&id2));
    assert_eq!(app.accounting.track_id, Some(id2.clone()));
}
