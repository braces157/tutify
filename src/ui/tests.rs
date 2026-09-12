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
