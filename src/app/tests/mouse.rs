use super::*;

fn assert_mix_hit_text(
    buffer: &ratatui::buffer::Buffer,
    area: ratatui::layout::Rect,
    code: KeyCode,
) {
    let labels: &[&str] = match code {
        KeyCode::Char('p') => &["p pin"],
        KeyCode::Char('g') => &["g regenerate", "g regen"],
        KeyCode::Char('a') => &["a artist gap", "a gap"],
        KeyCode::Char('w') => &["w save"],
        KeyCode::Char('o') => &["o reopen"],
        KeyCode::Char('?') => &["? close details", "? back", "? details"],
        KeyCode::Esc => &["Esc keep name", "Esc close details", "Esc cancel"],
        KeyCode::Enter => &["Enter save"],
        KeyCode::Char('3') => &["3"],
        KeyCode::Char('4') => &["4"],
        KeyCode::Char('6') => &["6"],
        KeyCode::Char('[') => &["["],
        KeyCode::Char(']') => &["]"],
        _ => panic!("Unexpected mouse control {code:?}"),
    };
    let text: String = (area.x..area.right())
        .map(|x| buffer[(x, area.y)].symbol())
        .collect();
    assert!(
        !text.trim().is_empty() && labels.iter().any(|label| label.contains(&text)),
        "{code:?} at {area:?}: {text:?}"
    );
}

#[test]
fn mix_control_hit_positions_match_visible_glyphs_after_wrapping() {
    let mut app = crate::demo::app();
    app.open_queue_mix();
    for (width, height) in [(80, 24), (60, 18), (48, 18), (34, 15), (32, 10)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| ui::draw(frame, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let render = app.ui.render.borrow();
        let mut count = 0;
        for (area, target) in &render.mouse_hits {
            if let MouseTarget::MixKey(code) = target {
                assert_mix_hit_text(terminal.backend().buffer(), *area, *code);
                count += 1;
            }
        }
        assert!(count >= 3, "expected visible controls at {width}x{height}");
    }
    app.mix.detail = true;
    for (width, height) in [(80, 24), (32, 10)] {
        let mut detail_hits = 0;
        for scroll in 0..40 {
            app.mix.detail_scroll = scroll;
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| ui::draw(frame, &app, &mut app.ui.render.borrow_mut()))
                .unwrap();
            for (area, target) in &app.ui.render.borrow().mouse_hits {
                if let MouseTarget::MixKey(code) = target {
                    assert_mix_hit_text(terminal.backend().buffer(), *area, *code);
                    detail_hits += 1;
                }
            }
        }
        assert!(detail_hits > 0, "no detail controls at {width}x{height}");
    }
}

