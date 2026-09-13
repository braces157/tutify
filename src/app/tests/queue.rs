use super::*;
use crate::model::Repeat;

#[tokio::test]
async fn shift_j_reorders_and_page_down_only_navigates() {
    let (mut tasks, _) = tasks();
    let mut q = Queue::default();
    q.replace((0..20).map(|i| format!("{i:022}")).collect(), 0, false);
    let mut app = App::new(Config::default(), q);
    let (tx, _) = mpsc::unbounded_channel();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT),
        &mut tasks,
        &tx,
    );
    assert_eq!(&app.queue.order[..3], &[1, 0, 2]);
    assert_eq!(app.queue.selected, 1);
    assert_eq!(app.queue.cursor, Some(1));
    key(
        &mut app,
        KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(&app.queue.order[..3], &[1, 0, 2]);
    assert_eq!(app.queue.selected, 16);
}
#[tokio::test]
async fn undo_restores_removed_current_and_cleared_queue_paused_with_fresh_epoch() {
    let mut queue = Queue::default();
    queue.replace((0..5).map(|i| test_track(i).id).collect(), 2, true);
    queue.position_ms = 42_000;
    let original = queue.clone();
    let mut app = App::new(
        Config {
            shuffle: true,
            ..Config::default()
        },
        queue,
    );
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.queue.selected = app.queue.cursor.unwrap();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(app.queue.current().is_none());
    let removed_epoch = app.queue.epoch;
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.queue.ids, original.ids);
    assert_eq!(app.queue.order, original.order);
    assert_eq!(app.queue.current(), original.current());
    assert_eq!(app.queue.position_ms, 42_000);
    assert_eq!(app.state, State::Paused);
    assert!(!app.loaded);
    assert!(app.queue.epoch > removed_epoch);
    app.queue.validate().unwrap();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT),
        &mut tasks,
        &tx,
    );
    assert!(app.queue.ids.is_empty());
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.queue.ids, original.ids);
    assert!(
        std::iter::from_fn(|| rx.try_recv().ok()).all(|command| matches!(command, Command::Stop))
    );
}

#[tokio::test]
async fn undo_queue_replacement_rejects_late_jobs_and_restores_previous_position() {
    let mut queue = Queue::default();
    queue.replace(vec![test_track(1).id], 0, false);
    queue.position_ms = 8000;
    let mut app = App::new(Config::default(), queue);
    let (mut tasks, _) = tasks();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    app.catalog.view = View::Search;
    app.catalog.rows = Rows::Tracks(vec![test_track(2)]);
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    let replaced_epoch = app.queue.epoch;
    assert_eq!(app.queue.current(), Some(test_track(2).id.as_str()));
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    background(
        &mut app,
        &mut tasks,
        Background::Recommendations(
            replaced_epoch,
            Ok(Recommendations {
                tracks: vec![test_track(3)],
                source: crate::catalog::RecommendationSource::Spotify,
            }),
        ),
    );
    assert_eq!(app.queue.ids, vec![test_track(1).id]);
    assert_eq!(app.queue.position_ms, 8000);
    assert_eq!(app.state, State::Paused);
}

#[test]
fn removing_current_radio_seed_rejects_late_recommendations() {
    let mut queue = Queue::default();
    queue.replace(vec![test_track(1).id], 0, false);
    let epoch = queue.epoch;
    let mut app = App::new(Config::default(), queue);
    app.catalog.view = View::Queue;
    app.radio_epoch = Some(epoch);
    let (mut tasks, _rx) = tasks();
    tasks.radio_active = true;
    let (tx, _commands) = mpsc::unbounded_channel();

    actions::apply(&mut app, Action::RemoveSelected, &mut tasks, &tx);
    assert!(app.queue.ids.is_empty());
    assert_eq!(app.radio_epoch, None);
    assert!(!tasks.radio_active);

    background(
        &mut app,
        &mut tasks,
        Background::Recommendations(
            epoch,
            Ok(Recommendations {
                tracks: vec![test_track(2)],
                source: crate::catalog::RecommendationSource::Spotify,
            }),
        ),
    );
    assert!(app.queue.ids.is_empty());
}

#[test]
fn removing_item_invalidates_inflight_smart_shuffle_request() {
    let mut queue = Queue::default();
    queue.replace((0..6).map(|i| test_track(i).id).collect(), 0, false);
    queue.smart_shuffle = true;
    let epoch = queue.epoch;
    let mut app = App::new(Config::default(), queue);
    app.catalog.view = View::Queue;
    app.queue.selected = 5;
    let (mut tasks, _rx) = tasks();
    tasks.smart.request = 9;
    let stale_request = tasks.smart.request;
    let (tx, _commands) = mpsc::unbounded_channel();

    actions::apply(&mut app, Action::RemoveSelected, &mut tasks, &tx);
    assert_ne!(tasks.smart.request, stale_request);

    let before = app.queue.ids.clone();
    background(
        &mut app,
        &mut tasks,
        Background::SmartRecommendations(
            epoch,
            stale_request,
            Ok(Recommendations {
                tracks: vec![test_track(20)],
                source: crate::catalog::RecommendationSource::Spotify,
            }),
        ),
    );
    assert_eq!(app.queue.ids, before);
}

