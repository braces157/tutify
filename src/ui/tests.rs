use super::*;
use crate::{model::Track, queue::Queue, storage::Config};
use ratatui::backend::TestBackend;
#[test]
fn help_and_plain_lyrics_can_scroll_to_last_line_in_small_terminal() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Help;
    app.catalog.selected = usize::MAX;
    let mut terminal = Terminal::new(TestBackend::new(32, 10)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("Premium."), "{text}");
    assert!(app.ui.render.borrow().help_length > 30);
    app.ui.overlay = Overlay::Lyrics;
    app.lyrics.content = Some(crate::lyrics::Lyrics {
        lines: vec![],
        plain: Some(
            (0..100)
                .map(|i| format!("Line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    });
    app.lyrics.scroll = usize::MAX;
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("Line 99"), "{text}");
}
#[test]
fn viewport_tracks_selection_without_building_offscreen_rows() {
    let mut offset = 0;
    assert_eq!(viewport(4999, 5000, 12, &mut offset), 4988..5000);
    assert_eq!(viewport(4998, 5000, 12, &mut offset), 4988..5000);
    assert_eq!(viewport(0, 5000, 12, &mut offset), 0..12);
}
#[test]
fn queue_uses_selected_theme_accent() {
    let mut app = App::new(Config::default(), Queue::default());
    app.config.theme = "amber".into();
    app.queue.replace(vec!["0".repeat(22)], 0, false);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(
        buffer
            .content
            .iter()
            .any(|c| c.fg == Theme::Amber.primary())
    );
    assert!(
        !buffer
            .content
            .iter()
            .any(|c| c.fg == Theme::Spotify.primary())
    );
}

#[test]
fn semantic_palettes_are_cohesive_and_cyberpunk_has_one_accent_family() {
    for theme in [
        Theme::Spotify,
        Theme::Amber,
        Theme::Matrix,
        Theme::Cyberpunk,
        Theme::Monochrome,
        Theme::Glass,
    ] {
        let palette = theme.palette();
        assert_ne!(palette.background, palette.surface);
        assert_ne!(palette.surface, palette.surface_alt);
        assert_ne!(palette.surface_alt, palette.surface_selected);
        assert_ne!(palette.text, palette.text_muted);
        assert_ne!(palette.text_muted, palette.text_subtle);
        assert_ne!(palette.border, palette.border_focus);
    }

    let cyberpunk = Theme::Cyberpunk.palette();
    assert!(matches!(cyberpunk.primary, Color::Rgb(_, g, b) if b >= g));
    assert!(matches!(cyberpunk.primary_soft, Color::Rgb(_, g, b) if b >= g));
    assert_ne!(cyberpunk.primary_soft, Color::Rgb(255, 0, 127));
}

#[test]
fn glass_theme_table_header_and_durations_have_high_readability_contrast() {
    let glass = Theme::Glass;
    let palette = glass.palette();

    // Verify Glass palette values have sufficient luminance for contrast against image backgrounds
    let Color::Rgb(r_muted, g_muted, _b_muted) = palette.text_muted else {
        panic!()
    };
    let Color::Rgb(r_subtle, g_subtle, _b_subtle) = palette.text_subtle else {
        panic!()
    };
    assert!(
        r_muted >= 180 && g_muted >= 200,
        "text_muted must be bright for readability"
    );
    assert!(
        r_subtle >= 140 && g_subtle >= 170,
        "text_subtle must be legible over artwork"
    );

    // Verify header style
    let glass_header = table_header_style(glass);
    assert_eq!(glass_header.fg, Some(palette.primary_soft));
    assert!(glass_header.add_modifier.contains(Modifier::BOLD));

    let spotify_header = table_header_style(Theme::Spotify);
    assert_eq!(spotify_header.fg, Some(Theme::Spotify.palette().text_muted));

    // Verify Queue rendered row duration uses text_muted for loaded tracks
    let mut app = App::new(Config::default(), Queue::default());
    app.config.theme = "glass".into();
    app.catalog.view = View::Queue;
    let track = Track {
        id: "track_1".into(),
        name: "Test Track".into(),
        artists: "Test Artist".into(),
        duration_ms: 180_000,
        ..Default::default()
    };
    app.cache.insert(track.id.clone(), track);
    app.queue.replace(vec!["track_1".into()], 0, false);

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let buffer = terminal.backend().buffer();

    // Check that ARTIST and TIME header cells use primary_soft
    let artist_header_present = buffer.content.iter().any(|c| {
        c.symbol() == "A" && c.fg == palette.primary_soft && c.modifier.contains(Modifier::BOLD)
    });
    assert!(
        artist_header_present,
        "ARTIST header must render in bold primary_soft"
    );
    let time_header_present = buffer.content.iter().any(|c| {
        c.symbol() == "T" && c.fg == palette.primary_soft && c.modifier.contains(Modifier::BOLD)
    });
    assert!(
        time_header_present,
        "TIME header must render in bold primary_soft"
    );

    // Check that duration "3:00" renders with text_muted
    let duration_muted_present = buffer
        .content
        .iter()
        .any(|c| c.symbol() == "3" && c.fg == palette.text_muted);
    assert!(
        duration_muted_present,
        "Duration digits must render with high-contrast text_muted"
    );
}

#[test]
fn glass_background_composites_image_and_preserves_selected_surface() {
    let dir = tempfile::tempdir().unwrap();
    let image_path = dir.path().join("glass-test.png");
    let mut image = image::RgbImage::new(8, 8);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = image::Rgb([(x * 24 + 40) as u8, (y * 20 + 30) as u8, 180]);
    }
    image.save(&image_path).unwrap();

    let mut app = crate::demo::app();
    app.config.theme = Theme::Glass.as_str().into();
    app.config.background_image = Some(image_path.to_string_lossy().into_owned());
    app.config.background_dim = 20;
    app.catalog.view = View::Queue;
    app.catalog.sidebar = false;

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let palette = Theme::Glass.palette();

    const QUADRANTS: [&str; 7] = ["▘", "▝", "▀", "▖", "▌", "▞", "▛"];
    assert!(
        buffer
            .content
            .iter()
            .any(|cell| QUADRANTS.contains(&cell.symbol())),
        "glass mode should use quadrant cells for high-resolution image detail"
    );
    assert!(
        buffer
            .content
            .iter()
            .any(|cell| cell.bg == palette.surface_selected),
        "selected rows should remain opaque above the image"
    );
}

#[test]
fn responsive_glass_background_switches_between_horizontal_and_vertical() {
    let dir = tempfile::tempdir().unwrap();
    let horiz_path = dir.path().join("horiz.png");
    let vert_path = dir.path().join("vert.png");
    image::RgbImage::from_pixel(32, 32, image::Rgb([240, 20, 20]))
        .save(&horiz_path)
        .unwrap();
    image::RgbImage::from_pixel(32, 32, image::Rgb([20, 20, 240]))
        .save(&vert_path)
        .unwrap();

    let mut app = crate::demo::app();
    app.config.theme = Theme::Glass.as_str().into();
    app.config.background_image = Some(horiz_path.to_string_lossy().into_owned());
    app.config.background_image_vertical = Some(vert_path.to_string_lossy().into_owned());
    app.config.background_dim = 10;
    app.catalog.view = View::Queue;
    app.catalog.sidebar = false;

    // Landscape test (80 cols x 24 rows) -> Horizontal
    let mut terminal_h = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal_h
        .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let buf_h = terminal_h.backend().buffer();
    let has_red_tint = buf_h.content.iter().any(|cell| {
        if let Color::Rgb(r, _, b) = cell.fg {
            r > b && r > 100
        } else if let Color::Rgb(r, _, b) = cell.bg {
            r > b && r > 100
        } else {
            false
        }
    });
    assert!(
        has_red_tint,
        "Horizontal terminal should render the horizontal/landscape image"
    );

    // Portrait test (40 cols x 50 rows) -> Vertical
    let mut terminal_v = Terminal::new(TestBackend::new(40, 50)).unwrap();
    terminal_v
        .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let buf_v = terminal_v.backend().buffer();
    let has_blue_tint = buf_v.content.iter().any(|cell| {
        if let Color::Rgb(r, _, b) = cell.fg {
            b > r && b > 100
        } else if let Color::Rgb(r, _, b) = cell.bg {
            b > r && b > 100
        } else {
            false
        }
    });
    assert!(
        has_blue_tint,
        "Vertical terminal should render the vertical/portrait image"
    );
}

#[test]
fn native_glass_uses_terminal_background_for_main_surfaces() {
    let mut app = crate::demo::app();
    app.config.theme = Theme::Glass.as_str().into();
    app.config.native_glass = true;
    app.catalog.view = View::Queue;
    app.catalog.sidebar = false;

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let palette = Theme::Glass.palette();

    assert!(
        buffer.content.iter().any(|cell| cell.bg == Color::Reset),
        "native Glass should expose the terminal's GPU-rendered background"
    );
    assert_eq!(
        buffer[(0, 0)].bg,
        Color::Reset,
        "header background must be transparent so wallpaper covers top"
    );
    assert_eq!(
        buffer[(0, 22)].bg,
        Color::Reset,
        "playback border background must be transparent so wallpaper covers bottom"
    );
    assert_eq!(
        buffer[(20, 23)].bg,
        Color::Reset,
        "playback interior background must be transparent so wallpaper covers bottom"
    );
    assert_eq!(
        buffer[(0, 29)].bg,
        Color::Reset,
        "footer background must be transparent so wallpaper covers bottom"
    );
    assert!(
        !buffer
            .content
            .iter()
            .any(|cell| cell.bg == palette.surface_alt),
        "header/playback/footer surfaces should be transparent for full background coverage"
    );
    assert!(
        buffer
            .content
            .iter()
            .any(|cell| cell.bg == palette.surface_selected),
        "selected rows should remain opaque above the wallpaper"
    );
    assert!(
        !buffer.content.iter().any(|cell| {
            matches!(cell.symbol(), "▀" | "▌" | "▐" | "▚" | "▞")
                && cell.bg != palette.surface_selected
        }),
        "native Glass should not reconstruct wallpaper from block glyphs"
    );
}

#[test]
fn selected_current_track_preserves_both_visual_states() {
    let mut app = App::new(Config::default(), Queue::default());
    let mut current = Track::unknown(&"0".repeat(22));
    current.name = "Current and selected".into();
    current.artists = "Artist".into();
    current.duration_ms = 180_000;
    app.catalog.rows = Rows::Tracks(vec![current.clone()]);
    app.catalog.sidebar = false;
    app.catalog.selected = 0;
    app.queue.replace(vec![current.id.clone()], 0, false);
    app.cache.insert(current.id.clone(), current);
    app.state = State::Playing;

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let palette = Theme::Spotify.palette();
    let buffer = terminal.backend().buffer();
    assert!(
        buffer
            .content
            .iter()
            .any(|cell| cell.fg == palette.primary && cell.bg == palette.surface_selected),
        "current-track accent should survive the selected-row surface"
    );
    assert!(
        buffer
            .content
            .iter()
            .any(|cell| cell.symbol() == "▌" && cell.fg == palette.primary),
        "selected row should retain a non-color marker"
    );
}

#[test]
fn every_theme_renders_playback_states_at_required_sizes() {
    for theme in [
        Theme::Spotify,
        Theme::Amber,
        Theme::Matrix,
        Theme::Cyberpunk,
        Theme::Monochrome,
        Theme::Glass,
    ] {
        for (width, height) in [(120, 35), (80, 24), (48, 18), (32, 10), (20, 6)] {
            for state in [State::Paused, State::Loading, State::Playing, State::Failed] {
                let mut app = crate::demo::app();
                app.config.theme = theme.as_str().into();
                app.state = state;
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
                    .unwrap();
                let text = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                assert!(text.contains("TUITIFY"));
            }
        }
    }
}
#[test]
#[ignore = "Release microbenchmark; run with --release --ignored --nocapture"]
fn benchmark_render_scaling() {
    for count in [50, 500, 5000] {
        let mut app = App::new(Config::default(), Queue::default());
        app.queue
            .replace((0..count).map(|i| format!("{i:022}")).collect(), 0, false);
        let tracks: Vec<Track> = (0..count)
            .map(|i| Track {
                id: format!("{i:022}"),
                name: format!("Benchmark song {i}"),
                artists: "Benchmark artist".into(),
                duration_ms: 200000,
                playable: true,
                ..Default::default()
            })
            .collect();
        for track in tracks.iter().take(50) {
            app.cache.insert(track.id.clone(), track.clone());
        }
        for mode in ["queue", "filtered"] {
            app.catalog.view = if mode == "queue" {
                View::Queue
            } else {
                View::Liked
            };
            if mode == "filtered" {
                app.catalog.rows = Rows::Tracks(tracks.clone());
                app.catalog.filter = "benchmark artist".into();
            }
            let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
            terminal
                .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
                .unwrap();
            let start = std::time::Instant::now();
            for _ in 0..100 {
                terminal
                    .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
                    .unwrap();
            }
            println!(
                "mode={mode} rows={count} mean_frame_ms={:.3} release={}",
                start.elapsed().as_secs_f64() * 10.0,
                !cfg!(debug_assertions)
            );
        }
    }
}
#[test]
fn render_all_views_normal_narrow_and_tiny() {
    for (w, h) in [(120, 35), (80, 24), (48, 18), (32, 10), (20, 6)] {
        for view in View::ALL {
            let mut app = App::new(Config::default(), Queue::default());
            app.catalog.view = view;
            let mut t = Track::unknown(&"0".repeat(22));
            t.name = "A song with Unicode: café 日本語".into();
            app.queue.replace(vec![t.id.clone()], 0, false);
            app.cache.insert(t.id.clone(), t.clone());
            app.catalog.rows = Rows::Tracks(vec![t]);
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
                .unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("TUITIFY"));
            if w >= 32 && h >= 10 {
                assert!(text.contains("PAUSED"));
            }
            if w >= 70 {
                assert!(text.contains("VOL 50%"));
                assert!(text.contains("R:Off"));
            }
        }
    }
}

#[test]
fn render_mix_builder_normal_and_narrow_with_explanations() {
    for (width, height) in [(120, 35), (80, 24), (48, 18), (32, 10), (20, 6)] {
        let mut app = crate::demo::app();
        app.open_queue_mix();
        app.mix.preview.entries[0].track.name =
            "A very long Unicode title — คืนฝนพรำ — 日本語".into();
        app.mix.preview.entries[0].track.artists =
            "Mali & The Signals featuring an exceptionally long artist name".into();
        app.mix.source_partial = true;
        app.mix.source_error = Some("playlist page 3 failed; retained earlier pages".into());
        app.mix.recommendation_error = Some("simulated outage".into());
        app.mix.refresh();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("TUITIFY"));
        if width >= 48 {
            assert!(text.contains("MIX BUILDER"), "{text}");
            assert!(text.contains("PARTIAL"), "{text}");
            for control in [
                "Enter replace",
                "A append",
                "w save",
                "o reopen",
                "Esc cancel",
            ] {
                assert!(
                    text.contains(control),
                    "missing {control} at {width}x{height}: {text}"
                );
            }
        } else if width == 32 {
            assert!(text.contains("? details"), "{text}");
            assert!(text.contains("Esc cancel"), "{text}");
        }

        app.mix.detail = true;
        app.mix.detail_scroll = 0;
        terminal
            .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let top: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        if width >= 80 {
            assert!(top.contains("Source note:"), "{top}");
            assert!(top.contains("playlist page 3 failed"), "{top}");
            assert!(top.contains("From your current queue"), "{top}");
            assert!(top.contains("desired 25%; achieved"), "{top}");
        }
        if width >= 32 && height >= 10 {
            let mut scrolled = String::new();
            for offset in 0..80 {
                app.mix.detail_scroll = offset;
                terminal
                    .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
                    .unwrap();
                scrolled.extend(
                    terminal
                        .backend()
                        .buffer()
                        .content
                        .iter()
                        .map(|cell| cell.symbol()),
                );
            }
            assert!(scrolled.contains("Esc close"), "{scrolled}");
            assert!(scrolled.contains("details"), "{scrolled}");
            if width >= 48 {
                assert!(scrolled.contains("w save"), "{scrolled}");
            } else {
                assert!(scrolled.contains("A append • w"), "{scrolled}");
                assert!(scrolled.contains("save • o reopen"), "{scrolled}");
            }
        }
    }
}