#[tokio::test]
async fn mix_mouse_normal_layout_changes_preview_without_applying_queue() {
    let mut app = crate::demo::app();
    app.open_queue_mix();
    assert!(app.mix.preview.entries.len() > 2);
    let queue = app.queue.ids.clone();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();

    draw_mouse(&app, 80, 24);
    for target in [
        MouseTarget::MixRow(1),
        MouseTarget::MixKey(KeyCode::Char('p')),
        MouseTarget::MixKey(KeyCode::Char('3')),
        MouseTarget::MixKey(KeyCode::Char('[')),
        MouseTarget::MixKey(KeyCode::Char('a')),
        MouseTarget::MixKey(KeyCode::Char('g')),
        MouseTarget::MixKey(KeyCode::Char('w')),
        MouseTarget::MixKey(KeyCode::Char('o')),
        MouseTarget::MixKey(KeyCode::Char('?')),
        MouseTarget::MixKey(KeyCode::Esc),
    ] {
        assert!(
            app.ui
                .render
                .borrow()
                .mouse_hits
                .iter()
                .any(|(_, hit)| *hit == target),
            "missing {target:?}"
        );
    }
    assert!(
        !app.ui
            .render
            .borrow()
            .mouse_hits
            .iter()
            .any(|(_, hit)| matches!(
                hit,
                MouseTarget::MixKey(KeyCode::Enter | KeyCode::Char('A'))
            ))
    );

    click_target(
        &mut app,
        MouseTarget::MixRow(1),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.mix.selected, 1);
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Char('p')),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(app.mix.preview.entries[1].pinned);
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Char('3')),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.mix.settings.target_minutes, 30);
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Char('[')),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.mix.settings.recommendation_percent, 15);
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Char('a')),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.mix.settings.artist_gap, 3);
    draw_mouse(&app, 80, 24);
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Char('g')),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.queue.ids, queue);
    assert!(rx.try_recv().is_err());

    draw_mouse(&app, 80, 24);
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Char('?')),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(app.mix.detail);
    draw_mouse(&app, 80, 24);
    assert!(
        !app.ui
            .render
            .borrow()
            .mouse_hits
            .iter()
            .any(|(_, hit)| matches!(
                hit,
                MouseTarget::MixRow(_) | MouseTarget::MixKey(KeyCode::Enter | KeyCode::Char('A'))
            ))
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    draw_mouse(&app, 80, 24);
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Esc),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.overlay, Overlay::None);
    assert_eq!(app.queue.ids, queue);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn mix_wrapped_details_expose_every_preview_control() {
    let mut app = crate::demo::app();
    app.open_queue_mix();
    app.mix.detail = true;
    app.mix.source_error = Some("p pin • g regenerate • ? details".into());
    app.mix.preview.entries[0].track.name = "日本語 p pin • w save".into();
    let queue = app.queue.ids.clone();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    for code in [
        KeyCode::Char('3'),
        KeyCode::Char('4'),
        KeyCode::Char('6'),
        KeyCode::Char('['),
        KeyCode::Char(']'),
        KeyCode::Char('a'),
        KeyCode::Char('p'),
        KeyCode::Char('g'),
        KeyCode::Char('w'),
        KeyCode::Char('o'),
        KeyCode::Char('?'),
        KeyCode::Esc,
    ] {
        let mut found = false;
        for scroll in 0..80 {
            app.mix.detail_scroll = scroll;
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(32, 10)).unwrap();
            terminal
                .draw(|frame| ui::draw(frame, &app, &mut app.ui.render.borrow_mut()))
                .unwrap();
            for (area, target) in &app.ui.render.borrow().mouse_hits {
                if *target == MouseTarget::MixKey(code) {
                    assert_mix_hit_text(terminal.backend().buffer(), *area, code);
                    found = true;
                }
            }
            if found {
                break;
            }
        }
        assert!(found, "Wrapped control missing: {code:?}");
        if code == KeyCode::Char('p') {
            click_target(
                &mut app,
                MouseTarget::MixKey(code),
                MouseButton::Left,
                &mut tasks,
                &tx,
            );
            assert!(app.mix.preview.entries[0].pinned);
        }
    }
    // Action-looking metadata appears near the top, outside the control section.
    app.mix.detail_scroll = 0;
    draw_mouse(&app, 80, 40);
    assert_eq!(
        app.ui
            .render
            .borrow()
            .mouse_hits
            .iter()
            .filter(|(_, target)| *target == MouseTarget::MixKey(KeyCode::Char('p')))
            .count(),
        1
    );
    assert_eq!(app.queue.ids, queue);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn long_mix_recipe_names_keep_only_save_and_cancel_clickable() {
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    for (width, height) in [(32, 10), (34, 15), (48, 18), (80, 24)] {
        let mut app = crate::demo::app();
        app.open_queue_mix();
        app.mix.naming = true;
        let name: String = "日本語 p pin • ? details • w save "
            .repeat(4)
            .chars()
            .take(80)
            .collect();
        app.mix.recipe_name = name.clone();
        draw_mouse(&app, width, height);
        let render = app.ui.render.borrow();
        for code in [KeyCode::Enter, KeyCode::Esc] {
            assert!(
                render
                    .mouse_hits
                    .iter()
                    .any(|(_, target)| *target == MouseTarget::MixKey(code)),
                "{width}x{height}: missing {code:?}"
            );
        }
        assert!(
            !render
                .mouse_hits
                .iter()
                .any(|(_, target)| matches!(target, MouseTarget::MixKey(KeyCode::Char(_))))
        );
        drop(render);
        click_target(
            &mut app,
            MouseTarget::MixKey(KeyCode::Esc),
            MouseButton::Left,
            &mut tasks,
            &tx,
        );
        assert!(!app.mix.naming);
        assert_eq!(app.mix.recipe_name, name);
        assert!(rx.try_recv().is_err());
    }
}

#[tokio::test]
async fn long_statistics_queries_keep_sort_clear_and_done_clickable() {
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    for (width, height) in [(32, 10), (34, 15), (48, 18), (80, 24)] {
        let mut app = App::new(Config::default(), Queue::default());
        app.stats.add_play(&"1".repeat(22), "Song", "Artist");
        app.ui.overlay = Overlay::Stats;
        let query = "日本語 👩‍💻 ".repeat(10);
        app.ui.stats.get_mut().query = query.clone();
        draw_mouse(&app, width, height);
        for target in [MouseTarget::StatsSort, MouseTarget::StatsClear] {
            assert!(
                app.ui
                    .render
                    .borrow()
                    .mouse_hits
                    .iter()
                    .any(|(area, hit)| *hit == target && area.width > 0),
                "{width}x{height}: missing {target:?}"
            );
        }
        click_target(
            &mut app,
            MouseTarget::StatsSort,
            MouseButton::Left,
            &mut tasks,
            &tx,
        );
        assert_eq!(
            app.ui.stats.borrow().sort,
            crate::app::ui_state::StatsSort::Time
        );
        assert_eq!(app.ui.stats.borrow().query, query);
        app.ui.stats.get_mut().editing = true;
        draw_mouse(&app, width, height);
        click_target(
            &mut app,
            MouseTarget::StatsSearchDone,
            MouseButton::Left,
            &mut tasks,
            &tx,
        );
        assert!(!app.ui.stats.borrow().editing);
        assert_eq!(app.ui.stats.borrow().query, query);
        draw_mouse(&app, width, height);
        click_target(
            &mut app,
            MouseTarget::StatsClear,
            MouseButton::Left,
            &mut tasks,
            &tx,
        );
        assert!(app.ui.stats.borrow().query.is_empty());
        assert_eq!(app.ui.overlay, Overlay::Stats);
    }
}

