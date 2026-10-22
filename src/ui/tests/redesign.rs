use super::*;

#[test]
fn portrait_queue_keeps_artist_lines_and_transport_targets_visible() {
    for theme in [Theme::Spotify, Theme::Glass] {
        let mut app = crate::demo::app();
        app.config.theme = theme.as_str().into();
        app.config.native_glass = theme == Theme::Glass;
        app.catalog.view = View::Queue;
        for state in [State::Playing, State::Paused] {
            app.state = state;
            let mut terminal = Terminal::new(TestBackend::new(84, 80)).unwrap();
            terminal
                .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
                .unwrap();
            let render = app.ui.render.borrow();
            let row = render
                .mouse_hits
                .iter()
                .find(|(_, target)| *target == MouseTarget::Queue(0))
                .unwrap()
                .0;
            assert_eq!(row.height, 3);
            let artist: String = (row.x..row.right())
                .map(|x| terminal.backend().buffer()[(x, row.y + 1)].symbol())
                .collect();
            assert!(artist.contains("Mali & The Signals"), "{artist}");
            for target in [MouseTarget::PlayPause, MouseTarget::Seek] {
                let control = render
                    .mouse_hits
                    .iter()
                    .find(|(_, found)| *found == target)
                    .unwrap()
                    .0;
                assert!(!control.intersects(row));
                assert!(control.x >= row.right());
                if target == MouseTarget::PlayPause {
                    let text: String = (control.x..control.right())
                        .map(|x| terminal.backend().buffer()[(x, control.y)].symbol())
                        .collect();
                    assert!(text.contains("Space"));
                    assert!(text.contains(if state == State::Playing {
                        "Pause"
                    } else {
                        "Play"
                    }));
                }
            }
        }
    }
}