#[test]
fn render_mix_builder_naming_loading_and_failure_states() {
    let mut app = crate::demo::app();
    app.open_queue_mix();
    app.mix.source = Some(crate::mix::MixSource::Playlist {
        id: "9".repeat(22),
        name: "Long source playlist".into(),
    });
    app.mix.naming = true;
    app.mix.recipe_name = format!("Night commute 東京{}", "é".repeat(64));
    assert_eq!(app.mix.recipe_name.chars().count(), 80);
    for (width, height) in [(80, 24), (48, 18), (32, 10)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let naming: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(naming.contains("Recipe name:"), "{naming}");
        assert!(naming.contains("Night commute"), "{naming}");
        assert!(naming.contains("_"), "insertion marker missing: {naming}");
        if width >= 48 {
            assert!(naming.contains("Enter"), "{naming}");
            assert!(naming.contains("save"), "{naming}");
        }
    }

    app.mix.naming = false;
    app.mix.loading_source = true;
    app.mix.source_partial = true;
    app.mix.source_error = Some("rate limited after page 2".into());
    app.mix.detail = true;
    let mut terminal = Terminal::new(TestBackend::new(48, 18)).unwrap();
    terminal
        .draw(|frame| draw(frame, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let loading: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(loading.contains("LOADING/PARTIAL"), "{loading}");
    assert!(loading.contains("rate limited"), "{loading}");
}
#[test]
fn render_playback_timestamp_and_muted_volume() {
    let mut app = App::new(Config::default(), Queue::default());
    let mut track = Track::unknown(&"0".repeat(22));
    track.name = "Timestamp check".into();
    track.duration_ms = 210_000;
    app.queue.replace(vec![track.id.clone()], 0, false);
    app.queue.position_ms = 23_000;
    app.cache.insert(track.id.clone(), track);
    app.config.volume = 0;
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("0:23 / 3:30  10%  -3:07"));
    assert!(text.contains("VOL MUTED"));
}
#[test]
fn render_visualizer_bars_and_smooth_animation() {
    let mut app = App::new(Config::default(), Queue::default());
    let mut track = Track::unknown(&"0".repeat(22));
    track.name = "Moon Landing Plan".into();
    track.artists = "tuki.".into();
    track.duration_ms = 242_000;
    app.queue.replace(vec![track.id.clone()], 0, false);
    app.queue.position_ms = 157_000;
    app.cache.insert(track.id.clone(), track);
    app.state = State::Playing;
    app.animation_frame = 42;

    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();

    assert!(text.contains("Moon Landing Plan"));
    assert!(text.contains("tuki."));
    assert!(text.contains("2:37 / 4:02  64%  -1:25"));
    assert!(text.contains("VOL 50%"));
    let has_eq_bar = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█']
        .iter()
        .any(|&c| text.contains(c));
    assert!(has_eq_bar);
}
#[test]
fn render_real_time_visualizer_with_live_audio() {
    let mut app = App::new(Config::default(), Queue::default());
    let mut track = Track::unknown(&"0".repeat(22));
    track.name = "Live Frequency Track".into();
    track.artists = "Artist".into();
    track.duration_ms = 200_000;
    app.queue.replace(vec![track.id.clone()], 0, false);
    app.cache.insert(track.id.clone(), track);
    app.state = State::Playing;
    app.ui.overlay = Overlay::Visualizer;

    // Feed some real audio samples (220 Hz tone)
    for i in 0..2048 {
        let t = i as f32 / 44100.0;
        app.visualizer
            .push_sample((2.0 * std::f32::consts::PI * 220.0 * t).sin());
    }

    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();

    assert!(text.contains("RETRO VISUALIZER"));
    assert!(text.contains("REAL-TIME SPECTRUM"));
    assert!(text.contains("Real-time FFT"));
    assert!(text.contains("50Hz"));
}
#[test]
fn render_modern_ui_elements() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Queue;
    let mut t1 = Track::unknown(&"0".repeat(22));
    t1.name = "My Song".into();
    t1.artists = "My Artist".into();
    t1.duration_ms = 180_000;
    let uncached_id = "1".repeat(22);
    app.queue
        .replace(vec![t1.id.clone(), uncached_id.clone()], 0, false);
    app.cache.insert(t1.id.clone(), t1);
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("TITLE"));
    assert!(text.contains("ARTIST"));
    assert!(text.contains("TIME"));
    assert!(text.contains("My Song"));
    assert!(text.contains("My Artist"));
    assert!(text.contains("Loading track info..."));
    assert!(!text.contains(&uncached_id));
    assert!(text.contains("QUEUE • 2"));
    assert!(text.contains("Space"));
    assert!(text.contains("Play/Pause"));
}
#[test]
fn render_filter_prompt_and_filtered_tracks() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Liked;
    app.catalog.title = "Liked Songs".into();
    app.catalog.rows = Rows::Tracks(vec![
        Track {
            id: "1".into(),
            name: "Bohemian Rhapsody".into(),
            artists: "Queen".into(),
            duration_ms: 354000,
            playable: true,
            ..Default::default()
        },
        Track {
            id: "2".into(),
            name: "Yellow".into(),
            artists: "Coldplay".into(),
            duration_ms: 269000,
            playable: true,
            ..Default::default()
        },
    ]);
    app.catalog.filtering = true;
    app.catalog.filter = "queen".into();

    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();

    assert!(text.contains("FILTER"));
    assert!(text.contains("queen"));
    assert!(text.contains("1 of 2 loaded matches"));
    assert!(text.contains("Bohemian Rhapsody"));
    assert!(!text.contains("Coldplay"));

    // When no matches
    app.catalog.filter = "jazz".into();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text2 = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text2.contains("No loaded tracks match your filter"));
}

