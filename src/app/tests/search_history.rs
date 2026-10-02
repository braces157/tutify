use super::*;

#[test]
fn search_recall_keeps_unique_recent_queries_bounded_and_restores_drafts() {
    let mut history = super::super::search_history::SearchHistory::default();
    for i in 0..25 {
        history.record(&format!("Search {i}"));
    }
    history.record("  Search 22  ");
    assert_eq!(
        history.recall("my draft", true).as_deref(),
        Some("Search 22")
    );
    assert_eq!(
        history.recall("Search 22", true).as_deref(),
        Some("Search 24")
    );
    assert_eq!(
        history.recall("Search 24", false).as_deref(),
        Some("Search 22")
    );
    assert_eq!(
        history.recall("Search 22", false).as_deref(),
        Some("my draft")
    );
    for _ in 0..30 {
        history.recall("", true);
    }
    assert_eq!(history.recall("", true).as_deref(), Some("Search 5"));
    history.detach();
    assert_eq!(
        history.recall("new draft", true).as_deref(),
        Some("Search 22")
    );
    assert_eq!(history.recall("", false).as_deref(), Some("new draft"));
}

#[tokio::test]
async fn search_recall_arrows_do_not_run_network_search_and_restore_typed_draft() {
    let mut app = App::new(Config::default(), Queue::default());
    app.ui.search_history.record("日本語 songs");
    app.ui.search_history.record("new query");
    app.catalog.editing = true;
    app.catalog.query = "unsent draft".into();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.query, "new query");
    key(
        &mut app,
        KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.query, "日本語 songs");
    key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.query, "unsent draft");
    assert!(tasks.browse.is_none());
    assert!(rx.try_recv().is_err());
    assert!(app.catalog.editing);
}

#[tokio::test]
async fn queue_filter_recovery_does_not_change_playback_or_repeat() {
    let mut app = crate::demo::app();
    app.ui.queue.query = "midnight".into();
    app.clear_queue_filter();
    app.clear_queue_filter();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let repeat = app.config.repeat;
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.queue.query, "midnight");
    assert_eq!(app.len(), 1);
    assert_eq!(app.config.repeat, repeat);
    assert!(rx.try_recv().is_err());
}