#[test]
fn queue_duration_refreshes_after_metadata_and_queue_edits_and_labels_partial_totals() {
    let mut app = crate::demo::app();
    app.catalog.view = View::Queue;
    let mut terminal = Terminal::new(TestBackend::new(140, 38)).unwrap();
    let render = |terminal: &mut Terminal<TestBackend>, app: &App| {
        terminal
            .draw(|frame| draw(frame, app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    };
    let original = app.cache.get(&app.queue.ids[0]).unwrap().clone();
    render(&mut terminal, &app);
    let before = app
        .ui
        .render
        .borrow()
        .queue_summary
        .as_ref()
        .unwrap()
        .duration_ms;
    let mut changed = original.clone();
    changed.duration_ms += 120_000;
    app.cache.insert(changed.id.clone(), changed);
    render(&mut terminal, &app);
    assert_eq!(
        app.ui
            .render
            .borrow()
            .queue_summary
            .as_ref()
            .unwrap()
            .duration_ms,
        before + 120_000
    );

    app.cache.remove(&original.id);
    let text = render(&mut terminal, &app);
    assert!(text.contains("min loaded"));
    assert_eq!(
        app.ui
            .render
            .borrow()
            .queue_summary
            .as_ref()
            .unwrap()
            .known_durations,
        app.queue.ids.len() - 1
    );

    app.queue.remove(0);
    let text = render(&mut terminal, &app);
    assert!(!text.contains("min loaded"));
    assert!(text.contains("8 tracks"));
    let summary = app.ui.render.borrow();
    assert_eq!(
        summary.queue_summary.as_ref().unwrap().duration_ms,
        before - u64::from(original.duration_ms)
    );
}

#[test]
fn queue_summary_expires_with_metadata_without_a_revision_or_prune() {
    let app = crate::demo::app();
    let mut render = RenderState::default();
    let revision = (app.queue.revision, app.cache.revision);
    let expires = app
        .queue
        .ids
        .iter()
        .filter_map(|id| app.cache.expires_at(id))
        .max()
        .unwrap();
    crate::ui::queue::refresh_summary(&app, &mut render, crate::cache::now());
    assert!(render.queue_summary.as_ref().unwrap().duration_ms > 0);
    crate::ui::queue::refresh_summary(&app, &mut render, expires);
    let summary = render.queue_summary.as_ref().unwrap();
    assert_eq!(summary.revision, revision);
    assert_eq!((summary.duration_ms, summary.known_durations), (0, 0));
    assert_eq!((app.queue.revision, app.cache.revision), revision);
}

#[test]
fn resizing_the_same_terminal_switches_layout_without_swapping_wallpaper_orientation() {
    let mut app = crate::demo::app();
    app.config.theme = "glass".into();
    app.config.native_glass = true;
    app.catalog.view = View::Queue;
    let mut terminal = Terminal::new(TestBackend::new(140, 38)).unwrap();
    for (width, height, tall) in [
        (140, 38, false),
        (84, 80, true),
        (160, 44, false),
        (100, 80, true),
        (120, 35, false),
    ] {
        terminal.backend_mut().resize(width, height);
        draw_terminal(&mut terminal, &app).unwrap();
        let render = app.ui.render.borrow();
        assert_eq!(render.terminal_size, (width, height));
        assert_eq!(
            render.background.orientation(),
            Some(if tall {
                background::Orientation::Vertical
            } else {
                background::Orientation::Horizontal
            })
        );
        let queue = render
            .mouse_hits
            .iter()
            .find(|(_, target)| *target == MouseTarget::QueueScroll)
            .unwrap()
            .0;
        let seek = render
            .mouse_hits
            .iter()
            .find(|(_, target)| *target == MouseTarget::Seek)
            .unwrap()
            .0;
        if tall {
            assert!(seek.x >= queue.right());
        } else {
            assert_eq!(queue.x, 22);
            assert!(seek.y >= queue.bottom());
        }
        assert!(
            render
                .mouse_hits
                .iter()
                .all(|(rect, _)| rect.right() <= width && rect.bottom() <= height)
        );
    }
}

#[test]
fn wide_windows_keep_sidebar_and_bottom_player_while_tall_windows_dock_controls() {
    for (width, height, tall) in [(140, 38, false), (120, 35, false), (84, 80, true)] {
        let mut app = crate::demo::app();
        app.catalog.view = View::Queue;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let render = app.ui.render.borrow();
        let queue = render
            .mouse_hits
            .iter()
            .find(|(_, target)| *target == MouseTarget::QueueScroll)
            .unwrap()
            .0;
        let seek = render
            .mouse_hits
            .iter()
            .find(|(_, target)| *target == MouseTarget::Seek)
            .unwrap()
            .0;
        let tabs: Vec<_> = render
            .mouse_hits
            .iter()
            .filter(|(_, target)| matches!(target, MouseTarget::Navigation(_)))
            .collect();
        assert_eq!(tabs.len(), 5);
        if tall {
            assert!(tabs.iter().all(|(rect, _)| rect.y < queue.y));
            assert!(seek.x >= queue.right());
            assert!(seek.width >= 24);
        } else {
            assert_eq!(queue.x, 22);
            assert_eq!(queue.width, width - 22 - 30);
            assert!(tabs.iter().all(|(rect, _)| rect.right() <= queue.x));
            assert!(seek.y >= queue.bottom());
            let second_header: String = (0..width)
                .map(|x| terminal.backend().buffer()[(x, 1)].symbol())
                .collect();
            assert!(
                !second_header.contains("TUITIFY"),
                "wide windows must retain their single-line header"
            );
        }
    }
}

#[test]
fn queue_selection_leaves_spacing_clear_and_glass_wallpaper_visible() {
    for theme in [Theme::Spotify, Theme::Glass] {
        let mut app = crate::demo::app();
        app.config.theme = theme.as_str().into();
        app.config.native_glass = theme == Theme::Glass;
        app.catalog.view = View::Queue;
        app.catalog.sidebar = false;
        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        terminal
            .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let render = app.ui.render.borrow();
        let selected = app.queue_selection().unwrap();
        let row = render
            .mouse_hits
            .iter()
            .find(|(_, target)| *target == MouseTarget::Queue(selected))
            .unwrap()
            .0;
        assert_eq!(row.height, 2);
        let palette = theme.palette();
        for x in row.x..row.right() {
            let gap = &terminal.backend().buffer()[(x, row.y + 1)];
            assert_eq!(gap.symbol(), " ");
            assert_eq!(
                gap.bg,
                if theme == Theme::Glass {
                    Color::Reset
                } else {
                    palette.background
                }
            );
            if theme == Theme::Glass {
                // The selection stays an opaque accent band so the row is
                // still identifiable over the wallpaper.
                assert_eq!(
                    terminal.backend().buffer()[(x, row.y)].bg,
                    palette.surface_selected
                );
            }
        }
        let marker = &terminal.backend().buffer()[(row.x, row.y)];
        assert_eq!(marker.fg, palette.primary);
    }
}

#[test]
fn expanded_player_has_a_clickable_transport_control_instead_of_thumbnail_pixels() {
    let mut app = crate::demo::app();
    // Cover metadata can still exist; rendering the player should not depend on it.
    let id = app.queue.current().unwrap().to_owned();
    let mut track = app.cache.get(&id).unwrap().clone();
    track.album_art_url = Some("https://unavailable.example/cover.png".into());
    app.cache.insert(id, track);
    for state in [State::Playing, State::Paused] {
        app.state = state;
        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        terminal
            .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let render = app.ui.render.borrow();
        let button = render
            .mouse_hits
            .iter()
            .find(|(area, target)| {
                if *target != MouseTarget::PlayPause {
                    return false;
                }
                let text: String = (area.y..area.bottom())
                    .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
                    .map(|pos| terminal.backend().buffer()[pos].symbol())
                    .collect();
                text.contains("Space")
            })
            .unwrap()
            .0;
        let symbols: String = (button.y..button.bottom())
            .flat_map(|y| (button.x..button.right()).map(move |x| (x, y)))
            .map(|pos| terminal.backend().buffer()[pos].symbol())
            .collect();
        assert!(symbols.contains("Space"));
        assert!(symbols.contains(if state == State::Playing {
            "Pause"
        } else {
            "Play"
        }));
        assert!(!symbols.contains('▀'));
    }
}

#[test]
fn short_wide_terminal_keeps_focused_navigation_item_visible_and_clickable() {
    let mut app = crate::demo::app();
    app.catalog.sidebar = true;
    app.catalog.nav = View::Help.index();
    let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
    terminal
        .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let render = app.ui.render.borrow();
    let rect = render
        .mouse_hits
        .iter()
        .find(|(_, target)| *target == MouseTarget::Navigation(View::Help))
        .unwrap()
        .0;
    let row: String = (rect.x..rect.right())
        .map(|x| terminal.backend().buffer()[(x, rect.y)].symbol())
        .collect();
    assert!(row.contains("Help"), "{row}");
}

#[test]
fn redesigned_layout_preserves_queue_mouse_rows_and_seek_at_responsive_sizes() {
    let mut app = crate::demo::app();
    app.catalog.view = View::Queue;
    app.catalog.sidebar = false;
    app.state = State::Playing;
    app.animation_frame = 42;
    app.queue.position_ms = 84_000;
    let mut previews = Vec::new();
    for (width, height) in [
        (140, 38),
        (120, 35),
        (116, 24),
        (84, 46),
        (84, 80),
        (100, 28),
        (80, 24),
        (60, 24),
        (48, 18),
        (32, 10),
    ] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let render = app.ui.render.borrow();
        let rows: Vec<_> = render
            .mouse_hits
            .iter()
            .filter_map(|(area, target)| {
                if let MouseTarget::Queue(index) = target {
                    Some((*area, *index))
                } else {
                    None
                }
            })
            .collect();
        assert!(!rows.is_empty(), "queue missing at {width}x{height}");
        for (rect, _) in &render.mouse_hits {
            assert!(rect.right() <= width && rect.bottom() <= height);
        }
        for pair in rows.windows(2) {
            assert!(pair[0].0.bottom() <= pair[1].0.y, "queue rows overlap");
        }
        for (rect, index) in &rows {
            let row: String = (rect.x..rect.right())
                .map(|x| terminal.backend().buffer()[(x, rect.y)].symbol())
                .collect();
            let track = app
                .cache
                .get(&app.queue.ids[app.queue.order[*index]])
                .unwrap();
            assert!(
                row.contains(&track.name.chars().take(5).collect::<String>()),
                "hit target misses its track: {row}"
            );
        }
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        if width >= 116 || width == 84 {
            assert!(text.contains("SPECTRUM"));
        } else {
            assert!(!text.contains("SPECTRUM"));
        }
        if width >= 60 {
            let seek = render
                .mouse_hits
                .iter()
                .find(|(_, target)| *target == MouseTarget::Seek)
                .unwrap()
                .0;
            assert!(seek.width >= 8);
            assert!(
                rows.iter().all(|(row, _)| !seek.intersects(*row)),
                "seeking must not cover a queue row"
            );
        }
        if std::env::var_os("TUITIFY_UI_PREVIEW").is_some() {
            let cells: Vec<_> = terminal.backend().buffer().content.iter().map(|cell| {
                serde_json::json!({"symbol": cell.symbol(), "fg": cell.fg.to_string(), "bg": cell.bg.to_string(), "bold": cell.modifier.contains(Modifier::BOLD)})
            }).collect();
            previews.push(serde_json::json!({"name": "queue", "width": width, "height": height, "cells": cells}));
        }
    }
    if let Some(path) = std::env::var_os("TUITIFY_UI_PREVIEW") {
        std::fs::write(path, serde_json::to_vec(&previews).unwrap()).unwrap();
    }
}

/// Exports one frame per main view so the redesigned screens can be reviewed
/// as images.  Only runs when `TUITIFY_UI_PREVIEW` points at a file.
#[test]
fn ui_preview_covers_the_main_views() {
    let Some(path) = std::env::var_os("TUITIFY_UI_PREVIEW") else {
        return;
    };
    let mut previews = Vec::new();
    for (name, view) in [
        ("search", View::Search),
        ("liked", View::Liked),
        ("playlists", View::Playlists),
        ("help", View::Help),
    ] {
        let mut app = crate::demo::app();
        app.catalog.view = view;
        app.catalog.title = match view {
            View::Search => "Search".into(),
            View::Liked => "Liked Songs".into(),
            View::Playlists => "Playlists".into(),
            View::Help => "Help".into(),
            _ => String::new(),
        };
        if matches!(view, View::Search | View::Liked) {
            app.catalog.rows = Rows::Tracks(crate::demo::playlist_tracks());
        }
        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        terminal
            .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let cells: Vec<_> = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| {
                serde_json::json!({"symbol": cell.symbol(), "fg": cell.fg.to_string(), "bg": cell.bg.to_string(), "bold": cell.modifier.contains(Modifier::BOLD)})
            })
            .collect();
        previews
            .push(serde_json::json!({"name": name, "width": 120, "height": 35, "cells": cells}));
    }
    std::fs::write(path, serde_json::to_vec(&previews).unwrap()).unwrap();
}