#[test]
fn test_wrap_lyric_line_basics() {
    assert_eq!(wrap_lyric_line("", 20), vec![""]);
    assert_eq!(wrap_lyric_line("   ", 20), vec![""]);
    assert_eq!(wrap_lyric_line("Hello world", 20), vec!["Hello world"]);
    assert_eq!(
        wrap_lyric_line(
            "And tasted the sweet perfume of the mountain grass I rolled down",
            30
        ),
        vec![
            "And tasted the sweet perfume",
            "of the mountain grass I rolled",
            "down",
        ]
    );
    // Word longer than max width chunks cleanly
    assert_eq!(
        wrap_lyric_line("abcdefghijklm", 5),
        vec!["abcde", "fghij", "klm"]
    );
}

#[test]
fn test_synced_lyrics_wraps_in_narrow_terminal() {
    let mut app = App::new(Config::default(), Queue::default());
    app.ui.overlay = Overlay::Lyrics;
    let mut track = Track::unknown(&"0".repeat(22));
    track.name = "Castle on the Hill".into();
    track.artists = "Ed Sheeran".into();
    app.queue.replace(vec![track.id.clone()], 0, false);
    app.cache.insert(track.id.clone(), track);

    let lrc_sample = "\
[00:01.00] When I was six years old, I broke my leg
[00:05.00] I was running from my brother and his friends
[00:10.00] And tasted the sweet perfume of the mountain grass I rolled down
[00:15.00] I was younger then, take me back to when I";

    app.lyrics.content = Some(crate::lyrics::Lyrics {
        lines: crate::lyrics::parse_lrc(lrc_sample),
        plain: None,
    });
    // Position at 16 seconds (line index 3 is active: "I was younger then...")
    app.queue.position_ms = 16_000;

    // Terminal width 80 (just like the user's resized screenshot)
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();

    // "rolled down" was previously cut off at "grass I r"
    assert!(
        text.contains("rolled down"),
        "Expected 'rolled down' to be visible, but buffer was:\n{text}"
    );
    assert!(
        text.contains("mountain grass"),
        "Expected 'mountain grass' to be visible, but buffer was:\n{text}"
    );
    assert!(
        text.contains("► I was younger then"),
        "Expected active marker on current line, but buffer was:\n{text}"
    );
}

