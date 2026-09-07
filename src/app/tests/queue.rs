use super::*;

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