#[tokio::test]
async fn mix_mouse_compact_layout_only_exposes_drawn_preview_controls() {
    let mut app = crate::demo::app();
    app.open_queue_mix();
    let queue = app.queue.ids.clone();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    draw_mouse(&app, 32, 10);
    let render = app.ui.render.borrow();
    let hits = &render.mouse_hits;
    for code in [KeyCode::Char('p'), KeyCode::Char('?'), KeyCode::Esc] {
        assert!(
            hits.iter()
                .any(|(_, hit)| *hit == MouseTarget::MixKey(code))
        );
    }
    assert!(!hits.iter().any(|(_, hit)| matches!(
        hit,
        MouseTarget::MixRow(_)
            | MouseTarget::MixKey(KeyCode::Enter | KeyCode::Char('A' | 'g' | '3'))
    )));
    let pin_area = hits
        .iter()
        .find(|(_, hit)| *hit == MouseTarget::MixKey(KeyCode::Char('p')))
        .unwrap()
        .0;
    assert!(pin_area.right() <= 32 && pin_area.bottom() <= 10);
    drop(render);

    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Char('p')),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    assert!(!app.mix.preview.entries[0].pinned);
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Char('p')),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(app.mix.preview.entries[0].pinned);
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Char('?')),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(app.mix.detail);
    draw_mouse(&app, 32, 10);
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Char('?')),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(!app.mix.detail);
    draw_mouse(&app, 32, 10);
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Esc),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.overlay, Overlay::None);
    assert_eq!(app.queue.ids, queue);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn mix_mouse_naming_and_scrolled_details_use_visible_actions() {
    let mut app = crate::demo::app();
    app.open_queue_mix();
    let queue = app.queue.ids.clone();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();

    draw_mouse(&app, 80, 24);
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Char('w')),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(app.mix.naming);
    draw_mouse(&app, 80, 24);
    assert!(
        app.ui
            .render
            .borrow()
            .mouse_hits
            .iter()
            .any(|(_, hit)| *hit == MouseTarget::MixKey(KeyCode::Enter))
    );
    assert!(
        !app.ui
            .render
            .borrow()
            .mouse_hits
            .iter()
            .any(|(_, hit)| matches!(hit, MouseTarget::MixRow(_)))
    );
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Enter),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(!app.mix.naming);
    assert!(!app.mix_recipes.recipes.is_empty());
    assert_eq!(app.queue.ids, queue);

    draw_mouse(&app, 80, 24);
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Char('?')),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(app.mix.detail);
    let mut found = false;
    for scroll in 0..40 {
        app.mix.detail_scroll = scroll;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| ui::draw(frame, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let render = app.ui.render.borrow();
        if let Some((area, _)) = render
            .mouse_hits
            .iter()
            .find(|(_, hit)| *hit == MouseTarget::MixKey(KeyCode::Char('?')))
        {
            assert_eq!(terminal.backend().buffer()[(area.x, area.y)].symbol(), "?");
            found = true;
        }
        drop(render);
        if found {
            break;
        }
    }
    assert!(
        found,
        "scrolled details should expose a visible close control"
    );
    click_target(
        &mut app,
        MouseTarget::MixKey(KeyCode::Char('?')),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(!app.mix.detail);
    assert_eq!(app.queue.ids, queue);
    assert!(rx.try_recv().is_err());
}

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
        assert_eq!(hits.len(), 6);
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

#[tokio::test]
async fn stats_mouse_rows_search_sort_and_footer_use_drawn_hit_regions() {
    let mut app = App::new(Config::default(), Queue::default());
    let first_id = "1".repeat(22);
    let second_id = "2".repeat(22);
    app.stats.add_play(&first_id, "Zulu Song", "Artist One");
    app.stats.add_play(&first_id, "Zulu Song", "Artist One");
    app.stats.add_play(&second_id, "Alpha Song", "Artist Two");
    app.stats.tracks.get_mut(&first_id).unwrap().listened_ms = 10_000;
    app.stats.tracks.get_mut(&second_id).unwrap().listened_ms = 50_000;
    app.ui.overlay = Overlay::Stats;

    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    draw_mouse(&app, 100, 30);
    assert!(
        app.ui
            .render
            .borrow()
            .mouse_hits
            .iter()
            .any(|(_, target)| *target == MouseTarget::StatsRow(1))
    );

    click_target(
        &mut app,
        MouseTarget::StatsRow(1),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.stats.borrow().selected, 1);
    assert!(app.queue.ids.is_empty());

    draw_mouse(&app, 100, 30);
    click_target(
        &mut app,
        MouseTarget::StatsSort,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(
        app.ui.stats.borrow().sort,
        crate::app::ui_state::StatsSort::Time
    );
    assert_eq!(app.ui.stats.borrow().selected, 0);
    draw_mouse(&app, 100, 30);
    assert_eq!(app.ui.stats.borrow().rows[0].name, "Alpha Song");

    click_target(
        &mut app,
        MouseTarget::StatsSearch,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(app.ui.stats.borrow().editing);
    for character in "Alpha".chars() {
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
    }
    draw_mouse(&app, 100, 30);
    click_target(
        &mut app,
        MouseTarget::StatsSearchDone,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(!app.ui.stats.borrow().editing);
    draw_mouse(&app, 100, 30);
    assert_eq!(app.ui.stats.borrow().rows.len(), 1);
    assert_eq!(app.ui.stats.borrow().rows[0].name, "Alpha Song");

    click_target(
        &mut app,
        MouseTarget::StatsClear,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(app.ui.stats.borrow().query.is_empty());
    assert_eq!(app.ui.overlay, Overlay::Stats);
    draw_mouse(&app, 100, 30);
    click_target(
        &mut app,
        MouseTarget::StatsClose,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.overlay, Overlay::None);
}

#[tokio::test]
async fn stats_mouse_wheel_navigates_and_overlay_consumes_underlying_clicks() {
    let mut app = App::new(Config::default(), Queue::default());
    for i in 0..30 {
        app.stats
            .add_play(&format!("{i:022}"), &format!("Song {i}"), "Artist");
    }
    app.catalog.view = View::Search;
    app.catalog.selected = 7;
    app.ui.overlay = Overlay::Stats;
    app.loaded = true;
    app.state = State::Playing;

    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    draw_mouse(&app, 120, 35);
    let row = app
        .ui
        .render
        .borrow()
        .mouse_hits
        .iter()
        .find(|(_, target)| *target == MouseTarget::StatsRow(0))
        .map(|(area, _)| *area)
        .expect("stats renderer should register its visible rows");
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
    assert_eq!(app.ui.stats.borrow().selected, 3);
    assert_eq!(app.catalog.selected, 7);

    for target in [MouseTarget::Navigation(View::Liked), MouseTarget::PlayPause] {
        draw_mouse(&app, 120, 35);
        click_target(&mut app, target, MouseButton::Left, &mut tasks, &tx);
        assert_eq!(app.catalog.view, View::Search);
        assert_eq!(app.state, State::Playing);
        assert!(rx.try_recv().is_err());
    }
}

#[tokio::test]
async fn stats_title_exit_hit_closes_empty_narrow_and_filtered_overlays() {
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    for (width, height) in [(100, 30), (34, 15)] {
        let mut app = App::new(Config::default(), Queue::default());
        app.ui.overlay = Overlay::Stats;
        draw_mouse(&app, width, height);
        let close = app
            .ui
            .render
            .borrow()
            .mouse_hits
            .iter()
            .find(|(_, target)| *target == MouseTarget::StatsClose)
            .map(|(area, _)| *area)
            .expect("stats title should expose its visible exit text");
        assert!(close.width > 0 && close.height > 0);
        assert!(close.right() <= width && close.bottom() <= height);
        click_target(
            &mut app,
            MouseTarget::StatsClose,
            MouseButton::Left,
            &mut tasks,
            &tx,
        );
        assert_eq!(app.ui.overlay, Overlay::None);
    }

    let mut app = App::new(Config::default(), Queue::default());
    app.stats.add_play(&"1".repeat(22), "Song", "Artist");
    app.ui.overlay = Overlay::Stats;
    app.ui.stats.get_mut().query = "Song".into();
    draw_mouse(&app, 100, 30);
    click_target(
        &mut app,
        MouseTarget::StatsClose,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.overlay, Overlay::None);
    assert_eq!(app.ui.stats.borrow().query, "Song");
}

#[test]
fn seek_hitbox_matches_visible_progress_bar_in_tall_layout() {
    let app = App::new(Config::default(), Queue::default());

    draw_mouse(&app, 105, 100);

    let seek = app
        .ui
        .render
        .borrow()
        .mouse_hits
        .iter()
        .find(|(_, target)| *target == MouseTarget::Seek)
        .map(|(area, _)| *area)
        .expect("wide playback layout should render a seek bar");

    assert!(
        seek.x > 1,
        "timestamp area to the left of the visible bar must not be seekable: {seek:?}"
    );
    assert_eq!(seek.height, 1);
}

#[tokio::test]
async fn right_click_context_menu_catalog_track_displays_view_album_and_artist_and_navigates() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Search;
    let track = Track {
        id: "0000000000000000000001".into(),
        name: "Test Song".into(),
        artists: "Test Artist".into(),
        artist_ids: vec!["0000000000000000000500".into()],
        duration_ms: 200_000,
        playable: true,
        album: Some("Test Album".into()),
        album_id: Some("8000000000000000000001".into()),
        album_art_url: None,
        track_number: Some(1),
    };
    app.catalog.rows = Rows::Tracks(vec![track]);
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    let menu = app.context_menu.as_ref().unwrap();
    let labels: Vec<&str> = menu.labels().collect();
    assert_eq!(
        labels,
        vec![
            "Play",
            "Add to queue",
            "Play next",
            "View Album",
            "View Artist"
        ]
    );

    // Click "View Album" (index 3)
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Menu(3),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(
        app.catalog.browse,
        Browse::Album("8000000000000000000001".into())
    );
    assert_eq!(app.catalog.title, "Test Album");
    assert!(app.context_menu.is_none());

    // Pop navigation back to Search
    assert!(app.pop_navigation());
    assert_eq!(app.catalog.view, View::Search);

    // Right-click again and click "View Artist" (index 4)
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Menu(4),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Artist);
    assert_eq!(
        app.catalog.browse,
        Browse::Artist("0000000000000000000500".into())
    );
    assert_eq!(app.catalog.title, "Test Artist");
    assert!(app.context_menu.is_none());
}

#[tokio::test]
async fn right_click_context_menu_queue_track_displays_view_album_and_artist_and_remove_works() {
    let mut app = App::new(Config::default(), Queue::default());
    let track1 = Track {
        id: "0000000000000000000001".into(),
        name: "Queue Track 1".into(),
        artists: "Artist Alpha".into(),
        artist_ids: vec!["0000000000000000000501".into()],
        duration_ms: 210_000,
        playable: true,
        album: Some("Alpha Album".into()),
        album_id: Some("8000000000000000000002".into()),
        album_art_url: None,
        track_number: Some(2),
    };
    let track2 = Track {
        id: "0000000000000000000002".into(),
        name: "Queue Track 2".into(),
        artists: "Artist Beta".into(),
        artist_ids: vec!["0000000000000000000502".into()],
        duration_ms: 190_000,
        playable: true,
        album: Some("Beta Album".into()),
        album_id: Some("8000000000000000000003".into()),
        album_art_url: None,
        track_number: Some(3),
    };
    app.cache.insert(track1.id.clone(), track1.clone());
    app.cache.insert(track2.id.clone(), track2.clone());
    app.queue.enqueue(track1.id.clone());
    app.queue.enqueue(track2.id.clone());
    app.catalog.view = View::Queue;

    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Queue(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    let menu = app.context_menu.as_ref().unwrap();
    let labels: Vec<&str> = menu.labels().collect();
    assert_eq!(
        labels,
        vec![
            "Play",
            "Add to queue",
            "Play next",
            "Remove from queue",
            "View Album",
            "View Artist",
        ]
    );

    // Click "View Album" (index 4)
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Menu(4),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(
        app.catalog.browse,
        Browse::Album("8000000000000000000002".into())
    );
    assert_eq!(app.catalog.title, "Alpha Album");

    // Pop back to Queue
    assert!(app.pop_navigation());
    assert_eq!(app.catalog.view, View::Queue);

    // Right-click and click "View Artist" (index 5)
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Queue(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Menu(5),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Artist);
    assert_eq!(
        app.catalog.browse,
        Browse::Artist("0000000000000000000501".into())
    );
    assert_eq!(app.catalog.title, "Artist Alpha");

    // Pop back to Queue
    assert!(app.pop_navigation());
    assert_eq!(app.catalog.view, View::Queue);

    // Verify "Remove from queue" (index 3) still works
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Queue(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Menu(3),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.queue.ids.len(), 1);
    assert_eq!(app.queue.ids[0], track2.id);
}

#[tokio::test]
async fn adversarial_right_click_non_track_items_and_edge_targets() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    let click_unhandled = |app: &mut App,
                           target: MouseTarget,
                           tasks: &mut Tasks,
                           tx: &mpsc::UnboundedSender<Command>| {
        let area = app
            .ui
            .render
            .borrow()
            .mouse_hits
            .iter()
            .find(|(_, hit)| *hit == target)
            .unwrap()
            .0;
        let handled = mouse(
            app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: area.x,
                row: area.y,
                modifiers: KeyModifiers::NONE,
            },
            tasks,
            tx,
        );
        assert!(!handled);
        assert!(app.context_menu.is_none());
    };

    // 1. Right click on Search mode buttons (Spotify / Library) in View::Search
    app.catalog.view = View::Search;
    draw_mouse(&app, 120, 35);
    click_unhandled(
        &mut app,
        MouseTarget::SearchMode(SearchScope::Library),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.search_scope, SearchScope::Spotify);

    // 2. Right click on Search prompt bar
    draw_mouse(&app, 120, 35);
    click_unhandled(&mut app, MouseTarget::Prompt, &mut tasks, &tx);
    assert!(!app.catalog.editing);

    // 3. Right click on Navigation buttons
    draw_mouse(&app, 120, 35);
    click_unhandled(
        &mut app,
        MouseTarget::Navigation(View::Liked),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Search);

    // 4. Right click on PlayPause control
    draw_mouse(&app, 120, 35);
    click_unhandled(&mut app, MouseTarget::PlayPause, &mut tasks, &tx);
    assert_eq!(app.state, State::Paused);

    // 5. Right click on Seek bar
    draw_mouse(&app, 120, 35);
    click_unhandled(&mut app, MouseTarget::Seek, &mut tasks, &tx);

    // 6. Right click on CatalogScroll
    draw_mouse(&app, 120, 35);
    click_unhandled(&mut app, MouseTarget::CatalogScroll, &mut tasks, &tx);

    // 7. Right click on QueueScroll when app.can_undo() is false
    app.catalog.view = View::Queue;
    draw_mouse(&app, 120, 35);
    assert!(!app.can_undo());
    click_unhandled(&mut app, MouseTarget::QueueScroll, &mut tasks, &tx);

    // 8. Right click on QueueScroll when app.can_undo() is true
    app.remember_queue();
    assert!(app.can_undo());
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::QueueScroll,
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    assert!(app.context_menu.is_some());
    let menu = app.context_menu.as_ref().unwrap();
    let labels: Vec<&str> = menu.labels().collect();
    assert_eq!(labels, vec!["Undo queue change"]);

    // Dismiss menu via Esc
    key(
        &mut app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(app.context_menu.is_none());

    // 9. Coordinate with no hit target (e.g. 0, 0)
    let handled = mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        &mut tasks,
        &tx,
    );
    assert!(!handled);
    assert!(app.context_menu.is_none());
}

#[tokio::test]
async fn adversarial_context_menu_rapid_toggle_dismiss_and_keyboard_interaction() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.rows = Rows::Tracks(vec![test_track(0), test_track(1), test_track(2)]);
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    // 1. Open context menu on track 0
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    assert!(app.context_menu.is_some());
    assert_eq!(app.catalog.selected, 0);

    // 2. Click outside at (0, 0) dismisses menu immediately
    assert!(mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        &mut tasks,
        &tx,
    ));
    assert!(app.context_menu.is_none());

    // 3. Open context menu on track 0, then right-click on track 1
    // First click closes existing menu
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    assert!(app.context_menu.is_some());
    // A right click outside menu items dismisses the open menu
    assert!(mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        &mut tasks,
        &tx,
    ));
    assert!(app.context_menu.is_none());

    // Second right-click opens track 1's menu
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(1),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    assert!(app.context_menu.is_some());
    assert_eq!(app.catalog.selected, 1);

    // 4. Keyboard arrow navigation inside context menu
    assert_eq!(app.context_menu.as_ref().unwrap().selected, 0);
    key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.context_menu.as_ref().unwrap().selected, 1);
    key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.context_menu.as_ref().unwrap().selected, 2);
    key(
        &mut app,
        KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.context_menu.as_ref().unwrap().selected, 1);

    // Press Enter to activate selected action ("Add to queue")
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(app.context_menu.is_none());
    assert_eq!(app.queue.ids.len(), 1);
    assert_eq!(app.queue.ids[0], test_track(1).id);

    // 5. Rapid open / dismiss stress loop (200 cycles)
    for i in 0..200 {
        draw_mouse(&app, 120, 35);
        click_target(
            &mut app,
            MouseTarget::Catalog(i % 3),
            MouseButton::Right,
            &mut tasks,
            &tx,
        );
        assert!(app.context_menu.is_some());
        if i % 2 == 0 {
            key(
                &mut app,
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                &mut tasks,
                &tx,
            );
        } else {
            assert!(mouse(
                &mut app,
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: 0,
                    row: 0,
                    modifiers: KeyModifiers::NONE,
                },
                &mut tasks,
                &tx,
            ));
        }
        assert!(app.context_menu.is_none());
    }
    assert!(!app.quit);
}