#[test]
fn render_stats_empty_normal_and_narrow() {
    let mut app = App::new(Config::default(), Queue::default());
    app.ui.overlay = Overlay::Stats;

    // Normal terminal (100x30)
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("SONG STATISTICS [S/Esc exit]"));
    assert!(text.contains("No song statistics yet."));

    // Narrow terminal (60x20)
    let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("SONG STATISTICS [S/Esc exit]"));
    assert!(text.contains("No song statistics yet."));

    // Tiny terminal (34x15) -> truncated title
    let mut terminal = Terminal::new(TestBackend::new(34, 15)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("STATS [S/Esc exit]"));
}

#[test]
fn render_stats_populated_normal_and_narrow() {
    let mut app = App::new(Config::default(), Queue::default());
    let id1 = "1".repeat(22);
    let id2 = "2".repeat(22);
    app.stats.add_play(&id1, "Track Alpha", "Artist One");
    app.stats.tracks.get_mut(&id1).unwrap().play_count = 10;
    app.stats.tracks.get_mut(&id1).unwrap().listened_ms = 300_000;

    app.stats.add_play(&id2, "Track Beta", "Artist Two");
    app.stats.tracks.get_mut(&id2).unwrap().play_count = 5;
    app.stats.tracks.get_mut(&id2).unwrap().listened_ms = 150_000;

    app.queue.replace(vec![id1.clone()], 0, false);
    app.state = State::Playing;
    app.ui.overlay = Overlay::Stats;

    // Normal terminal (100x30) -> shows Artist column and active indicator
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();

    assert!(text.contains("SONG STATISTICS [S/Esc exit]"));
    assert!(text.contains("Title"));
    assert!(text.contains("Artist"));
    assert!(text.contains("Plays"));
    assert!(text.contains("Time"));
    assert!(text.contains("Track Alpha"));
    assert!(text.contains("Artist One"));
    assert!(text.contains("10"));
    assert!(text.contains("5:00"));
    assert!(text.contains("Track Beta"));
    assert!(text.contains("Artist Two"));
    assert!(text.contains("►"));
    assert!(text.contains("All time: 7:30 | 15 plays | 2 songs"));
    assert!(text.contains("Most played: Track Alpha"));
    assert!(text.contains("Share"));
    assert!(text.contains("66.7%"));
    assert!(text.contains("33.3%"));

    // Narrow terminal (60x20) -> collapses Artist column
    let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();

    assert!(text.contains("SONG STATISTICS [S/Esc exit]"));
    assert!(text.contains("Title"));
    assert!(!text.contains("Artist"));
    assert!(text.contains("Plays"));
    assert!(text.contains("Time"));
    assert!(text.contains("Track Alpha"));
    assert!(!text.contains("Artist One"));
    assert!(text.contains("Track Beta"));
    assert!(!text.contains("Artist Two"));
}

#[test]
fn render_stats_search_empty_results_and_tiny_sizes() {
    let mut app = App::new(Config::default(), Queue::default());
    app.stats.add_play(&"1".repeat(22), "Song", "Singer");
    app.ui.overlay = Overlay::Stats;
    app.ui.stats.get_mut().query = "missing".into();
    for (width, height) in [(100, 30), (34, 15), (32, 10), (10, 4)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        if width == 100 {
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("No matching songs."));
            assert!(text.contains("0 matches"));
            assert!(text.contains("1 plays | 1 songs"));
        }
    }
}
#[test]
#[ignore = "Requires a real terminal; exercises alternate screen and a caught panic"]
fn terminal_cleanup_acceptance() {
    {
        let _guard = TerminalGuard::enter().unwrap();
        assert!(crossterm::terminal::is_raw_mode_enabled().unwrap());
    }
    assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
    let caught = std::panic::catch_unwind(|| {
        let _guard = TerminalGuard::enter().unwrap();
        panic!("intentional terminal restoration test");
    });
    assert!(caught.is_err());
    assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
    println!("PASS: raw mode restored after normal exit and a caught panic");
}

#[test]
fn authentication_banner_uses_health_instead_of_status_wording() {
    let mut app = App::new(Config::default(), Queue::default());
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    for (health, status, expected) in [
        (
            crate::catalog::Health::Unknown,
            "Try auth --force if needed",
            false,
        ),
        (
            crate::catalog::Health::AuthenticationRequired,
            "Volume 50%",
            true,
        ),
        (
            crate::catalog::Health::Ready,
            "Catalog login failed earlier",
            false,
        ),
    ] {
        app.catalog_health = health;
        app.status = status.into();
        terminal
            .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert_eq!(text.contains("AUTH EXPIRED"), expected);
    }
}

