use super::*;
use crate::{
    diagnostics::{
        history::{Cause, Subsystem},
        support::SupportReport,
    },
    service::{FailureKind, ServiceFailure},
};

#[tokio::test]
async fn failed_browse_survives_volume_success_status_and_report_preview() {
    let (mut tasks, _events) = tasks();
    let mut app = App::new(Config::default(), Queue::default());
    let (commands, mut receiver) = mpsc::unbounded_channel();
    let failure = ServiceFailure {
        status: Some(403),
        ..ServiceFailure::spotify(FailureKind::AccessRestricted)
    };
    let request = app.catalog.request;
    background(
        &mut app,
        &mut tasks,
        Background::Page(
            request,
            Err(anyhow::Error::new(failure)
                .context("private token C:/Users/Private Person/Secret Song")),
        ),
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    app.playback_event(Event::Volume(75), &commands);
    app.status = "An unrelated successful update".into();
    assert_eq!(app.ui.diagnostics.history.records().len(), 1);
    assert_eq!(
        app.ui.diagnostics.history.records()[0].cause,
        Cause::Service(FailureKind::AccessRestricted)
    );
    while receiver.try_recv().is_ok() {}
    key(
        &mut app,
        KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    assert_eq!(app.ui.overlay, Overlay::Diagnostics);
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    assert!(app.ui.diagnostics.exported_name.is_none());
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    let preview = app.ui.diagnostics.preview_json.clone();
    assert!(preview.contains("access_restricted"));
    for secret in ["private token", "Private Person", "Secret Song"] {
        assert!(!preview.contains(secret));
    }
    background(
        &mut app,
        &mut tasks,
        Background::SaveError("private filesystem path".into()),
    );
    assert_eq!(app.ui.diagnostics.history.records().len(), 2);
    assert_eq!(app.ui.diagnostics.preview_json, preview);
    let temporary = tempfile::tempdir().unwrap();
    let destination = temporary.path().join("reviewed.json");
    app.ui
        .diagnostics
        .preview
        .as_ref()
        .unwrap()
        .export(&destination, None)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(destination).unwrap().trim_end(),
        preview
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    assert_eq!(app.ui.overlay, Overlay::None);
    assert!(!app.quit);
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn stale_job_errors_are_not_recorded_and_sessions_are_isolated() {
    let (mut tasks, _events) = tasks();
    let mut app = App::new(Config::default(), Queue::default());
    let request = app.catalog.request;
    background(
        &mut app,
        &mut tasks,
        Background::Page(
            request.wrapping_add(1),
            Err(anyhow::anyhow!("stale failure")),
        ),
    );
    assert!(app.ui.diagnostics.history.records().is_empty());
    background(
        &mut app,
        &mut tasks,
        Background::LibraryDone(request, Err(anyhow::anyhow!("private library failure"))),
    );
    assert_eq!(
        app.ui.diagnostics.history.records()[0].subsystem,
        Subsystem::Library
    );
    assert!(
        App::new(Config::default(), Queue::default())
            .ui
            .diagnostics
            .history
            .records()
            .is_empty()
    );
    let report = SupportReport::new(
        &app.ui.diagnostics.history,
        tasks.catalog.capability_summary(),
        vec![],
    );
    assert!(!report.json().unwrap().contains("private library"));
}

#[tokio::test]
async fn diagnostics_mouse_keyboard_and_paste_do_not_modify_queue_or_start_music() {
    let (mut tasks, _events) = tasks();
    let mut app = App::new(Config::default(), Queue::default());
    app.queue
        .replace(vec!["0".repeat(22), "1".repeat(22)], 0, false);
    app.ui
        .diagnostics
        .history
        .record_text(Subsystem::Storage, "first");
    app.ui
        .diagnostics
        .history
        .record_text(Subsystem::Playback, "second");
    let original = serde_json::to_string(&app.queue).unwrap();
    let (commands, mut receiver) = mpsc::unbounded_channel();
    key(
        &mut app,
        KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    draw_mouse(&app, 80, 24);
    click_target(
        &mut app,
        MouseTarget::DiagnosticRow(1),
        MouseButton::Left,
        &mut tasks,
        &commands,
    );
    assert_eq!(app.ui.diagnostics.selected, 1);
    assert!(route_input(
        &mut app,
        Input::Paste("private pasted query".into()),
        &mut tasks,
        &commands
    ));
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    assert!(app.ui.diagnostics.preview.is_none());
    click_target(
        &mut app,
        MouseTarget::DiagnosticKey(KeyCode::Char('r')),
        MouseButton::Left,
        &mut tasks,
        &commands,
    );
    assert!(app.ui.diagnostics.preview.is_some());
    draw_mouse(&app, 32, 10);
    key(
        &mut app,
        KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    assert!(app.ui.diagnostics.scroll > 0);
    key(
        &mut app,
        KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    assert_eq!(app.ui.overlay, Overlay::None);
    assert_eq!(serde_json::to_string(&app.queue).unwrap(), original);
    assert!(app.catalog.query.is_empty());
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn diagnostic_shortcut_does_not_interrupt_text_entry() {
    let (mut tasks, _events) = tasks();
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.editing = true;
    let (commands, _receiver) = mpsc::unbounded_channel();
    key(
        &mut app,
        KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    assert_eq!(app.ui.overlay, Overlay::None);
    assert!(app.catalog.editing);
}

#[tokio::test]
async fn reviewed_report_export_key_writes_exact_preview_and_preserves_session() {
    let (mut tasks, _events) = tasks();
    let mut app = App::new(Config::default(), Queue::default());
    app.ui
        .diagnostics
        .history
        .record_text(Subsystem::Playback, "private-fixture-secret");
    let (commands, mut receiver) = mpsc::unbounded_channel();
    key(
        &mut app,
        KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    let preview = app.ui.diagnostics.preview_json.clone();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    let name = app.ui.diagnostics.exported_name.as_ref().unwrap();
    assert!(name.starts_with("tuitify-support-") && name.ends_with(".json"));
    let target = std::env::current_dir().unwrap().join(name);
    let exported = std::fs::read_to_string(&target).unwrap();
    assert_eq!(exported.trim_end(), preview);
    assert!(!exported.contains("private-fixture-secret"));
    // This test owns this exact newly-created, byte-verified file; remove only it.
    std::fs::remove_file(target).unwrap();
    assert_eq!(app.ui.diagnostics.history.records().len(), 1);
    assert!(receiver.try_recv().is_err());
}
