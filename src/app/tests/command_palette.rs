use super::*;

fn send(app: &mut App, tasks: &mut Tasks, tx: &mpsc::UnboundedSender<Command>, code: KeyCode) {
    route_input(
        app,
        Input::Key(KeyEvent::new(code, KeyModifiers::NONE)),
        tasks,
        tx,
    );
}

fn query(app: &mut App, tasks: &mut Tasks, tx: &mpsc::UnboundedSender<Command>, text: &str) {
    send(app, tasks, tx, KeyCode::F(8));
    route_input(app, Input::Paste(text.into()), tasks, tx);
}

fn screen(app: &App, width: u16, height: u16) -> String {
    screen_buffer(app, width, height)
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn screen_buffer(app: &App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| ui::draw(frame, app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn palette_text(app: &App, width: u16, height: u16) -> String {
    let buffer = screen_buffer(app, width, height);
    let panel_width = width.min(76);
    let panel_height = height.min(18);
    let x = (width - panel_width) / 2;
    let y = (height - panel_height) / 2;
    (y + 1..y + panel_height - 1)
        .map(|row| {
            (x + 1..x + panel_width - 1)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn edit_stats_search(app: &mut App, tasks: &mut Tasks, tx: &mpsc::UnboundedSender<Command>) {
    app.stats.add_play(&test_track(0).id, "Song 0", "Artist");
    send(app, tasks, tx, KeyCode::Char('S'));
    send(app, tasks, tx, KeyCode::Char('/'));
    route_input(app, Input::Paste("stats draft".into()), tasks, tx);
    assert_eq!(app.ui.overlay, Overlay::Stats);
    assert_eq!(app.ui.stats.borrow().query, "stats draft");
}

#[test]
fn command_palette_empty_queue_reports_empty_instead_of_filter_guidance() {
    for command in [
        "move selected queue entry up",
        "move selected queue entry down",
        "remove selected queue entry",
        "clear entire queue",
    ] {
        let mut app = App::new(Config::default(), Queue::default());
        app.catalog.view = View::Queue;
        let (mut tasks, _) = tasks();
        let (tx, mut rx) = mpsc::unbounded_channel();
        query(&mut app, &mut tasks, &tx, command);
        send(&mut app, &mut tasks, &tx, KeyCode::Enter);
        assert_eq!(app.status, "The queue is empty.", "{command}");
        assert!(palette_text(&app, 120, 32).contains("The queue is empty."));
        assert!(app.ui.palette.is_some());
        assert!(app.queue.ids.is_empty());
        assert!(!app.can_undo());
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn command_palette_nonempty_queue_keeps_filter_and_stale_selection_guards() {
    for (filter, selected) in [("no matching song", 0), ("", 10)] {
        let mut app = App::new(Config::default(), Queue::default());
        let track = test_track(0);
        app.queue.enqueue(track.id.clone());
        app.cache.insert(track.id.clone(), track);
        app.catalog.view = View::Queue;
        app.ui.queue.query = filter.into();
        app.queue.selected = selected;
        let before = app.queue.ids.clone();
        let (mut tasks, _) = tasks();
        let (tx, mut rx) = mpsc::unbounded_channel();
        query(&mut app, &mut tasks, &tx, "remove selected queue entry");
        send(&mut app, &mut tasks, &tx, KeyCode::Enter);
        let reason = "No matching queue tracks. Press / to edit or Esc to clear the filter.";
        assert_eq!(app.status, reason);
        assert!(palette_text(&app, 120, 32).contains(reason));
        assert!(app.ui.palette.is_some());
        assert_eq!(app.queue.ids, before);
        assert_eq!(app.queue.selected, selected);
        assert!(!app.can_undo());
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn command_palette_undo_restores_a_cleared_queue() {
    let mut app = crate::demo::app();
    let before = app.queue.ids.clone();
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    query(&mut app, &mut tasks, &tx, "clear entire queue");
    send(&mut app, &mut tasks, &tx, KeyCode::Enter);
    assert!(app.queue.ids.is_empty());
    query(&mut app, &mut tasks, &tx, "undo queue edit");
    send(&mut app, &mut tasks, &tx, KeyCode::Enter);
    assert!(app.ui.palette.is_none());
    assert_eq!(app.queue.ids, before);
}

#[test]
fn command_palette_leaving_stats_for_queue_restores_f6_and_f7() {
    let mut app = crate::demo::app();
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    edit_stats_search(&mut app, &mut tasks, &tx);
    query(&mut app, &mut tasks, &tx, "view queue");
    send(&mut app, &mut tasks, &tx, KeyCode::Enter);
    assert!(app.ui.palette.is_none());
    assert_eq!(app.ui.overlay, Overlay::None);
    assert_eq!(app.catalog.view, View::Queue);
    assert_eq!(app.ui.stats.borrow().query, "stats draft");
    send(&mut app, &mut tasks, &tx, KeyCode::F(6));
    assert_eq!(app.ui.overlay, Overlay::ListeningTools);
    send(&mut app, &mut tasks, &tx, KeyCode::Esc);
    send(&mut app, &mut tasks, &tx, KeyCode::F(7));
    assert_eq!(app.ui.overlay, Overlay::Diagnostics);
}

#[tokio::test]
async fn command_palette_leaving_stats_for_another_overlay_restores_f6_and_f7() {
    for (command, overlay) in [
        ("toggle lyrics view", Overlay::Lyrics),
        ("toggle visualizer", Overlay::Visualizer),
        ("toggle listening tools", Overlay::ListeningTools),
        ("open mix builder", Overlay::MixBuilder),
    ] {
        let mut app = crate::demo::app();
        let (mut tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();
        edit_stats_search(&mut app, &mut tasks, &tx);
        query(&mut app, &mut tasks, &tx, command);
        send(&mut app, &mut tasks, &tx, KeyCode::Enter);
        assert!(app.ui.palette.is_none(), "{command}");
        assert_eq!(app.ui.overlay, overlay, "{command}");
        assert_eq!(app.ui.stats.borrow().query, "stats draft", "{command}");
        send(&mut app, &mut tasks, &tx, KeyCode::Esc);
        send(&mut app, &mut tasks, &tx, KeyCode::F(6));
        assert_eq!(app.ui.overlay, Overlay::ListeningTools, "{command}");
        send(&mut app, &mut tasks, &tx, KeyCode::Esc);
        send(&mut app, &mut tasks, &tx, KeyCode::F(7));
        assert_eq!(app.ui.overlay, Overlay::Diagnostics, "{command}");
    }
}

#[test]
fn command_palette_preserves_stats_search_when_it_stays_open() {
    for (command, key, palette_open) in [
        ("view queue", KeyCode::Esc, false),
        ("no-such-command", KeyCode::Enter, true),
        ("start radio", KeyCode::Enter, true),
        ("view playlists", KeyCode::Enter, true),
        ("cycle theme", KeyCode::Enter, false),
        ("increase volume", KeyCode::Enter, false),
    ] {
        let mut app = crate::demo::app();
        app.config.source = crate::model::MusicSource::Youtube;
        app.config.youtube_connected = false;
        let (mut tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();
        edit_stats_search(&mut app, &mut tasks, &tx);
        query(&mut app, &mut tasks, &tx, command);
        send(&mut app, &mut tasks, &tx, key);
        assert_eq!(app.ui.palette.is_some(), palette_open, "{command}");
        if palette_open {
            send(&mut app, &mut tasks, &tx, KeyCode::Esc);
        }
        assert_eq!(app.ui.overlay, Overlay::Stats, "{command}");
        assert_eq!(app.ui.stats.borrow().query, "stats draft", "{command}");
        send(&mut app, &mut tasks, &tx, KeyCode::Char('x'));
        assert_eq!(app.ui.stats.borrow().query, "stats draftx", "{command}");
    }
}

#[tokio::test]
async fn command_palette_leaving_base_editors_for_overlays_restores_f6_and_f7() {
    for view in [View::Search, View::Liked, View::Queue] {
        for (command, overlay) in [
            ("toggle lyrics view", Overlay::Lyrics),
            ("toggle visualizer", Overlay::Visualizer),
            ("toggle listening tools", Overlay::ListeningTools),
            ("open song statistics", Overlay::Stats),
            ("open mix builder", Overlay::MixBuilder),
        ] {
            let mut app = crate::demo::app();
            app.catalog.view = view;
            app.catalog.query.clear();
            let (mut tasks, _) = tasks();
            let (tx, _) = mpsc::unbounded_channel();
            send(
                &mut app,
                &mut tasks,
                &tx,
                if view == View::Search {
                    KeyCode::F(2)
                } else {
                    KeyCode::Char('/')
                },
            );
            route_input(&mut app, Input::Paste("draft".into()), &mut tasks, &tx);
            query(&mut app, &mut tasks, &tx, command);
            send(&mut app, &mut tasks, &tx, KeyCode::Enter);
            assert!(app.ui.palette.is_none(), "{view:?}: {command}");
            assert_eq!(app.ui.overlay, overlay, "{view:?}: {command}");
            let draft = match view {
                View::Search => &app.catalog.query,
                View::Liked => &app.catalog.filter,
                View::Queue => &app.ui.queue.query,
                _ => unreachable!(),
            };
            assert_eq!(draft, "draft", "{view:?}: {command}");
            if overlay == Overlay::MixBuilder {
                send(&mut app, &mut tasks, &tx, KeyCode::Esc);
            }
            send(&mut app, &mut tasks, &tx, KeyCode::F(6));
            if overlay == Overlay::ListeningTools {
                assert_eq!(app.ui.overlay, Overlay::None, "{view:?}: {command}");
                send(&mut app, &mut tasks, &tx, KeyCode::F(6));
            }
            assert_eq!(
                app.ui.overlay,
                Overlay::ListeningTools,
                "{view:?}: {command}"
            );
            send(&mut app, &mut tasks, &tx, KeyCode::Esc);
            send(&mut app, &mut tasks, &tx, KeyCode::F(7));
            assert_eq!(app.ui.overlay, Overlay::Diagnostics, "{view:?}: {command}");
        }
    }
}

#[tokio::test]
async fn command_palette_leaving_search_for_track_details_finishes_the_editor() {
    for (command, view) in [("view album", View::Album), ("view artist", View::Artist)] {
        let mut app = crate::demo::app();
        app.catalog.view = View::Search;
        app.catalog.query.clear();
        let mut track = test_track(0);
        track.album_id = Some("1".repeat(22));
        track.artist_ids = vec!["2".repeat(22)];
        let (mut tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();
        send(&mut app, &mut tasks, &tx, KeyCode::F(2));
        app.catalog.rows = Rows::Tracks(vec![track]);
        app.catalog.selected = 0;
        route_input(&mut app, Input::Paste("draft".into()), &mut tasks, &tx);
        query(&mut app, &mut tasks, &tx, command);
        send(&mut app, &mut tasks, &tx, KeyCode::Enter);
        assert!(app.ui.palette.is_none(), "{command}");
        assert_eq!(app.catalog.view, view, "{command}");
        assert_eq!(app.catalog.query, "draft", "{command}");
        send(&mut app, &mut tasks, &tx, KeyCode::F(6));
        assert_eq!(app.ui.overlay, Overlay::ListeningTools, "{command}");
    }
}

#[test]
fn command_palette_preserves_base_editors_when_the_context_stays_open() {
    for view in [View::Search, View::Liked, View::Queue] {
        for (command, key, palette_open) in [
            ("toggle lyrics view", KeyCode::Esc, false),
            ("no-such-command", KeyCode::Enter, true),
            ("connect spotify", KeyCode::Enter, true),
            ("view playlists", KeyCode::Enter, true),
            ("cycle theme", KeyCode::Enter, false),
            ("increase volume", KeyCode::Enter, false),
        ] {
            let mut app = crate::demo::app();
            app.config.source = crate::model::MusicSource::Youtube;
            app.config.youtube_connected = false;
            app.catalog.view = view;
            app.catalog.query.clear();
            let (mut tasks, _) = tasks();
            let (tx, _) = mpsc::unbounded_channel();
            send(
                &mut app,
                &mut tasks,
                &tx,
                if view == View::Search {
                    KeyCode::F(2)
                } else {
                    KeyCode::Char('/')
                },
            );
            route_input(&mut app, Input::Paste("draft".into()), &mut tasks, &tx);
            query(&mut app, &mut tasks, &tx, command);
            send(&mut app, &mut tasks, &tx, key);
            assert_eq!(
                app.ui.palette.is_some(),
                palette_open,
                "{view:?}: {command}"
            );
            if palette_open {
                send(&mut app, &mut tasks, &tx, KeyCode::Esc);
            }
            assert_eq!(app.ui.overlay, Overlay::None, "{view:?}: {command}");
            assert_eq!(app.catalog.view, view, "{view:?}: {command}");
            send(&mut app, &mut tasks, &tx, KeyCode::Char('x'));
            let draft = match view {
                View::Search => &app.catalog.query,
                View::Liked => &app.catalog.filter,
                View::Queue => &app.ui.queue.query,
                _ => unreachable!(),
            };
            assert_eq!(draft, "draftx", "{view:?}: {command}");
        }
    }
}

#[tokio::test]
async fn command_palette_finishes_mix_naming_only_when_leaving_its_overlay() {
    for (command, key, overlay, palette_open) in [
        ("toggle lyrics view", KeyCode::Enter, Overlay::Lyrics, false),
        ("view queue", KeyCode::Enter, Overlay::None, false),
        (
            "toggle lyrics view",
            KeyCode::Esc,
            Overlay::MixBuilder,
            false,
        ),
        ("no-such-command", KeyCode::Enter, Overlay::MixBuilder, true),
        ("connect spotify", KeyCode::Enter, Overlay::MixBuilder, true),
        ("view playlists", KeyCode::Enter, Overlay::MixBuilder, true),
        ("cycle theme", KeyCode::Enter, Overlay::MixBuilder, false),
        (
            "increase volume",
            KeyCode::Enter,
            Overlay::MixBuilder,
            false,
        ),
    ] {
        let mut app = crate::demo::app();
        app.config.source = crate::model::MusicSource::Youtube;
        app.config.youtube_connected = false;
        let (mut tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();
        send(&mut app, &mut tasks, &tx, KeyCode::Char('M'));
        app.mix.recipe_name = "mix draft".into();
        send(&mut app, &mut tasks, &tx, KeyCode::Char('w'));
        query(&mut app, &mut tasks, &tx, command);
        send(&mut app, &mut tasks, &tx, key);
        assert_eq!(app.ui.palette.is_some(), palette_open, "{command}");
        assert_eq!(app.ui.overlay, overlay, "{command}");
        assert_eq!(app.mix.recipe_name, "mix draft", "{command}");
        assert_eq!(app.mix.naming, overlay == Overlay::MixBuilder, "{command}");
        if overlay == Overlay::MixBuilder {
            if palette_open {
                send(&mut app, &mut tasks, &tx, KeyCode::Esc);
            }
            send(&mut app, &mut tasks, &tx, KeyCode::Char('x'));
            assert_eq!(app.mix.recipe_name, "mix draftx", "{command}");
        }
    }
}

#[test]
fn command_palette_cancel_preserves_every_editor_and_selection() {
    for mode in 0..5 {
        let mut app = crate::demo::app();
        app.catalog.editing = mode == 0;
        app.catalog.filtering = mode == 1;
        app.catalog.query = "search draft".into();
        app.catalog.filter = "filter draft".into();
        app.catalog.selected = 2;
        app.queue.selected = 3;
        app.ui.queue.query = "queue draft".into();
        app.ui.queue.editing = mode == 2;
        if mode == 2 {
            app.catalog.view = View::Queue;
        }
        if mode == 3 {
            app.ui.overlay = Overlay::Stats;
        }
        if mode == 4 {
            app.ui.overlay = Overlay::MixBuilder;
        }
        app.ui.stats.get_mut().editing = mode == 3;
        app.ui.stats.get_mut().query = "stats draft".into();
        app.ui.stats.get_mut().selected = 4;
        app.mix.naming = mode == 4;
        app.mix.recipe_name = "mix draft".into();
        let overlay = app.ui.overlay;
        let view = app.catalog.view;
        let (mut tasks, _) = tasks();
        let (tx, mut rx) = mpsc::unbounded_channel();
        route_input(
            &mut app,
            Input::Key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)),
            &mut tasks,
            &tx,
        );
        assert!(app.ui.palette.is_some());
        send(&mut app, &mut tasks, &tx, KeyCode::Char('q'));
        send(&mut app, &mut tasks, &tx, KeyCode::Char(' '));
        route_input(
            &mut app,
            Input::Paste("palette\nquery".into()),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.ui.palette.as_ref().unwrap().query, "q palettequery");
        send(&mut app, &mut tasks, &tx, KeyCode::Esc);
        assert!(app.ui.palette.is_none());
        assert_eq!(app.ui.overlay, overlay);
        assert_eq!(app.catalog.view, view);
        assert_eq!(app.catalog.query, "search draft");
        assert_eq!(app.catalog.filter, "filter draft");
        assert_eq!(app.catalog.selected, 2);
        assert_eq!(app.queue.selected, 3);
        assert_eq!(app.ui.queue.query, "queue draft");
        assert_eq!(app.ui.stats.borrow().query, "stats draft");
        assert_eq!(app.ui.stats.borrow().selected, 4);
        assert_eq!(app.mix.recipe_name, "mix draft");
        assert_eq!(app.catalog.editing, mode == 0);
        assert_eq!(app.catalog.filtering, mode == 1);
        assert_eq!(app.ui.queue.editing, mode == 2);
        assert_eq!(app.ui.stats.borrow().editing, mode == 3);
        assert_eq!(app.mix.naming, mode == 4);
        assert!(!app.quit);
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn command_palette_enter_runs_one_command_and_never_plays_the_underlying_row() {
    for command in ["increase volume", "play or pause"] {
        let mut app = crate::demo::app();
        app.loaded = true;
        app.state = State::Playing;
        let initial_volume = app.config.volume;
        let (mut tasks, _) = tasks();
        let (tx, mut rx) = mpsc::unbounded_channel();
        query(&mut app, &mut tasks, &tx, command);
        for kind in [
            KeyEventKind::Press,
            KeyEventKind::Press,
            KeyEventKind::Repeat,
            KeyEventKind::Release,
        ] {
            route_input(
                &mut app,
                Input::Key(KeyEvent::new_with_kind(
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                    kind,
                )),
                &mut tasks,
                &tx,
            );
        }
        assert!(app.ui.palette.is_none());
        let effect = rx.try_recv().unwrap();
        if command == "increase volume" {
            assert!(
                matches!(effect, Command::Volume(volume) if volume == (initial_volume + 5).min(100))
            );
        } else {
            assert!(matches!(effect, Command::Pause));
            assert_eq!(app.state, State::Paused);
        }
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn command_palette_focus_loss_and_other_keys_release_enter_latch() {
    let mut app = crate::demo::app();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    for reset in [
        Input::FocusLost,
        Input::Key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE)),
    ] {
        query(&mut app, &mut tasks, &tx, "increase volume");
        send(&mut app, &mut tasks, &tx, KeyCode::Enter);
        assert!(matches!(rx.try_recv(), Ok(Command::Volume(_))));
        route_input(&mut app, reset, &mut tasks, &tx);
        if app.ui.palette.is_none() {
            send(&mut app, &mut tasks, &tx, KeyCode::F(8));
        }
        route_input(
            &mut app,
            Input::Paste("increase volume".into()),
            &mut tasks,
            &tx,
        );
        send(&mut app, &mut tasks, &tx, KeyCode::Enter);
        assert!(matches!(rx.try_recv(), Ok(Command::Volume(_))));
    }
}

#[test]
fn command_palette_disabled_reasons_are_visible_and_rechecked_at_execution() {
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    for (text, overlay, reason) in [
        ("start radio", Overlay::Stats, "Close the overlay to select"),
        ("stop after", Overlay::None, "Start a track before choosing"),
        (
            "connect spotify",
            Overlay::None,
            "Account connections are unavailable",
        ),
    ] {
        let mut app = crate::demo::app();
        app.ui.overlay = overlay;
        app.loaded = false;
        query(&mut app, &mut tasks, &tx, text);
        send(&mut app, &mut tasks, &tx, KeyCode::Enter);
        assert!(app.ui.palette.is_some());
        assert!(screen(&app, 120, 32).contains(reason));
        assert!(!app.quit);
        assert!(rx.try_recv().is_err());
    }
    let mut app = crate::demo::app();
    app.catalog.view = View::Liked;
    app.catalog.rows = Rows::Tracks(vec![test_track(0)]);
    app.catalog.selected = 0;
    query(&mut app, &mut tasks, &tx, "add selected");
    if let Rows::Tracks(rows) = &mut app.catalog.rows {
        rows[0].playable = false;
    }
    let before = app.queue.ids.clone();
    send(&mut app, &mut tasks, &tx, KeyCode::Enter);
    assert!(app.ui.palette.is_some());
    assert_eq!(app.queue.ids, before);
    assert!(screen(&app, 120, 32).contains("Unavailable track cannot be queued"));
}

#[test]
fn command_palette_disconnected_library_keeps_navigation_and_explains_the_refusal() {
    for command in ["view playlists", "view liked"] {
        let mut app = crate::demo::app();
        app.config.source = crate::model::MusicSource::Youtube;
        app.config.youtube_connected = false;
        app.push_navigation("Previous search".into());
        let history = format!("{:?}", app.catalog.history);
        let view = app.catalog.view;
        let queue = app.queue.ids.clone();
        let (mut tasks, _) = tasks();
        let (tx, mut rx) = mpsc::unbounded_channel();
        query(&mut app, &mut tasks, &tx, command);
        send(&mut app, &mut tasks, &tx, KeyCode::Enter);
        assert!(app.ui.palette.is_some());
        assert_eq!(format!("{:?}", app.catalog.history), history);
        assert_eq!(app.catalog.view, view);
        assert_eq!(app.queue.ids, queue);
        assert!(rx.try_recv().is_err());
        let reason = "This library view is unavailable until your Google library is connected. F6 > Connect Google music library. Your queue is preserved.";
        assert_eq!(app.status, reason);
        for (width, height) in [(32, 10), (120, 32)] {
            let text = screen(&app, width, height);
            let normalized = palette_text(&app, width, height);
            assert!(normalized.contains(reason), "{width}x{height}: {text}");
            assert!(text.contains(&format!("> {command}")), "{text}");
            assert!(text.contains("× View"), "{text}");
            assert!(text.contains("Enter Run"), "{text}");
            assert!(text.contains("Esc Cancel"), "{text}");
        }
    }
}

#[test]
fn command_palette_account_refusal_fits_compact_and_wide_layouts() {
    let mut app = crate::demo::app();
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    query(&mut app, &mut tasks, &tx, "connect spotify");
    send(&mut app, &mut tasks, &tx, KeyCode::Enter);
    assert!(app.ui.palette.is_some());
    for (width, height) in [(32, 10), (120, 32)] {
        let text = screen(&app, width, height);
        let normalized = palette_text(&app, width, height);
        assert!(
            normalized.contains("Account connections are unavailable in the offline demo."),
            "{width}x{height}: {text}"
        );
        assert!(text.contains("> connect spotify"), "{text}");
        assert!(text.contains("× Connect Spotify"), "{text}");
        assert!(text.contains("Enter Run"), "{text}");
        assert!(text.contains("Esc Cancel"), "{text}");
    }
}

#[test]
fn command_palette_empty_album_and_artist_selection_keeps_the_existing_messages() {
    for (command, reason) in [
        ("view album", "Select a track to view album."),
        ("view artist", "Select a track to view artist."),
    ] {
        let mut app = App::new(Config::default(), Queue::default());
        app.catalog.view = View::Search;
        app.catalog.rows = Rows::Tracks(Vec::new());
        let (mut tasks, _) = tasks();
        let (tx, mut rx) = mpsc::unbounded_channel();
        query(&mut app, &mut tasks, &tx, command);
        send(&mut app, &mut tasks, &tx, KeyCode::Enter);
        assert!(app.ui.palette.is_some());
        assert_eq!(app.status, reason);
        assert_eq!(app.catalog.view, View::Search);
        assert!(app.catalog.history.is_empty());
        assert!(rx.try_recv().is_err());
        for (width, height) in [(32, 10), (120, 32)] {
            let text = palette_text(&app, width, height);
            assert!(text.contains(reason), "{width}x{height}: {text}");
        }
    }
}

#[test]
fn command_palette_preserves_context_menu_on_cancel_and_clears_it_on_dispatch() {
    let mut app = crate::demo::app();
    app.context_menu = Some(ContextMenu {
        selected: 0,
        filter: "menu draft".into(),
        x: 10,
        y: 10,
        actions: vec![("Play", Action::PlaySelected)],
        view: app.catalog.view,
        row: 0,
        revision: 0,
    });
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    query(&mut app, &mut tasks, &tx, "view queue");
    send(&mut app, &mut tasks, &tx, KeyCode::Esc);
    assert_eq!(app.context_menu.as_ref().unwrap().filter, "menu draft");
    query(&mut app, &mut tasks, &tx, "connect spotify");
    send(&mut app, &mut tasks, &tx, KeyCode::Enter);
    assert!(app.ui.palette.is_some());
    assert_eq!(app.context_menu.as_ref().unwrap().filter, "menu draft");
    send(&mut app, &mut tasks, &tx, KeyCode::Esc);
    query(&mut app, &mut tasks, &tx, "view queue");
    send(&mut app, &mut tasks, &tx, KeyCode::Enter);
    assert!(app.ui.palette.is_none());
    assert!(app.context_menu.is_none());
    assert_eq!(app.catalog.view, View::Queue);
    app.queue.selected = 0;
    send(&mut app, &mut tasks, &tx, KeyCode::Down);
    assert_eq!(app.queue.selected, 1);
}

#[test]
fn command_palette_queue_actions_resolve_the_filtered_original_entry() {
    let mut app = App::new(Config::default(), Queue::default());
    for i in 0..3 {
        let track = test_track(i);
        app.queue.enqueue(track.id.clone());
        app.cache.insert(track.id.clone(), track);
    }
    app.catalog.view = View::Queue;
    app.ui.queue.query = "Song 1".into();
    app.reset_queue_filter_selection();
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    query(&mut app, &mut tasks, &tx, "clear entire");
    send(&mut app, &mut tasks, &tx, KeyCode::Enter);
    assert!(app.ui.palette.is_some());
    assert_eq!(app.queue.ids.len(), 3);
    assert!(screen(&app, 120, 32).contains("Press Esc to clear the filter"));
    send(&mut app, &mut tasks, &tx, KeyCode::Esc);
    query(&mut app, &mut tasks, &tx, "remove selected");
    send(&mut app, &mut tasks, &tx, KeyCode::Enter);
    assert!(app.ui.palette.is_none());
    assert_eq!(app.queue.ids, vec![test_track(0).id, test_track(2).id]);
}

#[test]
fn command_palette_consumes_stale_mouse_hits_and_scrolls_only_its_rows() {
    let mut app = crate::demo::app();
    app.loaded = true;
    app.state = State::Playing;
    draw_mouse(&app, 120, 32);
    let stale = app
        .ui
        .render
        .borrow()
        .mouse_hits
        .iter()
        .find(|(_, target)| *target == MouseTarget::PlayPause)
        .unwrap()
        .0;
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    send(&mut app, &mut tasks, &tx, KeyCode::F(8));
    let click = |column, row, kind| {
        Input::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    route_input(
        &mut app,
        click(stale.x, stale.y, MouseEventKind::Down(MouseButton::Left)),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.state, State::Playing);
    assert!(rx.try_recv().is_err());
    for (width, height) in [(32, 10), (120, 32)] {
        let text = screen(&app, width, height);
        assert!(text.contains("COMMANDS"));
        assert!(text.contains("Enter Run"));
        assert!(text.contains("Esc Cancel"));
        let render = app.ui.render.borrow();
        assert!(
            render
                .mouse_hits
                .iter()
                .all(|(area, target)| area.right() <= width
                    && area.bottom() <= height
                    && matches!(
                        target,
                        MouseTarget::PaletteRow(_)
                            | MouseTarget::PaletteApply
                            | MouseTarget::PaletteClose
                    ))
        );
    }
    let before = app.ui.palette.as_ref().unwrap().selected;
    route_input(
        &mut app,
        click(0, 0, MouseEventKind::ScrollDown),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.palette.as_ref().unwrap().selected, before + 1);
    assert_eq!(app.state, State::Playing);
    assert!(rx.try_recv().is_err());
    route_input(
        &mut app,
        Input::Paste("no-such-command".into()),
        &mut tasks,
        &tx,
    );
    route_input(
        &mut app,
        click(stale.x, stale.y, MouseEventKind::Down(MouseButton::Left)),
        &mut tasks,
        &tx,
    );
    assert!(app.ui.palette.is_some());
    assert!(screen(&app, 32, 10).contains("No matching commands"));
    send(&mut app, &mut tasks, &tx, KeyCode::Enter);
    assert!(app.ui.palette.is_some());
}

#[test]
fn command_palette_search_is_tokenized_bounded_and_exposes_everyday_actions() {
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    for (text, label) in [
        ("LYRICS", "Toggle lyrics view"),
        ("radio", "Start radio"),
        ("sleep", "Sleep in 15"),
        ("timer", "Sleep in 15"),
        ("shuffle", "Cycle shuffle"),
        ("connect", "Connect Spotify"),
        ("volume increase", "Increase volume"),
        ("theme", "Cycle theme"),
    ] {
        let mut app = crate::demo::app();
        query(&mut app, &mut tasks, &tx, text);
        assert!(
            crate::app::command_palette::matches(&app)
                .iter()
                .any(|spec| spec.label.contains(label)),
            "{text}"
        );
    }
    let mut app = crate::demo::app();
    query(
        &mut app,
        &mut tasks,
        &tx,
        &format!("\n{}\r", "界".repeat(120)),
    );
    assert_eq!(app.ui.palette.as_ref().unwrap().query.chars().count(), 100);
    assert!(!app.ui.palette.as_ref().unwrap().query.contains('\n'));
    send(&mut app, &mut tasks, &tx, KeyCode::Backspace);
    assert_eq!(app.ui.palette.as_ref().unwrap().query.chars().count(), 99);
    route_input(
        &mut app,
        Input::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        &mut tasks,
        &tx,
    );
    assert!(app.quit);
}

#[test]
fn command_palette_navigation_and_compact_disabled_reason_remain_visible() {
    let mut app = crate::demo::app();
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    send(&mut app, &mut tasks, &tx, KeyCode::F(8));
    send(&mut app, &mut tasks, &tx, KeyCode::End);
    let last = crate::app::command_palette::matches(&app).len() - 1;
    assert_eq!(app.ui.palette.as_ref().unwrap().selected, last);
    send(&mut app, &mut tasks, &tx, KeyCode::PageUp);
    assert_eq!(app.ui.palette.as_ref().unwrap().selected, last - 8);
    send(&mut app, &mut tasks, &tx, KeyCode::Home);
    assert_eq!(app.ui.palette.as_ref().unwrap().selected, 0);
    app.ui.overlay = Overlay::MixBuilder;
    route_input(
        &mut app,
        Input::Paste("start radio".into()),
        &mut tasks,
        &tx,
    );
    let text = screen(&app, 32, 10);
    assert!(text.contains("Close the overlay to select"), "{text}");
    assert!(text.contains("Enter Run"));
}

#[test]
fn command_palette_aliases_and_ranking_keep_the_requested_action_selected() {
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    for (text, label) in [
        ("connect", "Connect Spotify account"),
        ("volume up", "Increase volume"),
        ("volume down", "Decrease volume"),
        ("timer 30", "Sleep in 30 minutes"),
    ] {
        let mut app = crate::demo::app();
        query(&mut app, &mut tasks, &tx, text);
        let rows = crate::app::command_palette::matches(&app);
        assert_eq!(rows[app.ui.palette.as_ref().unwrap().selected].label, label);
        if text == "connect" {
            assert!(rows[0].unavailable(&app).is_some());
            assert!(!rows.iter().any(|row| row.label.starts_with("Sleep")));
        }
    }
}

#[tokio::test]
async fn command_palette_shared_ui_commands_match_the_advertised_shortcut() {
    for (text, shortcut) in [
        ("cycle theme", 't'),
        ("cycle shuffle", 's'),
        ("cycle repeat", 'r'),
        ("toggle lyrics view", 'l'),
        ("toggle docked lyrics", 'L'),
        ("toggle visualizer", 'v'),
        ("open song statistics", 'S'),
    ] {
        let mut direct = crate::demo::app();
        let mut palette = crate::demo::app();
        let (mut direct_tasks, _) = tasks();
        let (mut palette_tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();
        send(&mut direct, &mut direct_tasks, &tx, KeyCode::Char(shortcut));
        query(&mut palette, &mut palette_tasks, &tx, text);
        send(&mut palette, &mut palette_tasks, &tx, KeyCode::Enter);
        assert!(palette.ui.palette.is_none(), "{text}");
        assert_eq!(palette.config.theme, direct.config.theme);
        assert_eq!(palette.config.repeat, direct.config.repeat);
        assert_eq!(palette.config.shuffle, direct.config.shuffle);
        assert_eq!(palette.queue.smart_shuffle, direct.queue.smart_shuffle);
        assert_eq!(palette.ui.lyrics_panel, direct.ui.lyrics_panel);
        assert_eq!(palette.ui.overlay, direct.ui.overlay);
        assert_eq!(palette.status, direct.status);
    }
}

#[test]
fn command_palette_mouse_selects_then_runs_and_clears_hits_on_resize() {
    let mut app = crate::demo::app();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    query(&mut app, &mut tasks, &tx, "volume");
    draw_mouse(&app, 120, 32);
    let at = crate::app::command_palette::matches(&app)
        .iter()
        .position(|row| row.label == "Increase volume")
        .unwrap();
    let row = app
        .ui
        .render
        .borrow()
        .mouse_hits
        .iter()
        .find(|(_, target)| *target == MouseTarget::PaletteRow(at))
        .unwrap()
        .0;
    route_input(
        &mut app,
        Input::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: row.x,
            row: row.y,
            modifiers: KeyModifiers::NONE,
        }),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.palette.as_ref().unwrap().selected, at);
    assert!(rx.try_recv().is_err());
    let apply = app
        .ui
        .render
        .borrow()
        .mouse_hits
        .iter()
        .find(|(_, target)| *target == MouseTarget::PaletteApply)
        .unwrap()
        .0;
    route_input(
        &mut app,
        Input::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: apply.x,
            row: apply.y,
            modifiers: KeyModifiers::NONE,
        }),
        &mut tasks,
        &tx,
    );
    assert!(matches!(rx.try_recv(), Ok(Command::Volume(_))));
    assert!(app.ui.palette.is_none());
    assert!(app.ui.render.borrow().mouse_hits.is_empty());
    send(&mut app, &mut tasks, &tx, KeyCode::F(8));
    draw_mouse(&app, 120, 32);
    assert!(!app.ui.render.borrow().mouse_hits.is_empty());
    route_input(&mut app, Input::Resize(32, 10), &mut tasks, &tx);
    assert!(app.ui.render.borrow().mouse_hits.is_empty());
}
