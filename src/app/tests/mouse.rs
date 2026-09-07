use super::*;

#[tokio::test]
async fn right_click_filtered_row_opens_actions_without_pasting_and_enqueues_that_track() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Liked;
    let mut first = test_track(1);
    first.name = "Other".into();
    let mut second = test_track(2);
    second.name = "Wanted".into();
    app.catalog.rows = Rows::Tracks(vec![first, second.clone()]);
    app.catalog.filter = "Wanted".into();
    app.catalog.filtering = true;
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    assert!(!app.catalog.filtering);
    assert_eq!(app.catalog.filter, "Wanted");
    assert!(app.context_menu.is_some());
    assert!(app.queue.ids.is_empty());
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Menu(1),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.queue.ids, vec![second.id]);
    assert!(app.context_menu.is_none());
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn mouse_maps_scrolled_queue_rows_and_rejects_stale_menu_actions() {
    let mut app = App::new(Config::default(), Queue::default());
    for i in 0..70 {
        app.queue.enqueue(test_track(i).id);
    }
    app.catalog.view = View::Queue;
    app.queue.selected = 55;
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    draw_mouse(&app, 80, 24);
    assert!(app.ui.render.borrow().queue_scroll > 0);
    click_target(
        &mut app,
        MouseTarget::Queue(55),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    app.queue.enqueue(test_track(80).id);
    activate_menu(&mut app, 3, &mut tasks, &tx);
    assert_eq!(app.queue.ids.len(), 71);
    assert!(app.status.contains("List changed"));
    draw_mouse(&app, 80, 24);
    click_target(
        &mut app,
        MouseTarget::Queue(55),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    activate_menu(&mut app, 3, &mut tasks, &tx);
    assert_eq!(app.queue.ids.len(), 70);
    assert!(!app.queue.ids.contains(&test_track(55).id));
}

#[tokio::test]
async fn mouse_queue_sidebar_selection_and_small_terminal_menu_stay_in_bounds() {
    let mut app = App::new(Config::default(), Queue::default());
    for i in 0..5 {
        app.queue.enqueue(test_track(i).id);
    }
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Queue(3),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Queue);
    assert_eq!(app.queue.selected, 3);
    let menu = app.context_menu.as_mut().unwrap();
    menu.x = 119;
    menu.y = 34;
    for (width, height) in [(120, 35), (80, 24), (32, 10)] {
        draw_mouse(&app, width, height);
        let layout = app.ui.render.borrow();
        let hits = &layout.mouse_hits;
        assert_eq!(hits.len(), 4);
        assert!(
            hits.iter()
                .all(|(rect, _)| rect.right() <= width && rect.bottom() <= height)
        );
    }
    key(
        &mut app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(app.context_menu.is_none());
    assert!(!app.quit);
}

#[tokio::test]
async fn mouse_wheel_and_playback_badge_use_existing_controls() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.rows = Rows::Tracks((0..30).map(test_track).collect());
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    draw_mouse(&app, 80, 24);
    let row = app
        .ui
        .render
        .borrow()
        .mouse_hits
        .iter()
        .find(|(_, target)| *target == MouseTarget::Catalog(0))
        .unwrap()
        .0;
    assert!(mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: row.x,
            row: row.y,
            modifiers: KeyModifiers::NONE
        },
        &mut tasks,
        &tx
    ));
    assert_eq!(app.catalog.selected, 3);
    app.state = State::Playing;
    app.loaded = true;
    draw_mouse(&app, 80, 24);
    click_target(
        &mut app,
        MouseTarget::PlayPause,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.state, State::Paused);
    assert!(matches!(rx.try_recv(), Ok(Command::Pause)));
    assert!(!mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: row.x,
            row: row.y,
            modifiers: KeyModifiers::NONE
        },
        &mut tasks,
        &tx
    ));
}
