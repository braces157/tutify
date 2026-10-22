use super::*;
use crate::lyrics::{LyricLine, Lyrics, ReadingMode};

fn lyrics_app(timed: bool) -> App {
    let mut app = App::new(Config::default(), Queue::default());
    let track = Track {
        id: "0".repeat(22),
        name: "Scrolling lyrics".into(),
        artists: "Artist".into(),
        duration_ms: 120_000,
        ..Track::default()
    };
    app.queue.replace(vec![track.id.clone()], 0, false);
    app.cache.insert(track.id.clone(), track);
    app.ui.overlay = Overlay::Lyrics;
    app.lyrics.reading_mode = ReadingMode::Off;
    app.lyrics.content = Some(if timed {
        Lyrics {
            lines: (0..80)
                .map(|i| LyricLine {
                    position_ms: i * 1000,
                    text: format!("VERSE {i:03}"),
                })
                .collect(),
            ..Lyrics::default()
        }
    } else {
        Lyrics {
            plain: Some(
                (0..80)
                    .map(|i| format!("VERSE {i:03}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            ..Lyrics::default()
        }
    });
    app
}

fn render_lyrics(app: &App, width: u16, height: u16, panel: bool) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            if panel {
                lyrics_panel(frame, app, frame.area());
            } else {
                lyrics(frame, app, &mut app.ui.render.borrow_mut(), frame.area());
            }
        })
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn plain_lyrics_auto_scroll_with_progress_in_both_views() {
    for panel in [false, true] {
        let mut app = lyrics_app(false);
        let start = render_lyrics(&app, 48, 10, panel);
        assert!(start.contains("VERSE 000"));
        assert!(!start.contains("VERSE 079"));
        app.queue.position_ms = 60_000;
        let middle = render_lyrics(&app, 48, 10, panel);
        assert!(!middle.contains("VERSE 000"));
        assert!(!middle.contains("VERSE 079"));
        assert!(
            !middle.contains('►'),
            "estimated lyrics must not pretend to be synchronized"
        );
        app.queue.position_ms = 120_000;
        let end = render_lyrics(&app, 48, 10, panel);
        assert!(end.contains("VERSE 079"));
        assert!(!end.contains("VERSE 000"));
        assert!(end.to_lowercase().contains("estimated"));
        if !panel {
            let render = app.ui.render.borrow();
            assert_eq!(render.lyrics_scroll, render.lyrics_length - 1);
        }
    }
}

#[test]
fn lyrics_follow_seeks_and_remain_still_while_paused() {
    for panel in [false, true] {
        let mut app = lyrics_app(false);
        app.queue.position_ms = 90_000;
        let later = render_lyrics(&app, 48, 10, panel);
        app.state = State::Paused;
        assert_eq!(render_lyrics(&app, 48, 10, panel), later);
        app.queue.position_ms = 0;
        let earlier = render_lyrics(&app, 48, 10, panel);
        assert!(earlier.contains("VERSE 000"));
        assert_ne!(earlier, later);
        app.queue.position_ms = u32::MAX;
        assert!(render_lyrics(&app, 48, 10, panel).contains("VERSE 079"));
    }
}

#[test]
fn timestamp_following_is_clamped_and_manual_browsing_stays_in_place() {
    for panel in [false, true] {
        let mut app = lyrics_app(true);
        app.queue.position_ms = 40_000;
        assert!(render_lyrics(&app, 48, 10, panel).contains("► VERSE 040"));
        app.queue.position_ms = 10_000;
        assert!(render_lyrics(&app, 48, 10, panel).contains("► VERSE 010"));
        app.queue.position_ms = 120_000;
        assert!(render_lyrics(&app, 48, 10, panel).contains("► VERSE 079"));
        if !panel {
            let render = app.ui.render.borrow();
            assert_eq!(render.lyrics_scroll, render.lyrics_length - 1);
        }
        app.lyrics.manual_scroll = true;
        app.lyrics.scroll = 5;
        let manual = render_lyrics(&app, 48, 10, panel);
        assert!(manual.contains("VERSE 005"));
        app.queue.position_ms = 60_000;
        assert!(render_lyrics(&app, 48, 10, panel).contains("VERSE 005"));
        app.lyrics.manual_scroll = false;
        assert!(render_lyrics(&app, 48, 10, panel).contains("► VERSE 060"));
    }
}

#[test]
fn estimated_scrolling_counts_wrapped_chinese_and_pinyin_rows_after_resize() {
    let mut app = lyrics_app(false);
    let mut content = Lyrics {
        plain: Some(
            (0..20)
                .map(|i| format!("你好世界你好世界你好世界 {i:02}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        ..Lyrics::default()
    };
    content.prepare_readings();
    app.lyrics.content = Some(content);
    app.lyrics.reading_mode = ReadingMode::Mandarin;
    app.queue.position_ms = 120_000;
    for (width, height) in [(24, 10), (80, 14), (32, 12)] {
        for panel in [false, true] {
            let text = render_lyrics(&app, width, height, panel);
            assert!(
                text.contains("19"),
                "last original lyric should remain visible: {text}"
            );
            assert!(
                text.contains("shì jiè"),
                "last pronunciation should remain visible: {text}"
            );
        }
    }
}

#[test]
fn unknown_duration_stays_at_the_start_until_manually_scrolled() {
    let mut app = lyrics_app(false);
    app.cache.insert(
        "0".repeat(22),
        Track {
            duration_ms: 0,
            ..Track::default()
        },
    );
    app.queue.position_ms = 60_000;
    let text = render_lyrics(&app, 48, 10, false);
    assert!(text.contains("VERSE 000") && text.contains("no timing"));
    app.lyrics.manual_scroll = true;
    app.lyrics.scroll = 20;
    assert!(render_lyrics(&app, 48, 10, false).contains("VERSE 020"));
}
