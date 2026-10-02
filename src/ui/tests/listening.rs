use super::*;
use crate::app::listening::{SleepTimer, TOOL_LABELS};

fn text(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn listening_menu_keeps_selected_action_and_apply_close_controls_visible() {
    for theme in [
        "spotify",
        "amber",
        "matrix",
        "cyberpunk",
        "monochrome",
        "glass",
    ] {
        for (width, height) in [(32, 10), (40, 12), (80, 24), (120, 35)] {
            for selected in [0, 4, 8] {
                let mut app = crate::demo::app();
                app.config.theme = theme.into();
                app.config.native_glass = theme == "glass";
                app.ui.overlay = Overlay::ListeningTools;
                app.ui.listening.selected = selected;
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
                    .unwrap();
                let rendered = text(&terminal);
                assert!(
                    rendered.contains(TOOL_LABELS[selected]),
                    "{width}x{height} {theme}: {rendered}"
                );
                assert!(rendered.contains("Enter Apply"), "{rendered}");
                assert!(rendered.contains("Esc Close"), "{rendered}");
                if width >= 58 {
                    assert!(rendered.contains("q Quit"), "{rendered}");
                }
                let render = app.ui.render.borrow();
                assert!(
                    render
                        .mouse_hits
                        .iter()
                        .any(|(_, target)| *target == MouseTarget::ListeningRow(selected))
                );
                assert!(!render.mouse_hits.iter().any(|(_, target)| matches!(
                    target,
                    MouseTarget::Queue(_) | MouseTarget::Catalog(_)
                )));
                for (area, _) in &render.mouse_hits {
                    assert!(area.right() <= width && area.bottom() <= height);
                }
            }
        }
    }
}

#[test]
fn sleep_state_remains_visible_in_small_playback_and_tools_panels() {
    for (width, height) in [(32, 10), (80, 24), (120, 35)] {
        for overlay in [Overlay::None, Overlay::ListeningTools] {
            let mut app = crate::demo::app();
            app.ui.overlay = overlay;
            app.ui.listening.sleep = SleepTimer::Deadline(
                std::time::Instant::now() + std::time::Duration::from_secs(60),
            );
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
                .unwrap();
            assert!(
                text(&terminal).contains("Sleep 1:00"),
                "{}",
                text(&terminal)
            );
            app.ui.listening.sleep = SleepTimer::EndOfTrack(app.generation);
            terminal
                .draw(|f| draw(f, &app, &mut app.ui.render.borrow_mut()))
                .unwrap();
            assert!(text(&terminal).contains("End track"), "{}", text(&terminal));
        }
    }
}
