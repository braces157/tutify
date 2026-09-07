use super::*;

#[tokio::test]
async fn keyboard_controls_match_in_normal_and_stats_modes() {
    for loaded in [false, true] {
        let mut normal = App::new(Config::default(), Queue::default());
        let mut overlay = App::new(Config::default(), Queue::default());
        for app in [&mut normal, &mut overlay] {
            let track = test_track(1);
            app.queue.replace(vec![track.id.clone()], 0, false);
            app.cache.insert(track.id.clone(), track);
            app.loaded = loaded;
        }
        overlay.ui.overlay = Overlay::Stats;
        let (mut normal_tasks, _) = tasks();
        let (mut overlay_tasks, _) = tasks();
        let (normal_tx, mut normal_rx) = mpsc::unbounded_channel();
        let (overlay_tx, mut overlay_rx) = mpsc::unbounded_channel();
        for code in [
            KeyCode::Home,
            KeyCode::Right,
            KeyCode::End,
            KeyCode::Right,
            KeyCode::Left,
            KeyCode::Char('+'),
            KeyCode::Char(']'),
            KeyCode::Char('m'),
            KeyCode::Char('m'),
            KeyCode::Char('-'),
            KeyCode::Char('['),
        ] {
            let event = KeyEvent::new(code, KeyModifiers::NONE);
            key(&mut normal, event, &mut normal_tasks, &normal_tx);
            key(&mut overlay, event, &mut overlay_tasks, &overlay_tx);
            assert_eq!(normal.queue.position_ms, overlay.queue.position_ms);
            assert_eq!(normal.config.volume, overlay.config.volume);
            assert_eq!(normal.muted_volume, overlay.muted_volume);
            assert_eq!(normal.status, overlay.status);
            assert_eq!(
                format!("{:?}", normal_rx.try_recv()),
                format!("{:?}", overlay_rx.try_recv()),
                "different playback command for {code:?}, loaded={loaded}"
            );
        }
        assert_eq!(normal.queue.position_ms, 189_999);
        assert_eq!(normal.config.volume, 50);
    }
}

#[tokio::test]
async fn menu_executes_track_intent_without_keyboard_routing() {
    for (overlay, expected_count) in [(Overlay::None, 1), (Overlay::Stats, 0)] {
        let mut app = App::new(Config::default(), Queue::default());
        let (mut tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();
        app.catalog.rows = Rows::Tracks(vec![test_track(1)]);
        app.catalog.editing = true;
        app.ui.overlay = overlay;
        app.context_menu = Some(ContextMenu {
            x: 0,
            y: 0,
            selected: 0,
            actions: vec![("Add to queue", Action::EnqueueSelected)],
            view: app.catalog.view,
            row: 0,
            revision: app.catalog.rows_revision,
            filter: String::new(),
        });
        activate_menu(&mut app, 0, &mut tasks, &tx);
        assert_eq!(app.queue.ids.len(), expected_count);
        if expected_count > 0 {
            assert_eq!(app.queue.ids, vec![test_track(1).id]);
        }
        assert!(app.catalog.query.is_empty());
        assert!(app.context_menu.is_none());
    }
}

#[tokio::test]
async fn stats_navigation_preserves_catalog_cursor_after_real_frames() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    app.catalog.view = View::Liked;
    app.catalog.rows = Rows::Tracks((0..40).map(test_track).collect());
    app.catalog.selected = 25;
    for i in 0..4 {
        let track = test_track(i);
        app.stats.add_play(&track.id, &track.name, &track.artists);
    }
    draw_mouse(&app, 80, 24);
    let scroll = app.ui.render.borrow().catalog_scroll;
    for code in [KeyCode::Char('S'), KeyCode::Down, KeyCode::Down] {
        key(
            &mut app,
            KeyEvent::new(code, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        draw_mouse(&app, 80, 24);
    }
    assert_eq!(app.ui.stats.borrow().selected, 2);
    assert_eq!(app.catalog.selected, 25);
    key(
        &mut app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    draw_mouse(&app, 80, 24);
    assert_eq!(app.ui.overlay, Overlay::None);
    assert_eq!(app.catalog.selected, 25);
    assert_eq!(app.ui.render.borrow().catalog_scroll, scroll);
}