#[test]
fn test_format_breadcrumb_trail_basics_and_collapsing() {
    // 1. Full fit
    let history = ["Search"];
    assert_eq!(
        format_breadcrumb_trail(history, "OK Computer", 80),
        "Search › OK Computer"
    );

    // 2. Multi-level full fit
    let multi = ["Search", "Radiohead"];
    assert_eq!(
        format_breadcrumb_trail(multi, "OK Computer", 80),
        "Search › Radiohead › OK Computer"
    );

    // 3. Exact width fit
    let full_len = "Search › OK Computer".chars().count();
    assert_eq!(
        format_breadcrumb_trail(history, "OK Computer", full_len),
        "Search › OK Computer"
    );

    // 4. Intermediate collapsing with 3+ items
    let deep = ["Search", "Level 1", "Level 2"];
    assert_eq!(
        format_breadcrumb_trail(deep, "Target", 22),
        "… › Level 2 › Target"
    );

    // 5. Collapsing to last item
    assert_eq!(format_breadcrumb_trail(deep, "Target", 12), "… › Target");

    // 6. Two items where "… › current" fits
    assert_eq!(
        format_breadcrumb_trail(["Search"], "A Long Title", 16),
        "… › A Long Title"
    );

    // 7. Narrow width truncates with ellipsis
    let res = format_breadcrumb_trail(["Search"], "Extremely Long Title", 10);
    assert!(res.ends_with('…'), "expected ellipsis at end, got {res}");
    assert!(
        res.chars().count() <= 10,
        "expected <= 10 chars, got {}",
        res.chars().count()
    );

    // 8. Empty history returns title
    assert_eq!(
        format_breadcrumb_trail(std::iter::empty::<&str>(), "Search", 80),
        "Search"
    );

    // 9. Zero max_width returns full string without truncating
    assert_eq!(
        format_breadcrumb_trail(["Search", "Rock"], "Radiohead", 0),
        "Search › Rock › Radiohead"
    );
}

#[test]
fn test_album_tracklist_ui_rendering() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Album;
    app.catalog.title = "OK Computer".into();

    let t1 = Track {
        id: "1".repeat(22),
        name: "Airbag".into(),
        artists: "Radiohead".into(),
        duration_ms: 284_000,
        playable: true,
        track_number: Some(1),
        album: Some("OK Computer".into()),
        album_id: Some("album1".into()),
        ..Default::default()
    };
    let t2 = Track {
        id: "2".repeat(22),
        name: "Paranoid Android".into(),
        artists: "Radiohead".into(),
        duration_ms: 383_000,
        playable: false,
        track_number: Some(2),
        album: Some("OK Computer".into()),
        album_id: Some("album1".into()),
        ..Default::default()
    };
    let t3 = Track {
        id: "3".repeat(22),
        name: "Subterranean Homesick Alien".into(),
        artists: "Radiohead".into(),
        duration_ms: 267_000,
        playable: true,
        track_number: None, // fallback to sequence
        album: Some("OK Computer".into()),
        album_id: Some("album1".into()),
        ..Default::default()
    };

    app.catalog.rows = Rows::Tracks(vec![t1.clone(), t2.clone(), t3.clone()]);
    app.cache.insert(t1.id.clone(), t1.clone());
    app.cache.insert(t2.id.clone(), t2.clone());
    app.cache.insert(t3.id.clone(), t3.clone());

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();

    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();

    assert!(text.contains("OK Computer"), "Missing album title: {text}");
    assert!(text.contains("TITLE"), "Missing TITLE column: {text}");
    assert!(text.contains("ARTIST"), "Missing ARTIST column: {text}");
    assert!(text.contains("TIME"), "Missing TIME column: {text}");
    assert!(text.contains("Airbag"), "Missing track 1: {text}");
    assert!(
        text.contains("Paranoid Android [unavailable]"),
        "Missing unavailable track 2: {text}"
    );
    assert!(
        text.contains("Subterranean Homesick Alien"),
        "Missing track 3: {text}"
    );
    assert!(
        text.contains("4:44"),
        "Missing duration for track 1: {text}"
    );

    // Test empty album message
    app.catalog.rows = Rows::Tracks(vec![]);
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let empty_text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(
        empty_text.contains("No tracks found in this album."),
        "{empty_text}"
    );
    assert!(
        empty_text.contains("Esc  Go back to previous view"),
        "{empty_text}"
    );
}

#[test]
fn test_artist_top_tracks_ui_rendering() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Artist;
    app.catalog.title = "Radiohead • Top Tracks".into();

    let t1 = Track {
        id: "1".repeat(22),
        name: "Creep".into(),
        artists: "Radiohead".into(),
        duration_ms: 238_000,
        playable: true,
        album: Some("Pablo Honey".into()),
        ..Default::default()
    };
    let t2 = Track {
        id: "2".repeat(22),
        name: "Karma Police".into(),
        artists: "Radiohead".into(),
        duration_ms: 261_000,
        playable: true,
        album: None, // fallback to "-"
        ..Default::default()
    };
    let t3 = Track {
        id: "3".repeat(22),
        name: "No Surprises".into(),
        artists: "Radiohead".into(),
        duration_ms: 228_000,
        playable: false,
        album: Some("OK Computer".into()),
        ..Default::default()
    };

    app.catalog.rows = Rows::Tracks(vec![t1.clone(), t2.clone(), t3.clone()]);
    app.cache.insert(t1.id.clone(), t1.clone());
    app.cache.insert(t2.id.clone(), t2.clone());
    app.cache.insert(t3.id.clone(), t3.clone());

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();

    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();

    assert!(
        text.contains("Radiohead • Top Tracks"),
        "Missing artist title: {text}"
    );
    assert!(text.contains("TITLE"), "Missing TITLE column: {text}");
    assert!(
        text.contains("ALBUM"),
        "Artist view must have ALBUM header: {text}"
    );
    assert!(
        text.contains("Pablo Honey"),
        "Missing album name for track 1: {text}"
    );
    assert!(text.contains("Karma Police"), "Missing track 2: {text}");
    assert!(
        text.contains("No Surprises [unavailable]"),
        "Missing unavailable track 3: {text}"
    );

    // Test empty artist message
    app.catalog.rows = Rows::Tracks(vec![]);
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let empty_text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(
        empty_text.contains("No top tracks found for this artist."),
        "{empty_text}"
    );
    assert!(
        empty_text.contains("Esc  Go back to previous view"),
        "{empty_text}"
    );
}

#[test]
fn test_breadcrumb_ui_header_rendering() {
    let mut app = App::new(Config::default(), Queue::default());
    // Simulate navigation from Search to Album
    app.catalog.title = "Search".into();
    app.push_navigation("Search".into());
    app.catalog.view = View::Album;
    app.catalog.title = "OK Computer".into();
    app.catalog.rows = Rows::Tracks(vec![Track {
        id: "1".repeat(22),
        name: "Airbag".into(),
        artists: "Radiohead".into(),
        duration_ms: 284_000,
        playable: true,
        track_number: Some(1),
        ..Default::default()
    }]);

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();

    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();

    assert!(
        text.contains("Search › OK Computer"),
        "Breadcrumb header missing: {text}"
    );

    // Simulate multi-level navigation: Search -> OK Computer -> Radiohead • Top Tracks
    app.push_navigation("OK Computer".into());
    app.catalog.view = View::Artist;
    app.catalog.title = "Radiohead • Top Tracks".into();

    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();

    let text2 = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();

    assert!(
        text2.contains("Search › OK Computer › Radiohead • Top Tracks"),
        "Multi-level breadcrumb header missing: {text2}"
    );

    // Narrow terminal bounds test (48 columns) - ensure it renders without panic and truncates gracefully
    let mut narrow_terminal = Terminal::new(TestBackend::new(48, 18)).unwrap();
    narrow_terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let narrow_text = narrow_terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(narrow_text.contains("TUITIFY"));
    // Header should fit within 48 width and show collapsed or truncated trail
    assert!(narrow_text.contains('…') || narrow_text.contains("Top Tracks"));
}