#[tokio::test]
async fn adversarial_context_menu_filtered_vs_unfiltered_and_stale_rejection() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Liked;

    let track_rock = Track {
        id: "0000000000000000000001".into(),
        name: "Rock Anthem".into(),
        artists: "Rock Band".into(),
        artist_ids: vec!["0000000000000000000501".into()],
        duration_ms: 200_000,
        playable: true,
        album: Some("Rock Album".into()),
        album_id: Some("8000000000000000000001".into()),
        album_art_url: None,
        track_number: Some(1),
    };
    let track_jazz1 = Track {
        id: "0000000000000000000002".into(),
        name: "Jazz Ballad".into(),
        artists: "Jazz Quartet".into(),
        artist_ids: vec!["0000000000000000000502".into()],
        duration_ms: 220_000,
        playable: true,
        album: Some("Jazz Album One".into()),
        album_id: Some("8000000000000000000002".into()),
        album_art_url: None,
        track_number: Some(2),
    };
    let track_pop = Track {
        id: "0000000000000000000003".into(),
        name: "Pop Hit".into(),
        artists: "Pop Star".into(),
        artist_ids: vec!["0000000000000000000503".into()],
        duration_ms: 180_000,
        playable: true,
        album: Some("Pop Album".into()),
        album_id: Some("8000000000000000000003".into()),
        album_art_url: None,
        track_number: Some(3),
    };
    let track_jazz2 = Track {
        id: "0000000000000000000004".into(),
        name: "Jazz Fusion".into(),
        artists: "Jazz Trio".into(),
        artist_ids: vec!["0000000000000000000504".into()],
        duration_ms: 240_000,
        playable: true,
        album: Some("Jazz Album Two".into()),
        album_id: Some("8000000000000000000004".into()),
        album_art_url: None,
        track_number: Some(4),
    };

    app.catalog.rows = Rows::Tracks(vec![
        track_rock.clone(),
        track_jazz1.clone(),
        track_pop.clone(),
        track_jazz2.clone(),
    ]);

    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    // 1. Unfiltered test: click track 2 ("Pop Hit") -> View Album
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(2),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Menu(3), // "View Album"
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(
        app.catalog.browse,
        Browse::Album("8000000000000000000003".into())
    );
    assert_eq!(app.catalog.title, "Pop Album");

    // Pop back to Liked
    assert!(app.pop_navigation());
    assert_eq!(app.catalog.view, View::Liked);
    assert_eq!(app.catalog.selected, 2);

    // 2. Filtered test: Filter by "Jazz" -> matches track_jazz1 (index 1) and track_jazz2 (index 3)
    app.catalog.filter = "Jazz".into();
    draw_mouse(&app, 120, 35);

    // Visual row 1 in filtered catalog corresponds to track_jazz2!
    click_target(
        &mut app,
        MouseTarget::Catalog(1),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    assert!(app.context_menu.is_some());
    assert_eq!(app.context_menu.as_ref().unwrap().row, 1);
    assert_eq!(app.context_menu.as_ref().unwrap().filter, "Jazz");

    // Activate "View Album" on filtered row 1
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Menu(3),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );

    // Must navigate to track_jazz2's album ("Jazz Album Two"), NOT track_rock or track_jazz1!
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(
        app.catalog.browse,
        Browse::Album("8000000000000000000004".into())
    );
    assert_eq!(app.catalog.title, "Jazz Album Two");

    // Pop navigation back to Liked: filter and selection must be faithfully restored
    assert!(app.pop_navigation());
    assert_eq!(app.catalog.view, View::Liked);
    assert_eq!(app.catalog.filter, "Jazz");
    assert_eq!(app.catalog.selected, 1);

    // 3. Filtered test: Activate "View Artist" on filtered row 0 (track_jazz1)
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Menu(4), // "View Artist"
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Artist);
    assert_eq!(
        app.catalog.browse,
        Browse::Artist("0000000000000000000502".into())
    );
    assert_eq!(app.catalog.title, "Jazz Quartet");

    assert!(app.pop_navigation());
    assert_eq!(app.catalog.view, View::Liked);
    assert_eq!(app.catalog.filter, "Jazz");

    // 4. Stale rejection: filter changes while menu is open
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    assert!(app.context_menu.is_some());
    // Externally mutate filter before menu activation
    app.catalog.filter = "Rock".into();
    activate_menu(&mut app, 3, &mut tasks, &tx);
    assert!(app.status.contains("List changed"));
    assert_eq!(app.catalog.view, View::Liked); // Did NOT navigate

    // 5. Stale rejection: rows revision changes while menu is open
    app.catalog.filter = "".into();
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    assert!(app.context_menu.is_some());
    app.catalog.rows_revision = app.catalog.rows_revision.wrapping_add(1);
    activate_menu(&mut app, 3, &mut tasks, &tx);
    assert!(app.status.contains("List changed"));
    assert_eq!(app.catalog.view, View::Liked);
}

