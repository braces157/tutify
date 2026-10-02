use super::*;
use crate::library::{LibraryProgress, PlaylistSkipReason, SkippedPlaylist};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

fn skipped(index: usize) -> SkippedPlaylist {
    SkippedPlaylist {
        id: format!("{index:022}"),
        name: format!("Saved 日本語 {index}"),
        reason: PlaylistSkipReason::Restricted,
    }
}

fn buffer_text(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| ui::draw(f, app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}

#[tokio::test]
async fn partial_coverage_survives_navigation_and_cancellation_and_stale_skips_are_ignored() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    app.catalog.search_scope = SearchScope::Library;
    app.catalog.busy = true;
    app.catalog.request = 10;
    background(
        &mut app,
        &mut tasks,
        Background::LibraryProgress(
            10,
            LibraryProgress {
                tracks: vec![test_track(1)],
                scanned: 100,
                skipped: vec![skipped(11)],
                complete: false,
            },
        ),
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(!app.catalog.busy);
    assert_eq!(app.catalog.library_skipped.len(), 1);
    assert!(app.status.contains("1 playlists skipped"));
    background(
        &mut app,
        &mut tasks,
        Background::LibraryProgress(
            10,
            LibraryProgress {
                tracks: vec![test_track(2)],
                scanned: 200,
                skipped: vec![skipped(12)],
                complete: false,
            },
        ),
    );
    assert_eq!(app.catalog.library_skipped.len(), 1);
    assert_eq!(app.catalog.library_scanned, 100);
    app.catalog.busy = true;
    app.catalog.title = "Saved library — scanning".into();
    app.push_navigation("Search".into());
    app.catalog.view = View::Album;
    app.catalog.library_skipped = Arc::new(Vec::new());
    app.catalog.library_scanned = 0;
    assert!(app.pop_navigation());
    assert_eq!(app.catalog.search_scope, SearchScope::Library);
    assert_eq!(app.catalog.library_skipped.as_ref(), &vec![skipped(11)]);
    assert_eq!(app.catalog.library_scanned, 100);
    assert_eq!(app.len(), 1);
    assert!(!app.catalog.busy);
    assert!(app.catalog.title.contains("partial"));
    assert!(!app.catalog.title.contains("scanning"));
    app.catalog.query.clear();
    choose_search(&mut app, SearchScope::Spotify, &mut tasks);
    assert!(app.catalog.library_skipped.is_empty());
    assert_eq!(app.catalog.library_scanned, 0);
}

#[tokio::test]
async fn source_details_scroll_without_playback_commands_and_fit_small_terminals() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, mut commands) = mpsc::unbounded_channel();
    app.catalog.search_scope = SearchScope::Library;
    app.catalog.library_skipped = Arc::new((0..100).map(skipped).collect());
    app.catalog.title = "Saved library — partial coverage".into();
    // Persistent coverage cannot disappear when a transient status changes.
    app.status = "Volume 50%".into();
    assert!(buffer_text(&app, 100, 30).contains("100 skipped"));
    click_target(
        &mut app,
        MouseTarget::LibraryCoverage,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.overlay, Overlay::LibraryCoverage);
    key(
        &mut app,
        KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.coverage_selected, 99);
    let text = buffer_text(&app, 100, 30);
    // Wide terminal cells include spacer cells in the flattened buffer.
    assert!(text.replace(' ', "").contains("Saved日本語99"), "{text}");
    assert!(text.contains("Access restricted"));
    for size in [(32, 10), (48, 18), (80, 24), (120, 35)] {
        let text = buffer_text(&app, size.0, size.1);
        assert!(
            text.contains("99"),
            "last source must remain reachable at {size:?}: {text}"
        );
        for (rect, _) in &app.ui.render.borrow().mouse_hits {
            assert!(rect.right() <= size.0 && rect.bottom() <= size.1);
        }
    }
    app.catalog.editing = true;
    let query = app.catalog.query.clone();
    assert!(route_input(
        &mut app,
        Input::Paste("hidden text".into()),
        &mut tasks,
        &tx
    ));
    assert_eq!(app.catalog.query, query);
    for code in [
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Char(' '),
        KeyCode::Char('n'),
    ] {
        key(
            &mut app,
            KeyEvent::new(code, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
    }
    assert!(commands.try_recv().is_err());
    assert!(!app.quit);
    click_target(
        &mut app,
        MouseTarget::CoverageClose,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.overlay, Overlay::None);
    assert_eq!(app.catalog.library_skipped.len(), 100);
}

async fn mount_library(server: &MockServer, restricted: bool) {
    let track = |index: usize| json!({"id":format!("{index:022}"),"name":"Target Song","type":"track","is_playable":true});
    Mock::given(path("/me/tracks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items":[{"track":track(1)}],"next":null
        })))
        .expect(1)
        .mount(server)
        .await;
    Mock::given(path("/me/playlists")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "items":[{"id":format!("{:022}",11),"name":"Restricted"},{"id":format!("{:022}",12),"name":"Later"}],"next":null
    }))).expect(1).mount(server).await;
    for index in [11, 12] {
        let response = if index == 11 && restricted {
            ResponseTemplate::new(403)
        } else {
            ResponseTemplate::new(200)
                .set_body_json(json!({"items":[{"item":track(index)}],"next":null}))
        };
        Mock::given(path(format!("/playlists/{index:022}/items")))
            .respond_with(response)
            .expect(1)
            .mount(server)
            .await;
    }
}

async fn finish_scan(
    app: &mut App,
    tasks: &mut Tasks,
    rx: &mut mpsc::UnboundedReceiver<Background>,
) {
    while app.catalog.busy {
        let event = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .unwrap()
            .unwrap();
        background(app, tasks, event);
    }
}

#[tokio::test]
async fn library_job_reports_partial_coverage_and_f5_rechecks_a_previously_denied_source() {
    let server = MockServer::start().await;
    mount_library(&server, true).await;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(Catalog::mock(&server.uri()), tx).unwrap();
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.query = "Target".into();
    app.catalog.search_scope = SearchScope::Library;
    tasks.request(&mut app, 0);
    finish_scan(&mut app, &mut tasks, &mut rx).await;
    assert_eq!(app.len(), 2);
    assert_eq!(app.catalog.library_skipped.len(), 1);
    assert!(app.catalog.title.contains("partial coverage"));
    assert!(!app.catalog.title.contains("complete"));
    assert!(app.status.contains("Scan finished"));
    server.verify().await;
    server.reset().await;
    mount_library(&server, false).await;
    let (commands, mut command_rx) = mpsc::unbounded_channel();
    key(
        &mut app,
        KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    assert_eq!(app.ui.overlay, Overlay::LibraryCoverage);
    key(
        &mut app,
        KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    assert!(app.catalog.library_skipped.is_empty());
    assert_eq!(app.ui.overlay, Overlay::None);
    finish_scan(&mut app, &mut tasks, &mut rx).await;
    assert_eq!(app.len(), 3);
    assert!(app.catalog.library_skipped.is_empty());
    assert_eq!(app.catalog.title, "Saved library — complete");
    assert!(command_rx.try_recv().is_err());
}