#[test]
fn test_help_shortcuts_documented() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Help;

    let mut terminal = Terminal::new(TestBackend::new(160, 60)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();

    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();

    assert!(
        text.contains("View album for selected track"),
        "Missing 'a' shortcut: {text}"
    );
    assert!(
        text.contains("View artist top tracks"),
        "Missing 'Shift+A / A' shortcut: {text}"
    );
    assert!(
        text.contains("Add selected track to queue"),
        "Missing 'e' shortcut: {text}"
    );
    assert!(
        text.contains("Play next (in Album/Artist views) / Previous track"),
        "Missing 'p' shortcut: {text}"
    );
    assert!(
        text.contains("Back to previous view / Close overlay / Quit"),
        "Missing 'Esc' shortcut: {text}"
    );

    // Also verify when scrolled down in normal terminal height (35 lines)
    let mut terminal_35 = Terminal::new(TestBackend::new(120, 35)).unwrap();
    app.catalog.selected = 25;
    terminal_35
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let scrolled_text = terminal_35
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(
        scrolled_text.contains("Add selected track to queue"),
        "Missing 'e' shortcut in scrolled view: {scrolled_text}"
    );
}

#[test]
fn test_center_delegates_album_and_artist() {
    let mut app = App::new(Config::default(), Queue::default());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    // Album view
    app.catalog.view = View::Album;
    app.catalog.title = "In Rainbows".into();
    app.catalog.rows = Rows::Tracks(vec![Track {
        id: "1".repeat(22),
        name: "15 Step".into(),
        artists: "Radiohead".into(),
        duration_ms: 237_000,
        playable: true,
        track_number: Some(1),
        ..Default::default()
    }]);

    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("In Rainbows"), "{text}");
    assert!(text.contains("15 Step"), "{text}");

    // Artist view
    app.catalog.view = View::Artist;
    app.catalog.title = "Radiohead • Top Tracks".into();
    app.catalog.rows = Rows::Tracks(vec![Track {
        id: "2".repeat(22),
        name: "Karma Police".into(),
        artists: "Radiohead".into(),
        duration_ms: 261_000,
        playable: true,
        album: Some("OK Computer".into()),
        ..Default::default()
    }]);

    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text2 = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text2.contains("Karma Police"), "{text2}");
    assert!(text2.contains("ALBUM"), "{text2}");
}

#[test]
fn test_adversarial_album_tracklist_rendering() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Album;
    app.catalog.title = "OK Computer".into();

    // 1. Tracks with track_number: None (sequence fallback) and large track numbers (e.g. disc 2 track 45)
    let tracks = vec![
        Track {
            id: "1".repeat(22),
            name: "Airbag".into(),
            artists: "Radiohead".into(),
            duration_ms: 284_000,
            playable: true,
            track_number: None, // Sequence fallback -> 1
            ..Default::default()
        },
        Track {
            id: "2".repeat(22),
            name: "Paranoid Android".into(),
            artists: "Radiohead".into(),
            duration_ms: 383_000,
            playable: true,
            track_number: Some(2),
            ..Default::default()
        },
        Track {
            id: "3".repeat(22),
            name: "Subterranean Homesick Alien".into(),
            artists: "Radiohead".into(),
            duration_ms: 267_000,
            playable: true,
            track_number: None, // Sequence fallback -> 3
            ..Default::default()
        },
        Track {
            id: "4".repeat(22),
            name: "Exit Music (For a Film)".into(),
            artists: "Radiohead".into(),
            duration_ms: 264_000,
            playable: true,
            track_number: Some(45), // Large track number (disc 2 track 45)
            ..Default::default()
        },
        Track {
            id: "5".repeat(22),
            name: "Let Down".into(),
            artists: "Radiohead".into(),
            duration_ms: 299_000,
            playable: true,
            track_number: Some(99), // 2-digit max
            ..Default::default()
        },
        Track {
            id: "6".repeat(22),
            name: "Karma Police".into(),
            artists: "Radiohead".into(),
            duration_ms: 261_000,
            playable: true,
            track_number: Some(120), // 3-digit large track number
            ..Default::default()
        },
    ];

    app.catalog.rows = Rows::Tracks(tracks);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut rendered_lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        rendered_lines.push(line);
    }
    let all_text = rendered_lines.join("\n");

    // Verify track 1 sequence fallback (1)
    assert!(all_text.contains("Airbag"), "Missing Airbag: {all_text}");
    assert!(
        rendered_lines
            .iter()
            .any(|l| l.contains("  1 ") && l.contains("Airbag")),
        "Track 1 should render sequence fallback '  1 ':\n{all_text}"
    );

    // Verify track 2 explicit track_number (2)
    assert!(
        rendered_lines
            .iter()
            .any(|l| l.contains("  2 ") && l.contains("Paranoid Android")),
        "Track 2 should render track_number '  2 ':\n{all_text}"
    );

    // Verify track 3 sequence fallback (3)
    assert!(
        rendered_lines
            .iter()
            .any(|l| l.contains("  3 ") && l.contains("Subterranean")),
        "Track 3 should render sequence fallback '  3 ':\n{all_text}"
    );

    // Verify track 4 large track number 45 (disc 2 track 45)
    assert!(
        rendered_lines
            .iter()
            .any(|l| l.contains(" 45 ") && l.contains("Exit Music")),
        "Track 4 should render large track_number ' 45 ':\n{all_text}"
    );

    // Verify track 5 track number 99
    assert!(
        rendered_lines
            .iter()
            .any(|l| l.contains(" 99 ") && l.contains("Let Down")),
        "Track 5 should render track_number ' 99 ':\n{all_text}"
    );

    // Verify track 6 3-digit track number 120
    assert!(
        rendered_lines
            .iter()
            .any(|l| l.contains("120 ") && l.contains("Karma Police")),
        "Track 6 should render 3-digit track_number '120 ':\n{all_text}"
    );

    // 2. 100% unplayable albums
    let unplayable_tracks: Vec<Track> = (1..=5)
        .map(|i| Track {
            id: format!("{:022}", i),
            name: format!("Track {i}"),
            artists: "Various Artists".into(),
            duration_ms: 180_000,
            playable: false,
            track_number: Some(i),
            ..Default::default()
        })
        .collect();

    app.catalog.rows = Rows::Tracks(unplayable_tracks);
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();

    let unplayable_text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();

    for i in 1..=5 {
        assert!(
            unplayable_text.contains(&format!("Track {i} [unavailable]")),
            "Missing unavailable tag for track {i}: {unplayable_text}"
        );
    }
    // Verify it doesn't show empty message
    assert!(
        !unplayable_text.contains("No tracks found in this album"),
        "Unplayable album should render rows, not empty message: {unplayable_text}"
    );

    // Verify DIM style on unplayable tracks
    let unplayable_buffer = terminal.backend().buffer();
    let has_dim = unplayable_buffer
        .content
        .iter()
        .any(|cell| cell.modifier.contains(Modifier::DIM));
    assert!(has_dim, "Unplayable track titles must have DIM modifier");

    // 3. Empty album states
    app.catalog.rows = Rows::Tracks(vec![]);
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let empty_text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(
        empty_text.contains("No tracks found in this album."),
        "Empty album missing guidance: {empty_text}"
    );
    assert!(
        empty_text.contains("Esc  Go back to previous view"),
        "Empty album missing Esc hint: {empty_text}"
    );

    // Empty album while busy loading
    app.catalog.busy = true;
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let busy_text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(
        busy_text.contains("Loading tracks"),
        "Busy album should show loading tracks: {busy_text}"
    );
    app.catalog.busy = false;

    // Verify that empty album specifically presents album guidance, not playlist or liked filter message
    assert!(
        empty_text.contains("No tracks found in this album."),
        "Empty album should show specific album guidance: {empty_text}"
    );
}

