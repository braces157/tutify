use super::*;

#[tokio::test]
async fn stats_overlay_toggle_mutual_exclusion_and_esc() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    assert!(app.ui.overlay != Overlay::Stats);
    assert!(app.ui.overlay != Overlay::Lyrics);
    assert!(app.ui.overlay != Overlay::Visualizer);

    // Start with lyrics open
    app.ui.overlay = Overlay::Lyrics;

    // Press 'S' to open stats: closes lyrics
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.overlay == Overlay::Stats);
    assert!(app.ui.overlay != Overlay::Lyrics);
    assert!(app.ui.overlay != Overlay::Visualizer);
    assert!(app.status.contains("Song statistics"));

    // While stats is open, 'l' and 'v' are ignored (overlay is isolated)
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.overlay == Overlay::Stats);
    assert!(app.ui.overlay != Overlay::Lyrics);

    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.overlay == Overlay::Stats);
    assert!(app.ui.overlay != Overlay::Visualizer);

    // Press 'S' to dismiss
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.overlay != Overlay::Stats);
    assert!(app.status.contains("Exited"));

    // Start with visualizer open
    app.ui.overlay = Overlay::Visualizer;

    // Press 'S': closes visualizer, opens stats
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.overlay == Overlay::Stats);
    assert!(app.ui.overlay != Overlay::Visualizer);

    // Dismiss via Esc
    key(
        &mut app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.overlay != Overlay::Stats);
    assert!(!app.quit);
    assert!(app.status.contains("Exited"));
}

#[tokio::test]
async fn stats_overlay_navigation_and_enter_safety() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    // Populate 3 tracks in stats
    let id1 = "1".repeat(22);
    let id2 = "2".repeat(22);
    let id3 = "3".repeat(22);
    app.stats.add_play(&id1, "Track 1", "Artist 1");
    app.stats.add_play(&id2, "Track 2", "Artist 2");
    app.stats.add_play(&id3, "Track 3", "Artist 3");

    // Open stats overlay
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.overlay == Overlay::Stats);
    assert_eq!(app.ui.stats.borrow().selected, 0);

    // Down arrow navigates within stats bounds
    key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.stats.borrow().selected, 1);

    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.stats.borrow().selected, 2);

    // Down at end clamps to len - 1 (index 2)
    key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.stats.borrow().selected, 2);

    // Enter is suppressed while stats overlay is open
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.overlay == Overlay::Stats);
    assert!(app.queue.ids.is_empty());
}

#[tokio::test]
async fn stats_search_isolates_shortcuts_and_bounds_navigation() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    app.stats.add_play(&"1".repeat(22), "quiet", "One");
    app.stats.add_play(&"2".repeat(22), "Other", "Two");
    app.ui.overlay = Overlay::Stats;
    for code in [
        KeyCode::Char('/'),
        KeyCode::Char('q'),
        KeyCode::Enter,
        KeyCode::Down,
    ] {
        key(
            &mut app,
            KeyEvent::new(code, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
    }
    assert!(!app.quit);
    assert_eq!(app.ui.stats.borrow().selected, 0);
    assert_eq!(app.len(), 1);
    key(
        &mut app,
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(
        app.ui.stats.borrow().sort,
        crate::app::ui_state::StatsSort::Time
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.overlay == Overlay::Stats);
    assert_eq!(app.len(), 2);
    key(
        &mut app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.overlay != Overlay::Stats);
}

#[tokio::test]
async fn regression_stats_overlay_from_view_queue_navigates_selected_without_changing_queue_selected()
 {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    for i in 0..5 {
        app.queue.enqueue(test_track(i).id);
    }
    app.catalog.view = View::Queue;
    app.queue.selected = 3;

    let id1 = "1".repeat(22);
    let id2 = "2".repeat(22);
    app.stats.add_play(&id1, "Track 1", "Artist 1");
    app.stats.add_play(&id2, "Track 2", "Artist 2");

    // Open stats overlay
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.overlay == Overlay::Stats);
    assert_eq!(app.ui.stats.borrow().selected, 0);
    assert_eq!(app.queue.selected, 3);

    // Press Down / j
    key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.stats.borrow().selected, 1);
    assert_eq!(app.queue.selected, 3); // queue.selected unchanged!

    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.stats.borrow().selected, 0);
    assert_eq!(app.queue.selected, 3); // queue.selected unchanged!
}

#[tokio::test]
async fn regression_destructive_and_settings_keys_ignored_in_stats_overlay() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    for i in 0..5 {
        app.queue.enqueue(test_track(i).id);
    }
    app.catalog.view = View::Queue;
    let initial_queue_len = app.queue.ids.len();
    let initial_shuffle = app.config.shuffle;
    let initial_repeat = app.config.repeat;
    let initial_theme = app.config.theme.clone();

    app.ui.overlay = Overlay::Stats;

    // Press 's' (toggle shuffle) - must be ignored!
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.config.shuffle, initial_shuffle);

    // Press 'd' / 'x' / Delete (remove from queue) - must be ignored!
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.queue.ids.len(), initial_queue_len);

    // Press 'C' (clear queue) - must be ignored!
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.queue.ids.len(), initial_queue_len);

    // Press 'r' (cycle repeat) - must be ignored!
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.config.repeat, initial_repeat);

    // Press 't' (cycle theme) - must be ignored!
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.config.theme, initial_theme);

    // Press 'a' / 'A' - must be ignored!
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.queue.ids.len(), initial_queue_len);

    // Press 'J' / 'K' - must be ignored!
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT),
        &mut tasks,
        &tx,
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT),
        &mut tasks,
        &tx,
    );

    // Press '1'..='5' - must not change view!
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Queue);
}

#[tokio::test]
async fn regression_catalog_scroll_preserved_across_stats_overlay() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    app.ui.render.borrow_mut().catalog_scroll = 17;
    assert_eq!(app.ui.render.borrow().catalog_scroll, 17);

    // Open stats overlay
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.overlay == Overlay::Stats);
    assert_eq!(app.ui.render.borrow().catalog_scroll, 17); // catalog_scroll preserved!
    assert_eq!(app.ui.render.borrow().stats_scroll, 0);

    // Navigate in stats
    key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.render.borrow().catalog_scroll, 17);

    // Close stats
    key(
        &mut app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.overlay != Overlay::Stats);
    assert_eq!(app.ui.render.borrow().catalog_scroll, 17); // catalog_scroll preserved!
}