#[tokio::test]
async fn adversarial_context_menu_missing_metadata_and_playlist_rows() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    // 1. Track without album metadata
    let no_album_track = Track {
        id: "0000000000000000000010".into(),
        name: "Single Track".into(),
        artists: "Solo Artist".into(),
        artist_ids: vec!["0000000000000000000510".into()],
        duration_ms: 180_000,
        playable: true,
        album: None,
        album_id: None,
        album_art_url: None,
        track_number: None,
    };
    // 2. Track without artist metadata
    let no_artist_track = Track {
        id: "0000000000000000000011".into(),
        name: "Unknown Artist Track".into(),
        artists: "".into(),
        artist_ids: vec![],
        duration_ms: 180_000,
        playable: true,
        album: Some("Some Album".into()),
        album_id: Some("8000000000000000000011".into()),
        album_art_url: None,
        track_number: Some(1),
    };

    app.catalog.rows = Rows::Tracks(vec![no_album_track, no_artist_track]);

    // Right-click track 0 -> "View Album"
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Menu(3),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Search); // Did not navigate
    assert_eq!(app.status, "No album information available for this track.");
    assert!(app.catalog.history.is_empty());

    // Right-click track 1 -> "View Artist"
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(1),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Menu(4),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Search); // Did not navigate
    assert_eq!(
        app.status,
        "No artist information available for this track."
    );
    assert!(app.catalog.history.is_empty());

    // 3. Playlists view: Right-clicking a playlist row must NOT show View Album / View Artist
    app.catalog.view = View::Playlists;
    app.catalog.rows = Rows::Playlists(vec![crate::model::Playlist {
        id: "playlist000000000001".into(),
        name: "My Chill Mix".into(),
        owner: "user1".into(),
    }]);
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    let menu = app.context_menu.as_ref().unwrap();
    let labels: Vec<&str> = menu.labels().collect();
    assert_eq!(labels, vec!["Open playlist", "Add playlist to queue"]);
    assert!(!labels.contains(&"View Album"));
    assert!(!labels.contains(&"View Artist"));
}