#[test]
fn undo_history_is_bounded_by_action_count_and_total_tracks() {
    let mut app = App::new(Config::default(), Queue::default());
    for _ in 0..20 {
        app.remember_queue();
    }
    assert_eq!(app.undo.len(), 10);
    app.queue
        .replace((0..50_000).map(|i| test_track(i).id).collect(), 0, false);
    for _ in 0..4 {
        app.remember_queue();
    }
    assert_eq!(
        app.undo
            .iter()
            .map(|entry| entry.queue.ids.len())
            .sum::<usize>(),
        100_000
    );
    assert_eq!(app.undo.len(), 2);
}

#[test]
fn time_to_preload_triggers_next_track_preload_and_updates_on_play_next() {
    let mut q = Queue::default();
    let id0 = "0".repeat(22);
    let id1 = "1".repeat(22);
    let id_next = "9".repeat(22);
    q.replace(vec![id0.clone(), id1.clone()], 0, false);
    let mut app = App::new(Config::default(), q);
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Start track 0
    app.generation = 1;
    app.playback_event(
        Event::Playing {
            generation: 1,
            position_ms: 0,
        },
        &tx,
    );
    assert_eq!(app.preload_requested, None);

    // Time to preload arrives for generation 1
    app.playback_event(Event::TimeToPreload { generation: 1 }, &tx);
    assert_eq!(app.preload_requested, Some(1));
    assert_eq!(app.last_preloaded_id, Some(id1.clone()));

    // Verify Command::Preload was sent for track 1
    let cmd = rx.try_recv().expect("Command::Preload should be sent");
    match cmd {
        Command::Preload { id } => assert_eq!(id, id1),
        other => panic!("Unexpected command: {other:?}"),
    }

    // Now insert a new track to play next
    app.queue.insert_next(id_next.clone());
    app.check_preload(&tx);
    assert_eq!(app.last_preloaded_id, Some(id_next.clone()));

    // Verify updated Command::Preload was sent for track 9
    let cmd = rx
        .try_recv()
        .expect("Updated Command::Preload should be sent");
    match cmd {
        Command::Preload { id } => assert_eq!(id, id_next),
        other => panic!("Unexpected command: {other:?}"),
    }
}

#[test]
fn preload_respects_repeat_modes() {
    let mut q = Queue::default();
    let id0 = "0".repeat(22);
    let id1 = "1".repeat(22);
    q.replace(vec![id0.clone(), id1.clone()], 1, false); // cursor on last track (id1)
    let mut app = App::new(Config::default(), q);
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.generation = 1;

    // With Repeat::Off on last track, no next track
    app.config.repeat = Repeat::Off;
    app.playback_event(Event::TimeToPreload { generation: 1 }, &tx);
    assert_eq!(app.last_preloaded_id, None);
    assert!(rx.try_recv().is_err());

    // With Repeat::Queue on last track, loops back to id0
    app.config.repeat = Repeat::Queue;
    app.check_preload(&tx);
    assert_eq!(app.last_preloaded_id, Some(id0.clone()));
    match rx.try_recv().unwrap() {
        Command::Preload { id } => assert_eq!(id, id0),
        other => panic!("Unexpected command: {other:?}"),
    }

    // With Repeat::Track, preloads current track (id1)
    app.config.repeat = Repeat::Track;
    app.check_preload(&tx);
    assert_eq!(app.last_preloaded_id, Some(id1.clone()));
    match rx.try_recv().unwrap() {
        Command::Preload { id } => assert_eq!(id, id1),
        other => panic!("Unexpected command: {other:?}"),
    }
}

#[tokio::test]
async fn enqueue_triggers_preload_when_at_end_of_queue() {
    let mut q = Queue::default();
    let id0 = "0".repeat(22);
    let id1 = "1".repeat(22);
    q.replace(vec![id0], 0, false);
    let mut app = App::new(Config::default(), q);
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.generation = 1;
    app.config.repeat = Repeat::Off;

    // Time to preload arrives while playing id0 (the only/last track in queue)
    app.playback_event(Event::TimeToPreload { generation: 1 }, &tx);
    assert_eq!(app.last_preloaded_id, None);
    assert!(rx.try_recv().is_err());

    // User enqueues track 1
    let (bg_tx, _bg_rx) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(crate::catalog::Catalog::mock("http://127.0.0.1"), bg_tx).unwrap();
    app.catalog.rows = crate::catalog::Rows::Tracks(vec![crate::model::Track {
        id: id1.clone(),
        name: "Next Song".into(),
        artists: "Artist".into(),
        duration_ms: 180_000,
        playable: true,
        ..Default::default()
    }]);
    app.catalog.view = View::Search;
    app.catalog.selected = 0;
    crate::app::actions::apply(
        &mut app,
        crate::app::Action::EnqueueSelected,
        &mut tasks,
        &tx,
    );

    // Track 1 should now be preloaded!
    assert_eq!(app.last_preloaded_id, Some(id1.clone()));
    match rx
        .try_recv()
        .expect("Command::Preload should be sent on EnqueueSelected")
    {
        Command::Preload { id } => assert_eq!(id, id1),
        other => panic!("Unexpected command: {other:?}"),
    }
}
