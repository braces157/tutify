use super::*;

mod jobs;
mod mouse;
mod overlays;
mod persistence;
mod queue;
mod refactor;
mod smart_shuffle;
mod state;

fn tasks() -> (Tasks, mpsc::UnboundedReceiver<Background>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        Tasks::new(Catalog::mock("http://127.0.0.1:1"), tx).unwrap(),
        rx,
    )
}
fn test_track(i: usize) -> Track {
    Track {
        id: format!("{i:022}"),
        name: format!("Song {i}"),
        artists: "Artist".into(),
        duration_ms: 200000,
        playable: true,
        ..Default::default()
    }
}

fn draw_mouse(app: &App, width: u16, height: u16) {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| ui::draw(frame, app, &mut app.ui.render.borrow_mut()))
        .unwrap();
}
fn click_target(
    app: &mut App,
    target: MouseTarget,
    button: MouseButton,
    tasks: &mut Tasks,
    tx: &mpsc::UnboundedSender<Command>,
) {
    let area = app
        .ui
        .render
        .borrow()
        .mouse_hits
        .iter()
        .find(|(_, hit)| *hit == target)
        .unwrap()
        .0;
    assert!(mouse(
        app,
        MouseEvent {
            kind: MouseEventKind::Down(button),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::NONE
        },
        tasks,
        tx
    ));
}