#[tokio::test]
async fn adversarial_search_input_resolution_table() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    let valid_album_cases = [
        (
            "spotify:album:6kZDoAmRSvGQvun5LzyZhk",
            "6kZDoAmRSvGQvun5LzyZhk",
        ),
        (
            "SPOTIFY:ALBUM:6kZDoAmRSvGQvun5LzyZhk",
            "6kZDoAmRSvGQvun5LzyZhk",
        ),
        (
            "https://open.spotify.com/album/6kZDoAmRSvGQvun5LzyZhk",
            "6kZDoAmRSvGQvun5LzyZhk",
        ),
        (
            "http://open.spotify.com/album/6kZDoAmRSvGQvun5LzyZhk",
            "6kZDoAmRSvGQvun5LzyZhk",
        ),
        (
            "open.spotify.com/album/6kZDoAmRSvGQvun5LzyZhk",
            "6kZDoAmRSvGQvun5LzyZhk",
        ),
        (
            "https://open.spotify.com/intl-fr/album/6kZDoAmRSvGQvun5LzyZhk",
            "6kZDoAmRSvGQvun5LzyZhk",
        ),
        (
            "open.spotify.com/intl-pt-BR/album/6kZDoAmRSvGQvun5LzyZhk",
            "6kZDoAmRSvGQvun5LzyZhk",
        ),
        (
            "open.spotify.com/intl-es-419/album/6kZDoAmRSvGQvun5LzyZhk?si=xyz123&nd=1",
            "6kZDoAmRSvGQvun5LzyZhk",
        ),
        (
            "https://open.spotify.com/album/6kZDoAmRSvGQvun5LzyZhk/",
            "6kZDoAmRSvGQvun5LzyZhk",
        ),
        (
            "  https://open.spotify.com/album/6kZDoAmRSvGQvun5LzyZhk  ",
            "6kZDoAmRSvGQvun5LzyZhk",
        ),
        (
            "spotify:album:6kZDoAmRSvGQvun5LzyZhk?si=test123",
            "6kZDoAmRSvGQvun5LzyZhk",
        ),
        (
            "spotify:album:6kZDoAmRSvGQvun5LzyZhk#fragment",
            "6kZDoAmRSvGQvun5LzyZhk",
        ),
    ];

    for (query, expected_id) in valid_album_cases {
        app.catalog.view = View::Search;
        app.catalog.history.clear();
        app.catalog.editing = true;
        app.catalog.query = query.into();
        key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert!(!app.catalog.editing);
        assert_eq!(app.catalog.view, View::Album, "Failed for query: {query}");
        assert_eq!(
            app.catalog.browse,
            Browse::Album(expected_id.into()),
            "Failed for query: {query}"
        );
        assert_eq!(
            app.catalog.history.len(),
            1,
            "Expected history pushed for: {query}"
        );
        assert_eq!(app.catalog.history[0].breadcrumb, query.trim());
    }

    let valid_artist_cases = [
        (
            "spotify:artist:4Z8W4fKeB5YxbusRsdQVPb",
            "4Z8W4fKeB5YxbusRsdQVPb",
        ),
        (
            "SPOTIFY:ARTIST:4Z8W4fKeB5YxbusRsdQVPb",
            "4Z8W4fKeB5YxbusRsdQVPb",
        ),
        (
            "https://open.spotify.com/artist/4Z8W4fKeB5YxbusRsdQVPb",
            "4Z8W4fKeB5YxbusRsdQVPb",
        ),
        (
            "open.spotify.com/intl-de/artist/4Z8W4fKeB5YxbusRsdQVPb?si=xyz",
            "4Z8W4fKeB5YxbusRsdQVPb",
        ),
        (
            "https://open.spotify.com/artist/4Z8W4fKeB5YxbusRsdQVPb/",
            "4Z8W4fKeB5YxbusRsdQVPb",
        ),
        (
            "  spotify:artist:4Z8W4fKeB5YxbusRsdQVPb  ",
            "4Z8W4fKeB5YxbusRsdQVPb",
        ),
    ];

    for (query, expected_id) in valid_artist_cases {
        app.catalog.view = View::Search;
        app.catalog.history.clear();
        app.catalog.editing = true;
        app.catalog.query = query.into();
        key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert!(!app.catalog.editing);
        assert_eq!(app.catalog.view, View::Artist, "Failed for query: {query}");
        assert_eq!(
            app.catalog.browse,
            Browse::Artist(expected_id.into()),
            "Failed for query: {query}"
        );
        assert_eq!(
            app.catalog.history.len(),
            1,
            "Expected history pushed for: {query}"
        );
    }

    let adversarial_fallback_cases = [
        "spotify:album:invalid",
        "spotify:album:",
        "spotify:album",
        "spotify:artist:invalid",
        "spotify:album:toolong0000000000000000000001",
        "spotify:album:4cOdK2wGLETKBW3PvgPWq-",
        "spotify:album:4cOdK2wGLETKBW3PvgPWq!",
        "https://open.spotify.com/album/",
        "https://open.spotify.com/album/invalid",
        "https://open.spotify.com/artist/invalid",
        "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT",
        "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
        "https://evil.com/album/6kZDoAmRSvGQvun5LzyZhk",
        "https://open.spotify.com.evil.com/album/6kZDoAmRSvGQvun5LzyZhk",
        "https://notspotify.com/open.spotify.com/album/6kZDoAmRSvGQvun5LzyZhk",
        "album",
        "artist",
        "this is an album by artist",
        "Radiohead OK Computer album",
        "https://open.spotify.com/album/6kZDoAmRSvGQvun5LzyZhk extra text",
        "spotify:album:6kZDoAmRSvGQvun5LzyZhk extra",
        "   ",
        "",
        "\n\t  spotify:album:invalid  \r\n",
        "🎵 spotify:album:6kZDoAmRSvGQvun5LzyZhk 🎵",
    ];

    for query in adversarial_fallback_cases {
        app.catalog.view = View::Search;
        app.catalog.history.clear();
        app.catalog.editing = true;
        app.catalog.query = query.into();
        key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert!(!app.catalog.editing);
        assert_eq!(
            app.catalog.view,
            View::Search,
            "Expected View::Search fallback for adversarial query: {query:?}"
        );
        assert_eq!(
            app.catalog.browse,
            Browse::Search(query.trim().into()),
            "Expected Browse::Search for query: {query:?}"
        );
        assert!(
            app.catalog.history.is_empty(),
            "Expected NO history push for non-link query: {query:?}"
        );
    }
}

#[path = "demo.rs"]
mod demo;
