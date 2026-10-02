use super::*;

fn fixture() -> App {
    let mut app = App::new(Config::default(), Queue::default());
    for (i, name) in ["Night Drive", "Daylight", "Current Song"]
        .into_iter()
        .enumerate()
    {
        let track = Track {
            id: format!("{i:022}"),
            name: name.into(),
            artists: "Élan".into(),
            duration_ms: 180_000,
            playable: true,
            ..Default::default()
        };
        app.cache.insert(track.id.clone(), track);
    }
    app.queue.replace(
        vec![
            format!("{:022}", 0),
            format!("{:022}", 1),
            format!("{:022}", 0),
            format!("{:022}", 2),
        ],
        3,
        false,
    );
    app.catalog.view = View::Queue;
    app.ui.queue.query = "night".into();
    app
}

fn screen(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn queue_filter_render_keeps_matches_and_clear_control_visible_in_all_themes_and_sizes() {
    for theme in [
        "spotify",
        "amber",
        "matrix",
        "cyberpunk",
        "monochrome",
        "glass",
    ] {
        for (width, height) in [(32, 10), (40, 12), (60, 18), (80, 24), (120, 35)] {
            let mut app = fixture();
            app.config.theme = theme.into();
            app.ui.queue.editing = true;
            let before = app.queue.order.clone();
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
                .unwrap();
            let text = screen(&terminal);
            assert!(text.contains("2/4"), "{width}x{height}: {text}");
            assert!(text.contains("Esc Clear"), "{width}x{height}: {text}");
            assert!(text.contains("Night Drive"), "{width}x{height}: {text}");
            assert!(!text.contains("Daylight"), "{text}");
            let render = app.ui.render.borrow();
            assert!(
                render
                    .mouse_hits
                    .iter()
                    .any(|(_, target)| *target == MouseTarget::Queue(0))
            );
            assert!(
                render
                    .mouse_hits
                    .iter()
                    .any(|(_, target)| *target == MouseTarget::Queue(2))
            );
            assert!(
                !render
                    .mouse_hits
                    .iter()
                    .any(|(_, target)| *target == MouseTarget::Queue(1))
            );
            for (rect, _) in &render.mouse_hits {
                assert!(rect.right() <= width && rect.bottom() <= height);
            }
            assert_eq!(app.queue.order, before);
        }
    }
}

#[test]
fn queue_filter_long_unicode_and_no_matches_keep_recovery_controls_visible() {
    for (width, height) in [(32, 10), (80, 24), (120, 35)] {
        let mut app = fixture();
        app.ui.queue.query = "界é".repeat(50);
        app.ui.queue.editing = true;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let text = screen(&terminal);
        assert!(text.contains("0/4"), "{text}");
        assert!(text.contains("Esc Clear"), "{text}");
        assert!(text.contains("No matching tracks."), "{text}");
        assert!(
            !app.ui
                .render
                .borrow()
                .mouse_hits
                .iter()
                .any(|(_, target)| matches!(target, MouseTarget::Queue(_)))
        );
        app.cache.remove(&format!("{:022}", 1));
        terminal
            .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let text = screen(&terminal);
        assert!(text.contains("No matches in loaded info."), "{text}");
        if width >= 80 {
            assert!(text.contains("1 tracks without info"), "{text}");
        }
    }
}

#[test]
fn queue_filter_does_not_hide_tracks_in_the_side_queue() {
    let mut app = fixture();
    app.catalog.view = View::Liked;
    app.ui.queue.query = "no matches".into();
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    assert!(screen(&terminal).contains("Daylight"));
    assert!(
        app.ui
            .render
            .borrow()
            .mouse_hits
            .iter()
            .any(|(_, target)| *target == MouseTarget::Queue(1))
    );
    assert!(
        !app.ui
            .render
            .borrow()
            .mouse_hits
            .iter()
            .any(|(_, target)| *target == MouseTarget::QueueFilter)
    );
}
