use super::*;

fn route(app: &mut App, tasks: &mut Tasks, event: Input) -> bool {
    let (commands, _receiver) = mpsc::unbounded_channel();
    route_input(app, event, tasks, &commands)
}

#[test]
fn paste_routes_to_each_active_editor_with_shared_limits() {
    let (mut tasks, _receiver) = tasks();
    let mut app = App::new(Config::default(), Queue::default());

    app.catalog.editing = true;
    assert!(route(
        &mut app,
        &mut tasks,
        Input::Paste(format!("search\n{}", "é".repeat(600)))
    ));
    assert!(!app.catalog.query.contains('\n'));
    assert_eq!(app.catalog.query.chars().count(), 500);

    app.catalog.editing = false;
    app.catalog.filtering = true;
    assert!(route(
        &mut app,
        &mut tasks,
        Input::Paste("filter\rvalue".into())
    ));
    assert_eq!(app.catalog.filter, "filtervalue");

    app.catalog.filtering = false;
    app.ui.overlay = Overlay::Stats;
    app.ui.stats.get_mut().editing = true;
    assert!(route(
        &mut app,
        &mut tasks,
        Input::Paste("stats\nquery".into())
    ));
    assert_eq!(app.ui.stats.borrow().query, "statsquery");

    app.ui.overlay = Overlay::MixBuilder;
    app.mix.naming = true;
    app.mix.recipe_name = "夜".repeat(78);
    app.mix.detail_scroll = 12;
    assert!(route(
        &mut app,
        &mut tasks,
        Input::Paste("ABignored\n".into())
    ));
    assert_eq!(app.mix.recipe_name.chars().count(), 80);
    assert!(app.mix.recipe_name.ends_with("AB"));
    assert_eq!(app.mix.detail_scroll, 0);

    app.mix.naming = false;
    let unchanged = app.mix.recipe_name.clone();
    assert!(route(
        &mut app,
        &mut tasks,
        Input::Paste("modal paste".into())
    ));
    assert_eq!(app.mix.recipe_name, unchanged);

    app.ui.overlay = Overlay::None;
    assert!(!route(
        &mut app,
        &mut tasks,
        Input::Paste("not editing".into())
    ));
}

#[test]
fn typed_unicode_uses_character_limits_like_paste() {
    let (mut tasks, _receiver) = tasks();
    let mut app = App::new(Config::default(), Queue::default());
    let (commands, _receiver) = mpsc::unbounded_channel();

    app.catalog.editing = true;
    app.catalog.query = "é".repeat(499);
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('夜'), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    assert_eq!(app.catalog.query.chars().count(), 500);
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    assert_eq!(app.catalog.query.chars().count(), 500);

    app.catalog.editing = false;
    app.catalog.filtering = true;
    app.catalog.filter = "界".repeat(99);
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('é'), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    assert_eq!(app.catalog.filter.chars().count(), 100);
}

#[tokio::test]
async fn mouse_and_resize_events_use_the_shared_native_route() {
    let (mut tasks, _receiver) = tasks();
    let mut app = App::new(Config::default(), Queue::default());
    app.queue.enqueue("1".repeat(22));
    draw_mouse(&app, 80, 24);
    let (area, _) = app
        .ui
        .render
        .borrow()
        .mouse_hits
        .iter()
        .find(|(_, target)| *target == MouseTarget::Navigation(View::Queue))
        .copied()
        .unwrap();
    assert!(route(
        &mut app,
        &mut tasks,
        Input::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        })
    ));
    assert_eq!(app.catalog.view, View::Queue);

    tasks.open_mix(&mut app);
    let queue = app.queue.ids.clone();
    assert!(route(
        &mut app,
        &mut tasks,
        Input::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        })
    ));
    assert_eq!(
        app.queue.ids, queue,
        "Mix Builder consumes underlying mouse hits"
    );
    app.mix.detail_scroll = 30;
    assert!(route(&mut app, &mut tasks, Input::Resize(32, 10)));
    assert_eq!(app.mix.detail_scroll, 0);
}
