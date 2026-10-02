use super::*;

fn fixture() -> App {
    let mut app = App::new(Config::default(), Queue::default());
    for i in 0..5 {
        let mut track = test_track(i);
        if matches!(i, 0 | 3) {
            track.name = if i == 0 { "Night Drive" } else { "Night Rain" }.into();
            track.artists = "Élan 日本語".into();
            track.album = Some("City Lights".into());
        }
        app.cache.insert(track.id.clone(), track);
    }
    app.queue.replace(
        vec![
            test_track(0).id,
            test_track(1).id,
            test_track(0).id,
            test_track(3).id,
            test_track(4).id,
        ],
        1,
        false,
    );
    app.queue.order = vec![4, 0, 1, 3, 2];
    app.queue.cursor = Some(2);
    app.catalog.view = View::Queue;
    app
}

fn press(app: &mut App, tasks: &mut Tasks, tx: &mpsc::UnboundedSender<Command>, code: KeyCode) {
    key(app, KeyEvent::new(code, KeyModifiers::NONE), tasks, tx);
}

#[test]
fn queue_filter_matches_words_across_unicode_title_artist_and_album() {
    let mut app = fixture();
    app.ui.queue.query = "nIGHT éLAN city 日本語".into();
    assert_eq!(*app.queue_rows(), vec![1, 3, 4]);
    app.ui.queue.query = "City drive".into();
    assert_eq!(*app.queue_rows(), vec![1, 4]);
    assert_eq!(app.selected_track().unwrap().name, "Night Drive");
    app.ui.queue.query = "   \t ".into();
    assert_eq!(app.len(), 5);
}