#[test]
fn test_adversarial_artist_top_tracks_rendering() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Artist;
    app.catalog.title = "Radiohead • Top Tracks".into();

    // 1. 0 tracks (empty state)
    app.catalog.rows = Rows::Tracks(vec![]);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let empty_text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(
        empty_text.contains("No top tracks found for this artist."),
        "Empty artist missing guidance: {empty_text}"
    );
    assert!(
        empty_text.contains("Esc  Go back to previous view"),
        "Empty artist missing Esc hint: {empty_text}"
    );

    // 2. Exactly 1 track
    let single_track = vec![Track {
        id: "1".repeat(22),
        name: "Creep".into(),
        artists: "Radiohead".into(),
        duration_ms: 238_000,
        playable: true,
        album: Some("Pablo Honey".into()),
        ..Default::default()
    }];
    app.catalog.rows = Rows::Tracks(single_track);
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let single_text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(single_text.contains("ALBUM"), "Missing ALBUM column header");
    assert!(single_text.contains("Creep"), "Missing track title");
    assert!(single_text.contains("Pablo Honey"), "Missing album name");
    assert!(single_text.contains("3:58"), "Missing duration");

    // 3. Exactly 10 tracks (standard Spotify top-tracks count)
    let ten_tracks: Vec<Track> = (1..=10)
        .map(|i| Track {
            id: format!("{:022}", i),
            name: format!("Top Song {i}"),
            artists: "Radiohead".into(),
            duration_ms: 200_000 + (i as u32 * 10_000),
            playable: true,
            album: if i == 5 {
                None // 4. album: None fallback test
            } else if i == 7 {
                // 5. long album name test
                Some(
                    "The Rise and Fall of Ziggy Stardust and the Spiders from Mars (2012 Remaster)"
                        .into(),
                )
            } else {
                Some(format!("Album {i}"))
            },
            ..Default::default()
        })
        .collect();

    app.catalog.rows = Rows::Tracks(ten_tracks);
    terminal
        .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }
    let all_text = lines.join("\n");

    // Verify all ranks 1..=10 rendered in sequential order
    for rank in 1..=10 {
        let rank_str = format!("{:>3}", rank);
        assert!(
            lines
                .iter()
                .any(|l| l.contains(&rank_str) && l.contains(&format!("Top Song {rank}"))),
            "Missing rank {rank} for Top Song {rank}:\n{all_text}"
        );
    }

    // Verify rank 10 doesn't distort alignment (contains " 10 ")
    assert!(
        lines
            .iter()
            .any(|l| l.contains(" 10 ") && l.contains("Top Song 10")),
        "Rank 10 formatting issue:\n{all_text}"
    );

    // Verify track 5 with album: None falls back to "-"
    assert!(
        lines
            .iter()
            .any(|l| l.contains("Top Song 5") && l.contains(" - ")),
        "Track 5 with album: None must render '-' in ALBUM column:\n{all_text}"
    );

    // Verify track 7 with long album name renders without panic and truncates/clips cleanly
    assert!(
        lines
            .iter()
            .any(|l| l.contains("Top Song 7") && l.contains("The Rise and Fall")),
        "Track 7 with long album name should render cleanly:\n{all_text}"
    );

    // Verify header switches to ALBUM instead of ARTIST
    assert!(
        all_text.contains("ALBUM"),
        "Header must have ALBUM: {all_text}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("   #    ") && l.contains("TITLE") && l.contains("ALBUM")),
        "Header row structure check:\n{all_text}"
    );
}

#[test]
fn test_adversarial_width_scaling_and_alignment() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Artist;
    app.catalog.title = "David Bowie • Top Tracks".into();

    let tracks = vec![
        Track {
            id: "1".repeat(22),
            name: "Space Oddity".into(),
            artists: "David Bowie".into(),
            duration_ms: 318_000,
            playable: true,
            album: Some("David Bowie (Space Oddity)".into()),
            ..Default::default()
        },
        Track {
            id: "2".repeat(22),
            name: "Heroes - 2017 Remaster".into(),
            artists: "David Bowie".into(),
            duration_ms: 371_000,
            playable: true,
            album: Some("Heroes (2017 Remaster)".into()),
            ..Default::default()
        },
        Track {
            id: "3".repeat(22),
            name: "Starman - 2012 Remaster".into(),
            artists: "David Bowie".into(),
            duration_ms: 254_000,
            playable: false,
            album: Some("The Rise and Fall of Ziggy Stardust and the Spiders from Mars".into()),
            ..Default::default()
        },
    ];
    app.catalog.rows = Rows::Tracks(tracks);

    // Width test cases:
    // Narrow widths (<= 90 cols): 48, 70, 80, 90
    // Wide widths (> 90 cols): 91, 100, 120, 160
    let test_widths = [48, 70, 80, 90, 91, 100, 120, 160];

    for &width in &test_widths {
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal
            .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut lines = Vec::new();
        for y in 0..buffer.area.height {
            let mut line = String::new();
            for x in 0..buffer.area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line);
        }
        let full_text = lines.join("\n");

        // Verify fundamental elements render at every size without panic or crash
        assert!(
            full_text.contains("Space Oddity"),
            "Width {width}: Space Oddity not found:\n{full_text}"
        );

        if width >= 60 {
            // Check that TITLE column header is present
            assert!(
                full_text.contains("TITLE"),
                "Width {width}: missing TITLE header"
            );
        }

        if width <= 90 {
            // Narrow width checks:
            // Ensure column widths and truncation remain aligned and readable.
            if width == 48 {
                // At width 48, terminal is compact and TIME column is 0 width.
                // Title should still be visible and readable
                assert!(full_text.contains("Space Oddity"));
            } else if width == 80 {
                // At width 80 (sidebar 24 + center 56), center < 60, TIME column is suppressed
                // Title and Album should be readable without overlap
                assert!(full_text.contains("Space Oddity"));
            } else if width == 90 {
                // At width 90 (sidebar 24 + center 66), center >= 60, TIME column is visible
                assert!(full_text.contains("5:18")); // Space Oddity duration
            }
        } else {
            // Wide width checks (> 90 cols):
            // Widths 91, 100, 120, 160
            assert!(full_text.contains("TITLE"));
            assert!(full_text.contains("ALBUM"));
            assert!(full_text.contains("5:18")); // Space Oddity duration
            assert!(full_text.contains("6:11")); // Heroes duration
        }
    }
}