/// Exports the tall window with the lyrics column beside the player.
#[test]
fn portrait_lyrics_panel_swaps_the_tall_analyser_for_lyrics() {
    let mut app = crate::demo::app();
    app.catalog.view = View::Queue;
    let mut terminal = Terminal::new(TestBackend::new(84, 80)).unwrap();
    let render = |terminal: &mut Terminal<TestBackend>, app: &crate::app::App| {
        terminal
            .draw(|frame| draw(frame, app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    };

    // The panel is on by default: the lyrics column replaces the tall analyser.
    let text = render(&mut terminal, &app);
    assert!(text.contains("LYRICS"));
    assert!(
        text.contains("v expand"),
        "the compact strip replaces the graph"
    );
    assert!(!text.contains("Paused · v expand"));

    app.ui.lyrics_panel = false;
    let text = render(&mut terminal, &app);
    assert!(text.contains("SPECTRUM"));
    assert!(
        text.contains("v expand"),
        "the tall analyser stays when the panel is closed"
    );
    assert!(!text.contains("LYRICS"));
}

/// Exports the tall window with the lyrics column beside the player.
#[test]
fn ui_preview_covers_the_portrait_lyrics_column() {
    let Some(path) = std::env::var_os("TUITIFY_UI_PREVIEW") else {
        return;
    };
    let mut app = crate::demo::app();
    app.catalog.view = View::Queue;
    app.ui.lyrics_panel = true;
    app.queue.position_ms = 62_000;
    app.lyrics.content = Some(crate::lyrics::Lyrics {
        lines: vec![
            crate::lyrics::LyricLine {
                position_ms: 0,
                text: "Neon rain on a midnight train".into(),
            },
            crate::lyrics::LyricLine {
                position_ms: 30_000,
                text: "A borrowed beat comes home again".into(),
            },
            crate::lyrics::LyricLine {
                position_ms: 60_000,
                text: "Pin the spark and turn the page".into(),
            },
            crate::lyrics::LyricLine {
                position_ms: 90_000,
                text: "Build tomorrow on this stage".into(),
            },
        ],
        ..Default::default()
    });
    let mut terminal = Terminal::new(TestBackend::new(84, 80)).unwrap();
    terminal
        .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let cells: Vec<_> = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| {
            serde_json::json!({"symbol": cell.symbol(), "fg": cell.fg.to_string(), "bg": cell.bg.to_string(), "bold": cell.modifier.contains(Modifier::BOLD)})
        })
        .collect();
    std::fs::write(
        path,
        serde_json::to_vec(&serde_json::json!([{"name": "portrait-lyrics", "width": 84, "height": 80, "cells": cells}])).unwrap(),
    )
    .unwrap();
}