#[tokio::test]
async fn queue_filter_typing_paste_and_navigation_leave_playback_and_order_intact() {
    let mut app = fixture();
    let before = app.queue.clone();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    press(&mut app, &mut tasks, &tx, KeyCode::Char('/'));
    assert!(app.ui.queue.editing);
    assert!(route_input(
        &mut app,
        Input::Paste("nIGHT\néLAN city".into()),
        &mut tasks,
        &tx
    ));
    assert!(!app.ui.queue.query.contains('\n'));
    // Control characters are stripped, so replace the pasted query deliberately.
    app.ui.queue.query = "night élan city".into();
    app.reset_queue_filter_selection();
    assert_eq!(app.queue.selected, 1);
    press(&mut app, &mut tasks, &tx, KeyCode::Enter);
    assert!(!app.ui.queue.editing);
    assert!(
        rx.try_recv().is_err(),
        "finishing text entry must not play a track"
    );
    press(&mut app, &mut tasks, &tx, KeyCode::Down);
    assert_eq!(app.queue.selected, 3);
    press(&mut app, &mut tasks, &tx, KeyCode::PageDown);
    assert_eq!(app.queue.selected, 4);
    press(&mut app, &mut tasks, &tx, KeyCode::PageUp);
    assert_eq!(app.queue.selected, 1);
    press(&mut app, &mut tasks, &tx, KeyCode::Esc);
    assert_eq!(app.len(), 5);
    assert!(!app.quit);
    assert_eq!(app.queue.ids, before.ids);
    assert_eq!(app.queue.order, before.order);
    assert_eq!(app.queue.cursor, before.cursor);
    assert_eq!(app.queue.position_ms, before.position_ms);
    assert_eq!(app.queue.revision, before.revision);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn queue_filter_shortcut_letters_and_unicode_limits_are_safe_text() {
    let mut app = fixture();
    let before = app.queue.ids.clone();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    press(&mut app, &mut tasks, &tx, KeyCode::Char('f'));
    for c in "q C n s R m /".chars() {
        press(&mut app, &mut tasks, &tx, KeyCode::Char(c));
    }
    assert_eq!(app.ui.queue.query, "q C n s R m /");
    assert!(!app.quit);
    assert_eq!(app.queue.ids, before);
    assert!(rx.try_recv().is_err());
    app.ui.queue.query = "夜".repeat(99);
    route_input(
        &mut app,
        Input::Paste("éignored\r\n".into()),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.queue.query.chars().count(), 100);
    press(&mut app, &mut tasks, &tx, KeyCode::Char('x'));
    assert_eq!(app.ui.queue.query.chars().count(), 100);
    press(&mut app, &mut tasks, &tx, KeyCode::Backspace);
    assert_eq!(app.ui.queue.query.chars().count(), 99);
    press(&mut app, &mut tasks, &tx, KeyCode::Esc);
    assert!(app.ui.queue.query.is_empty());
}

#[tokio::test]
async fn queue_filter_play_and_remove_target_the_selected_duplicate_occurrence() {
    let mut app = fixture();
    app.ui.queue.query = "night drive".into();
    app.queue.selected = 4;
    let order = app.queue.order.clone();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    press(&mut app, &mut tasks, &tx, KeyCode::Enter);
    assert_eq!(app.queue.cursor, Some(4));
    assert_eq!(app.queue.order, order);
    assert!(matches!(rx.try_recv().unwrap(), Command::Load { .. }));
    press(&mut app, &mut tasks, &tx, KeyCode::Delete);
    assert_eq!(
        app.queue.ids,
        vec![
            test_track(0).id,
            test_track(1).id,
            test_track(3).id,
            test_track(4).id
        ]
    );
    assert_eq!(app.queue.order, vec![3, 0, 1, 2]);
    assert_eq!(app.queue.cursor, None);
    assert_eq!(*app.queue_rows(), vec![1]);
    assert!(matches!(rx.try_recv().unwrap(), Command::Stop));
    press(&mut app, &mut tasks, &tx, KeyCode::Char('u'));
    assert_eq!(app.queue.order, order);
    assert_eq!(app.queue.cursor, Some(4));
    assert_eq!(app.state, State::Paused);
    assert_eq!(*app.queue_rows(), vec![1, 4]);
    app.queue.validate().unwrap();
}

#[tokio::test]
async fn queue_filter_reorder_moves_one_real_position_and_keeps_playing_track() {
    let mut app = fixture();
    app.ui.queue.query = "night drive".into();
    app.queue.selected = 4;
    let current = app.queue.current().unwrap().to_owned();
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    press(&mut app, &mut tasks, &tx, KeyCode::Char('K'));
    assert_eq!(app.queue.order, vec![4, 0, 1, 2, 3]);
    assert_eq!(app.queue.selected, 3);
    assert_eq!(*app.queue_rows(), vec![1, 3]);
    assert_eq!(app.queue.current(), Some(current.as_str()));
    press(&mut app, &mut tasks, &tx, KeyCode::Char('J'));
    assert_eq!(app.queue.order, vec![4, 0, 1, 3, 2]);
    assert_eq!(app.queue.selected, 4);
    app.queue.validate().unwrap();
}

#[tokio::test]
async fn queue_filter_no_matches_cannot_act_on_hidden_tracks_or_clear_queue() {
    let mut app = fixture();
    app.ui.queue.query = "no such track".into();
    let before = serde_json::to_string(&app.queue).unwrap();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    for action in [
        Action::PlaySelected,
        Action::RemoveSelected,
        Action::MoveUp,
        Action::MoveDown,
        Action::PlayNext,
        Action::EnqueueSelected,
        Action::StartRadio,
        Action::ClearQueue,
    ] {
        actions::apply(&mut app, action, &mut tasks, &tx);
        assert_eq!(
            serde_json::to_string(&app.queue).unwrap(),
            before,
            "{action:?}"
        );
    }
    assert!(app.selected_track().is_none());
    assert!(rx.try_recv().is_err());
    assert!(!app.can_undo());
}

#[test]
fn queue_filter_cache_refreshes_for_metadata_order_and_undo_epochs() {
    let mut app = fixture();
    app.ui.queue.query = "new title".into();
    let old = app.queue_rows();
    assert!(old.is_empty());
    assert!(Arc::ptr_eq(&old, &app.queue_rows()));
    let mut track = test_track(4);
    track.name = "New Title".into();
    app.cache.insert(track.id.clone(), track);
    assert_eq!(*app.queue_rows(), vec![0]);
    assert!(!Arc::ptr_eq(&old, &app.queue_rows()));
    app.queue.move_item(0, 4);
    assert_eq!(*app.queue_rows(), vec![4]);
    let before_epoch = app.queue_rows();
    app.queue.epoch += 1;
    assert!(!Arc::ptr_eq(&before_epoch, &app.queue_rows()));
    app.cache.remove(&test_track(4).id);
    assert!(app.queue_rows().is_empty());
    assert_eq!(app.ui.queue.missing(&app.queue, &app.cache), 1);
}

#[tokio::test]
async fn queue_filter_matches_arriving_metadata_and_maps_metadata_scroll() {
    let mut app = fixture();
    app.cache.remove(&test_track(3).id);
    app.ui.queue.query = "night".into();
    assert_eq!(*app.queue_rows(), vec![1, 4]);
    assert_eq!(app.ui.queue.missing(&app.queue, &app.cache), 1);
    let (mut tasks, _) = tasks();
    let mut track = test_track(3);
    track.name = "Night Rain".into();
    let request = tasks.metadata_request;
    background(
        &mut app,
        &mut tasks,
        Background::Metadata(request, track.id.clone(), Ok(track)),
    );
    assert_eq!(*app.queue_rows(), vec![1, 3, 4]);
    app.ui.render.borrow_mut().queue_scroll = 1;
    assert_eq!(app.queue_metadata_start(), 3);
    app.ui.queue.query = "no matches".into();
    app.queue.selected = 4;
    assert_eq!(app.queue_metadata_start(), 4);
}

#[tokio::test]
async fn queue_filter_mouse_hits_use_real_positions_and_menus_reject_changed_filter() {
    let mut app = fixture();
    app.ui.queue.query = "night drive".into();
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    draw_mouse(&app, 80, 24);
    click_target(
        &mut app,
        MouseTarget::Queue(4),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.queue.selected, 4);
    assert_eq!(app.context_menu.as_ref().unwrap().row, 4);
    assert_eq!(app.context_menu.as_ref().unwrap().filter, "night drive");
    app.ui.queue.query = "night".into();
    let before = app.queue.ids.clone();
    activate_menu(&mut app, 3, &mut tasks, &tx);
    assert_eq!(app.queue.ids, before);
    assert!(app.status.contains("List changed"));
    draw_mouse(&app, 80, 24);
    click_target(
        &mut app,
        MouseTarget::Queue(4),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    activate_menu(&mut app, 3, &mut tasks, &tx);
    assert_eq!(app.queue.ids.len(), 4);
    assert_eq!(
        app.queue
            .ids
            .iter()
            .filter(|id| **id == test_track(0).id)
            .count(),
        1
    );
}

#[tokio::test]
async fn queue_filter_compact_mouse_controls_and_current_track_jump() {
    let mut app = fixture();
    app.ui.queue.query = "night".into();
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    draw_mouse(&app, 32, 10);
    click_target(
        &mut app,
        MouseTarget::QueueFilter,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(app.ui.queue.editing);
    draw_mouse(&app, 32, 10);
    click_target(
        &mut app,
        MouseTarget::QueueFilterClear,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(!app.ui.queue.editing);
    assert_eq!(app.len(), 5);
    app.ui.queue.query = "night".into();
    press(&mut app, &mut tasks, &tx, KeyCode::Char('.'));
    assert!(app.ui.queue.query.is_empty());
    assert_eq!(app.queue.selected, 2);
    assert_eq!(app.selected_track().unwrap().id, test_track(1).id);
}

#[test]
fn queue_filter_survives_album_back_navigation_and_stays_separate_from_catalog_filter() {
    let mut app = fixture();
    app.ui.queue.query = "night".into();
    app.queue.selected = 4;
    app.ui.render.borrow_mut().queue_scroll = 1;
    app.push_navigation("Queue".into());
    app.catalog.view = View::Album;
    app.catalog.filter = "other filter".into();
    assert!(app.pop_navigation());
    assert_eq!(app.catalog.view, View::Queue);
    assert_eq!(app.ui.queue.query, "night");
    assert_eq!(app.queue.selected, 4);
    assert_eq!(app.ui.render.borrow().queue_scroll, 1);
    assert!(app.catalog.filter.is_empty());
}

#[tokio::test]
async fn queue_filter_side_queue_click_reveals_the_clicked_track() {
    let mut app = fixture();
    app.ui.queue.query = "no matches".into();
    app.catalog.view = View::Liked;
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Queue(2),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Queue);
    assert!(app.ui.queue.query.is_empty());
    assert_eq!(app.queue.selected, 2);
    assert_eq!(app.selected_track().unwrap().id, test_track(1).id);
    activate_menu(&mut app, 0, &mut tasks, &tx);
    assert_eq!(app.queue.cursor, Some(2));
}

#[tokio::test]
async fn queue_filter_metadata_retry_keeps_the_original_visible_request_start() {
    let mut app = fixture();
    app.ui.queue.query = "night".into();
    app.queue.selected = 4;
    draw_mouse(&app, 80, 24);
    assert_eq!(app.queue_metadata_start(), 1);
    let (mut tasks, _) = tasks();
    tasks.retry_metadata(&mut app);
    assert!(app.queue_rows().is_empty());
    draw_mouse(&app, 80, 24);
    assert_eq!(
        app.queue_metadata_start(),
        1,
        "retry must hydrate the same original window after matches disappear"
    );
    app.clear_queue_filter();
    assert_eq!(app.queue_metadata_start(), 0);
}
