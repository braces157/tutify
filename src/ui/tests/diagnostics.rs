use super::*;
use crate::diagnostics::{history::Subsystem, support::SupportReport};

#[test]
fn diagnostics_panels_render_and_scroll_at_minimum_sizes_in_every_theme() {
    for theme in [
        "spotify",
        "amber",
        "matrix",
        "cyberpunk",
        "monochrome",
        "glass",
    ] {
        for (width, height) in [(32, 10), (60, 18), (80, 24), (120, 35)] {
            let mut app = App::new(Config::default(), Queue::default());
            app.config.theme = theme.into();
            app.ui.overlay = Overlay::Diagnostics;
            app.status =
                "private-fixture-secret untrusted old status C:/Users/Private Person".into();
            for _ in 0..70 {
                app.ui
                    .diagnostics
                    .history
                    .record_text(Subsystem::Catalog, "private-fixture-secret HTTP 403");
            }
            app.ui.diagnostics.selected = usize::MAX;
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
            assert!(
                text.contains("RECENT") && text.contains("ERRORS"),
                "{width}x{height} {theme}: {text}"
            );
            assert!(!text.contains("private-fixture-secret"));
            assert!(!text.contains("Private Person"));
            assert!(app.ui.render.borrow().diagnostics_scroll > 0);
            let report = SupportReport::new(
                &app.ui.diagnostics.history,
                crate::catalog::CapabilitySummary::unknown(),
                vec![],
            );
            app.ui.diagnostics.preview_json = report.json().unwrap();
            app.ui.diagnostics.preview = Some(report);
            app.ui.diagnostics.scroll = usize::MAX;
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
            assert!(
                text.contains("SUPPORT") && text.contains("REPORT"),
                "{text}"
            );
            assert!(!text.contains("private-fixture-secret"));
            assert!(app.ui.render.borrow().diagnostics_length > 1);
        }
    }
}