#[test]
fn test_breadcrumb_adversarial_deep_nesting_and_width_fuzzing() {
    let history: Vec<String> = (1..=20).map(|i| format!("Level {:02}", i)).collect();
    let history_refs: Vec<&str> = history.iter().map(|s| s.as_str()).collect();
    let current_title = "Final Destination Album";

    // Test across a full spectrum of widths: 0 to 300
    for width in 0..=300 {
        let trail = format_breadcrumb_trail(history_refs.iter().copied(), current_title, width);
        if width > 0 {
            assert!(
                trail.chars().count() <= width,
                "Width violation at max_width={width}: got len {} with content '{trail}'",
                trail.chars().count()
            );
        } else {
            // max_width == 0 is unbounded contract
            assert_eq!(
                trail,
                format!("{} › {}", history.join(" › "), current_title)
            );
        }

        // At very wide width (>= 260), entire 20-level trail must be present
        if width >= 260 {
            assert!(trail.starts_with("Level 01 › Level 02"));
            assert!(trail.ends_with("Level 20 › Final Destination Album"));
        }

        // At standard 80 cols, must be collapsed and contain ellipsis
        if width == 80 {
            assert!(trail.starts_with("… › "));
            assert!(trail.ends_with("Final Destination Album"));
            assert!(trail.chars().count() <= 80);
        }

        // At narrow 40 cols, must fit
        if width == 40 {
            assert!(trail.chars().count() <= 40);
        }

        // At very narrow 20 cols, must fit
        if width == 20 {
            assert!(trail.chars().count() <= 20);
        }
    }
}

#[test]
fn test_breadcrumb_adversarial_unicode_emojis_rtl_and_extreme_lengths() {
    let extreme_cases = [
        // 1. Extreme length (150+ chars ASCII)
        "The Rise and Fall of Ziggy Stardust and the Spiders from Mars (50th Anniversary Half-Speed Mastered Edition) [2022 Remaster] - Super Deluxe Extended Edition",
        // 2. Japanese (CJK)
        "シン・エヴァンゲリオン劇場版:|| 原声音乐集 • 鷺巣詩郎",
        // 3. Chinese
        "千里江山图 • 故宫博物院院藏古琴音乐合辑",
        // 4. Arabic (RTL)
        "فيروز • أروع ما غنت فيروز في مسرحيات الرحابنة",
        // 5. Hebrew (RTL)
        "שלום עליכם • אלבום מופת ישראלי לכל הזמנים",
        // 6. Cyrillic
        "Чайковский • Лебединое озеро (Полная версия)",
        // 7. Greek
        "Μίκης Θεοδωράκης • Το Άξιον Εστί",
        // 8. Single and Multi-byte emojis
        "🔥 Summer Hits 2026 🌴 🎧 🎵 💃 ✨ 🚀 🏖️",
        // 9. Complex ZWJ sequence emojis (Family, Rainbow Flag, etc.)
        "👨‍👩‍👧‍👦 Family Band 🏳️‍🌈 Pride Anthems 🧑‍💻 Coding Beats",
        // 10. Combining diacritics / Zalgo
        "Ẑa̗ĺğò D́ëât́ḧ Ḿët́âĺ",
        // 11. Empty and single character
        "",
        "X",
        "🎵",
        // 12. Whitespace and punctuation
        "   ",
        "!!! ??? *** / \\ | < > : \" '",
    ];

    let history_cases: &[&[&str]] = &[
        &[],                                     // empty history
        &["Search"],                             // 1-item
        &["Search", "Rock"],                     // 2-item
        &["Search", "Rock", "90s", "Radiohead"], // multi-item
        &["", "Search", "", "Rock", ""],         // empty history elements
        &["🔍 Поиск", "🎸 ロック", "🎶 עִבְרִית"],  // Unicode history
    ];

    let test_widths = [
        0, 1, 2, 3, 4, 5, 10, 15, 20, 25, 30, 40, 50, 80, 100, 120, 160, 200, 300,
    ];

    for history in history_cases {
        for title in extreme_cases {
            for &width in &test_widths {
                let trail = format_breadcrumb_trail(history.iter().copied(), title, width);
                if width > 0 {
                    assert!(
                        trail.chars().count() <= width,
                        "Truncation failed at width={width} for title='{title}': got len {} with result '{trail}'",
                        trail.chars().count()
                    );
                }

                // Verify valid UTF-8 and no panic on byte boundaries
                let _ = trail.as_bytes();
                for c in trail.chars() {
                    let mut b = [0u8; 4];
                    c.encode_utf8(&mut b);
                }
            }
        }
    }
}

#[test]
fn test_breadcrumb_adversarial_full_ui_render_widths() {
    let mut app = App::new(Config::default(), Queue::default());

    // Setup 20 navigation history entries with various titles
    for i in 1..=20 {
        app.push_navigation(format!("Level {:02} 🎵", i));
    }
    app.catalog.view = View::Album;
    app.catalog.title =
        "極道 • The Extreme Length Japanese & Arabic فيروز Album (Deluxe Edition) 🚀".into();
    app.catalog.rows = Rows::Tracks(vec![Track {
        id: "1".repeat(22),
        name: "Test Track 1".into(),
        artists: "Test Artist".into(),
        duration_ms: 200_000,
        playable: true,
        track_number: Some(1),
        ..Default::default()
    }]);

    let test_dimensions = [
        (200, 50), // Ultra wide
        (120, 35), // Wide
        (80, 24),  // Standard
        (40, 20),  // Narrow
        (20, 15),  // Very narrow
        (15, 10),  // Tiny
        (10, 8),   // Extremely tiny
        (5, 5),    // Micro
    ];

    for (w, h) in test_dimensions {
        // Test normal view
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();

        // Test with active filter
        app.catalog.filter = "Track".into();
        app.catalog.filtering = true;
        term.draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();

        // Test with busy loading indicator
        app.catalog.busy = true;
        term.draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();

        // Reset filter/busy
        app.catalog.filter.clear();
        app.catalog.filtering = false;
        app.catalog.busy = false;
    }
}

#[test]
fn test_breadcrumb_history_variations_ui_behavior() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.view = View::Album;
    app.catalog.title = "Current Album".into();

    // 1. Empty history: catalog header displays catalog.title directly without breadcrumb separator
    let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
    term.draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text_empty = term
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text_empty.contains("Current Album"));
    assert!(!text_empty.contains('›'));

    // 2. 1-item history: "Search" -> "Current Album"
    app.push_navigation("Search".into());
    term.draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text_1 = term
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text_1.contains("Search › Current Album"));

    // 3. Multi-item history: "Search" -> "Radiohead" -> "Current Album"
    app.push_navigation("Radiohead".into());
    term.draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text_multi = term
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text_multi.contains("Search › Radiohead › Current Album"));
}

#[test]
fn native_glass_redraw_removes_terminal_side_stale_text() {
    let config = Config {
        theme: "glass".into(),
        native_glass: true,
        ..Config::default()
    };
    let app = App::new(config, Queue::default());
    let mut terminal = Terminal::new(TestBackend::new(100, 35)).unwrap();
    draw_terminal(&mut terminal, &app).unwrap();
    let expected = terminal.backend().buffer().clone();
    // Simulate text retained by terminal reflow outside Ratatui's previous buffer.
    let mut stale = ratatui::buffer::Cell::default();
    stale.set_symbol("X");
    terminal
        .backend_mut()
        .draw([(50, 20, &stale)].into_iter())
        .unwrap();
    assert_ne!(terminal.backend().buffer(), &expected);
    draw_terminal(&mut terminal, &app).unwrap();
    assert_eq!(terminal.backend().buffer(), &expected);
}
