use super::*;

fn demo_tasks() -> (Tasks, mpsc::UnboundedReceiver<Background>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (Tasks::demo(sender).unwrap(), receiver)
}

fn press(
    app: &mut App,
    tasks: &mut Tasks,
    code: KeyCode,
    commands: &mpsc::UnboundedSender<Command>,
) {
    key(
        app,
        KeyEvent::new(code, KeyModifiers::NONE),
        tasks,
        commands,
    );
}

#[tokio::test]
async fn preview_failure_stale_results_apply_and_undo_preserve_live_queue() {
    let mut app = crate::demo::app();
    let before = app.queue.ids.clone();
    let (mut tasks, mut receiver) = demo_tasks();
    let (commands, _command_rx) = mpsc::unbounded_channel();
    tasks.open_mix(&mut app);
    assert_eq!(app.queue.ids, before, "opening a preview is non-mutating");
    let failure = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, failure);
    assert!(app.mix.recommendation_error.is_some());
    assert!(
        !app.mix.preview.entries.is_empty(),
        "source-only preview remains usable"
    );

    let stale_request = app.mix.recommendation_request;
    tasks.fetch_mix_recommendations(&mut app);
    let success = receiver.recv().await.unwrap();
    app.mix.recommendation_request = app.mix.recommendation_request.wrapping_add(1);
    background(&mut app, &mut tasks, success);
    assert!(
        app.mix.recommendation_candidates.is_empty(),
        "stale results are rejected"
    );
    app.mix.recommendation_request = stale_request.wrapping_add(1);
    tasks.fetch_mix_recommendations(&mut app);
    background(&mut app, &mut tasks, receiver.recv().await.unwrap());
    assert!(!app.mix.recommendation_candidates.is_empty());

    let preview_ids: Vec<_> = app
        .mix
        .preview
        .entries
        .iter()
        .map(|entry| entry.track.id.clone())
        .collect();
    app.apply_mix(false, &commands);
    assert_eq!(app.queue.ids, preview_ids);
    assert!(app.can_undo());
    perform_undo(&mut app, &mut tasks, &commands);
    assert_eq!(app.queue.ids, before);
    assert_eq!(app.state, State::Paused);
}

#[tokio::test]
async fn playlist_partial_state_pins_regenerates_and_recipe_roundtrips() {
    let mut app = crate::demo::app();
    let (mut tasks, _receiver) = demo_tasks();
    let (commands, _command_rx) = mpsc::unbounded_channel();
    app.open_playlist_mix("9".repeat(22), "Partial set".into());
    let request = app.mix.request;
    background(
        &mut app,
        &mut tasks,
        Background::MixPlaylistPage(request, crate::demo::playlist_tracks()[..5].to_vec(), false),
    );
    background(
        &mut app,
        &mut tasks,
        Background::MixPlaylistError(request, "page 2 unavailable".into()),
    );
    assert!(app.mix.source_partial);
    assert!(app.mix.preview.note.contains("partial"));
    app.mix.preview.entries[0].pinned = true;
    let pinned = app.mix.preview.entries[0].clone();
    app.mix.regenerate();
    assert_eq!(app.mix.preview.entries[0], pinned);

    app.mix.recipe_name = "Night commute".into();
    app.save_mix_recipe();
    let dir = tempfile::tempdir().unwrap();
    let store = Storage {
        root: dir.path().to_owned(),
    };
    store.save_mix_recipes(&app.mix_recipes).unwrap();
    let restored = store.mix_recipes().unwrap();
    assert_eq!(restored.recipes[0].name, "Night commute");

    let before = app.queue.ids.clone();
    app.apply_mix(true, &commands);
    assert!(app.queue.ids.len() > before.len());
    store.save_queue(&app.queue).unwrap();
    assert_eq!(store.queue().unwrap().ids, app.queue.ids);
    perform_undo(&mut app, &mut tasks, &commands);
    assert_eq!(app.queue.ids, before);
}

#[tokio::test]
async fn active_view_owns_mix_source_including_filtered_playlist_and_stale_browse() {
    let mut app = crate::demo::app();
    let (mut tasks, mut receiver) = demo_tasks();
    let (commands, _command_rx) = mpsc::unbounded_channel();
    app.catalog.view = View::Playlists;
    app.catalog.browse = Browse::Playlists;
    app.catalog.rows = Rows::Playlists(vec![
        crate::model::Playlist {
            id: "8".repeat(22),
            name: "Ignore me".into(),
            owner: "Owner".into(),
        },
        crate::model::Playlist {
            id: "9".repeat(22),
            name: "Filtered choice".into(),
            owner: "Owner".into(),
        },
    ]);
    app.catalog.filter = "filtered".into();
    app.catalog.selected = 0;
    tasks.open_mix(&mut app);
    assert_eq!(
        app.mix.source,
        Some(MixSource::Playlist {
            id: "9".repeat(22),
            name: "Filtered choice".into()
        })
    );
    background(&mut app, &mut tasks, receiver.recv().await.unwrap());
    let request = app.mix.request;
    app.apply_mix(false, &commands);
    background(
        &mut app,
        &mut tasks,
        Background::MixPlaylistPage(request, crate::demo::playlist_tracks(), true),
    );
    let applied = app.queue.ids.clone();
    app.catalog.browse = Browse::Playlist("7".repeat(22));
    app.catalog.title = "Stale playlist".into();
    tasks.open_mix(&mut app);
    assert_eq!(app.mix.source, Some(MixSource::Queue));
    assert_eq!(app.queue.ids, applied);
}

#[tokio::test]
async fn playlist_recipe_never_retargets_catalog_navigation() {
    for view in [View::Liked, View::Search, View::Queue] {
        let mut app = crate::demo::app();
        let (mut tasks, _receiver) = demo_tasks();
        let (commands, _command_rx) = mpsc::unbounded_channel();
        app.catalog.view = view;
        app.catalog.browse = match view {
            View::Liked => Browse::Liked,
            View::Search => Browse::Search("original query".into()),
            _ => Browse::Search("stale but preserved".into()),
        };
        app.catalog.title = "Original title".into();
        app.catalog.query = "original query".into();
        app.catalog.filter = "original filter".into();
        app.catalog.selected = 3;
        app.catalog.request = 41;
        let expected = (
            app.catalog.view,
            app.catalog.browse.clone(),
            app.catalog.title.clone(),
            app.catalog.query.clone(),
            app.catalog.filter.clone(),
            app.catalog.selected,
            app.catalog.request,
        );
        tasks.open_mix_recipe(
            &mut app,
            MixRecipe {
                name: "Saved playlist".into(),
                source: MixSource::Playlist {
                    id: "9".repeat(22),
                    name: "Elsewhere".into(),
                },
                settings: crate::mix::MixSettings::default(),
            },
        );
        press(&mut app, &mut tasks, KeyCode::Esc, &commands);
        assert_eq!(
            expected,
            (
                app.catalog.view,
                app.catalog.browse.clone(),
                app.catalog.title.clone(),
                app.catalog.query.clone(),
                app.catalog.filter.clone(),
                app.catalog.selected,
                app.catalog.request,
            )
        );
    }
}

#[tokio::test]
async fn empty_apply_keeps_delayed_playlist_job_and_success_rejects_late_results() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
    let server = MockServer::start().await;
    let id = "9".repeat(22);
    Mock::given(path(format!("/playlists/{id}/items")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(75))
                .set_body_json(serde_json::json!({
                    "items": [{"item": {
                        "id": "1".repeat(22), "name": "Delayed", "type": "track",
                        "artists": [{"id": "2".repeat(22), "name": "Artist"}],
                        "duration_ms": 180000, "is_playable": true
                    }}],
                    "next": null
                })),
        )
        .mount(&server)
        .await;
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(Catalog::mock(&server.uri()), sender).unwrap();
    let mut app = crate::demo::app();
    app.demo = false;
    app.catalog.view = View::Playlists;
    app.catalog.browse = Browse::Playlist(id);
    app.catalog.title = "Delayed source".into();
    let (commands, _command_rx) = mpsc::unbounded_channel();
    tasks.open_mix(&mut app);
    press(&mut app, &mut tasks, KeyCode::Enter, &commands);
    assert!(tasks.mix.is_some());
    let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    background(&mut app, &mut tasks, event);
    assert!(!app.mix.preview.entries.is_empty());
    let source_request = app.mix.request;
    let recommendation_request = app.mix.recommendation_request;
    press(&mut app, &mut tasks, KeyCode::Enter, &commands);
    let applied = app.queue.ids.clone();
    background(
        &mut app,
        &mut tasks,
        Background::MixPlaylistPage(source_request, crate::demo::playlist_tracks(), true),
    );
    background(
        &mut app,
        &mut tasks,
        Background::MixRecommendations(recommendation_request, Vec::new(), Some("late".into())),
    );
    assert_eq!(app.queue.ids, applied);
}

#[tokio::test]
async fn demo_supported_actions_stay_on_fixture_paths() {
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let mut tasks = Tasks::demo(sender).unwrap();
    tasks.catalog = Catalog::mock(&server.uri());
    let mut app = crate::demo::app();
    let (commands, _command_rx) = mpsc::unbounded_channel();

    app.catalog.query = "neon".into();
    tasks.view(&mut app, View::Search);
    background(&mut app, &mut tasks, receiver.recv().await.unwrap());
    assert!(app.len() > 0);
    tasks.view(&mut app, View::Playlists);
    background(&mut app, &mut tasks, receiver.recv().await.unwrap());
    tasks.enqueue_playlist(&mut app, "9".repeat(22), "Demo".into());
    background(&mut app, &mut tasks, receiver.recv().await.unwrap());
    tasks.start_radio(
        &mut app,
        crate::demo::playlist_tracks()[0].clone(),
        &commands,
    );
    background(&mut app, &mut tasks, receiver.recv().await.unwrap());
    tasks.cycle_shuffle(&mut app);
    tasks.cycle_shuffle(&mut app);
    if let Some(event) = receiver.recv().await {
        background(&mut app, &mut tasks, event);
    }
    tasks.view(&mut app, View::Queue);
    tasks.open_mix(&mut app);
    background(&mut app, &mut tasks, receiver.recv().await.unwrap());
    app.ui.overlay = Overlay::Lyrics;
    tasks.update_lyrics(&mut app);
    tasks.retry_metadata(&mut app);
    tasks.update_lyrics(&mut app);
    assert!(app.lyrics.content.is_some());
    assert!(app.queue.ids.iter().all(|id| app.cache.get(id).is_some()));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn demo_refreshes_supported_views_and_mix_locally() {
    let (mut tasks, mut receiver) = demo_tasks();
    let (commands, _command_rx) = mpsc::unbounded_channel();
    let mut app = crate::demo::app();
    app.catalog.query = "rain".into();
    for view in [View::Search, View::Playlists, View::Liked] {
        tasks.view(&mut app, view);
        background(&mut app, &mut tasks, receiver.recv().await.unwrap());
        press(&mut app, &mut tasks, KeyCode::F(5), &commands);
        background(&mut app, &mut tasks, receiver.recv().await.unwrap());
        assert!(app.len() > 0);
        assert!(app.queue.ids.iter().all(|id| app.cache.get(id).is_some()));
    }
    tasks.view(&mut app, View::Queue);
    press(&mut app, &mut tasks, KeyCode::F(5), &commands);
    tasks.open_mix(&mut app);
    background(&mut app, &mut tasks, receiver.recv().await.unwrap());
    assert_eq!(app.mix.source, Some(MixSource::Queue));
    assert!(!app.mix.preview.entries.is_empty());
}

#[test]
fn append_capacity_reports_actual_mutation_and_preserves_playback() {
    for available in [0usize, 1, 4, 20] {
        let mut app = crate::demo::app();
        app.open_queue_mix();
        let requested = app.mix.preview.entries.len();
        let initial_len = crate::queue::MAX_TRACKS.saturating_sub(available);
        app.queue
            .replace(vec!["1".repeat(22); initial_len], 0, false);
        app.state = State::Playing;
        app.generation = 7;
        app.queue.position_ms = 12_345;
        let (commands, _command_rx) = mpsc::unbounded_channel();
        app.apply_mix(true, &commands);
        let inserted = available.min(requested);
        assert_eq!(app.queue.ids.len(), initial_len + inserted);
        assert_eq!(app.generation, 7);
        assert_eq!(app.queue.position_ms, 12_345);
        assert_eq!(app.can_undo(), inserted > 0);
        assert!(app.status.contains(&inserted.to_string()));
    }
}

#[test]
fn queue_metadata_updates_every_duplicate_occurrence_and_stale_batches_are_ignored() {
    let mut app = App::new(Config::default(), Queue::default());
    let id = "1".repeat(22);
    app.queue.replace(vec![id.clone(), id.clone()], 0, false);
    app.open_queue_mix();
    assert_eq!(app.mix.source_candidates.len(), 2);
    let request = app.mix.request;
    let mut hydrated = Track::unknown(&id);
    hydrated.name = "Hydrated duplicate".into();
    hydrated.duration_ms = 180_000;
    assert!(app.update_queue_mix_track(hydrated));
    assert_eq!(app.mix.preview.entries.len(), 2);
    app.open_playlist_mix("9".repeat(22), "New source".into());
    let (mut tasks, _receiver) = tasks();
    background(
        &mut app,
        &mut tasks,
        Background::MixMetadataBatch(request, Vec::new(), true),
    );
    assert_eq!(
        app.mix.source,
        Some(MixSource::Playlist {
            id: "9".repeat(22),
            name: "New source".into()
        })
    );
}

#[test]
fn applying_after_a_pin_becomes_unavailable_keeps_newer_cache_metadata() {
    let mut app = crate::demo::app();
    app.open_queue_mix();
    app.mix.preview.entries[0].pinned = true;
    let id = app.mix.preview.entries[0].track.id.clone();
    let mut unavailable = app.mix.preview.entries[0].track.clone();
    unavailable.name = "Unavailable after refresh".into();
    unavailable.playable = false;
    app.cache.insert(id.clone(), unavailable.clone());
    assert!(app.update_queue_mix_track(unavailable.clone()));
    assert!(
        !app.mix
            .preview
            .entries
            .iter()
            .any(|entry| entry.track.id == id)
    );
    assert!(app.mix.preview.note.contains("was unpinned"));
    app.mix.refresh();
    assert!(
        app.mix.preview.note.contains("was unpinned"),
        "later candidate refreshes must not erase the invalid-pin warning"
    );
    let (commands, _receiver) = mpsc::unbounded_channel();
    app.apply_mix(false, &commands);
    assert_eq!(app.cache.get(&id), Some(&unavailable));
    assert!(!app.queue.ids.contains(&id));
}

#[tokio::test]
async fn same_playlist_retry_preserves_source_duplicate_and_recommendation_pins() {
    let mut app = crate::demo::app();
    let live_queue = app.queue.ids.clone();
    let (mut tasks, mut receiver) = demo_tasks();
    let (commands, _command_receiver) = mpsc::unbounded_channel();
    let mut page = crate::demo::playlist_tracks()[..5].to_vec();
    page.insert(1, page[0].clone());
    app.open_playlist_mix("9".repeat(22), "Partial playlist".into());
    let request = app.mix.request;
    background(
        &mut app,
        &mut tasks,
        Background::MixPlaylistPage(request, page, false),
    );
    tasks.fetch_mix_recommendations(&mut app);
    background(&mut app, &mut tasks, receiver.recv().await.unwrap());
    tasks.fetch_mix_recommendations(&mut app);
    background(&mut app, &mut tasks, receiver.recv().await.unwrap());
    assert!(!app.mix.recommendation_candidates.is_empty());
    app.mix.refresh();
    let source_position = app
        .mix
        .preview
        .entries
        .iter()
        .position(|entry| matches!(entry.provenance, Provenance::Source { .. }))
        .unwrap();
    let recommendation_position = app
        .mix
        .preview
        .entries
        .iter()
        .position(|entry| matches!(entry.provenance, Provenance::Recommendation { .. }))
        .unwrap();
    app.mix.preview.entries[source_position].pinned = true;
    app.mix.preview.entries[recommendation_position].pinned = true;
    let source_pin = app.mix.preview.entries[source_position].clone();
    let recommendation_pin = app.mix.preview.entries[recommendation_position].clone();
    background(
        &mut app,
        &mut tasks,
        Background::MixPlaylistError(request, "503 on page two".into()),
    );
    press(&mut app, &mut tasks, KeyCode::Char('g'), &commands);
    let retry_request = app.mix.request;
    assert_eq!(app.mix.preview.entries[source_position], source_pin);
    assert_eq!(
        app.mix.preview.entries[recommendation_position],
        recommendation_pin
    );
    background(
        &mut app,
        &mut tasks,
        Background::MixPlaylistPage(retry_request, crate::demo::playlist_tracks(), true),
    );
    while let Ok(event) = receiver.try_recv() {
        background(&mut app, &mut tasks, event);
    }
    assert_eq!(
        app.mix.preview.entries[source_position].track.id,
        source_pin.track.id
    );
    assert!(app.mix.preview.entries[source_position].pinned);
    assert_eq!(
        app.mix.preview.entries[recommendation_position].track.id,
        recommendation_pin.track.id
    );
    assert!(app.mix.preview.entries[recommendation_position].pinned);
    background(
        &mut app,
        &mut tasks,
        Background::MixPlaylistPage(request, Vec::new(), true),
    );
    assert_eq!(
        app.mix.request, retry_request,
        "old source response was rejected"
    );
    press(&mut app, &mut tasks, KeyCode::Esc, &commands);
    assert_eq!(app.queue.ids, live_queue);
}

#[tokio::test]
async fn delayed_two_page_playlist_failure_retry_preserves_pin_and_rejects_old_page() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate, matchers::path};

    #[derive(Clone)]
    struct PlaylistRetry {
        calls: Arc<AtomicUsize>,
    }
    impl Respond for PlaylistRetry {
        fn respond(&self, _request: &Request) -> ResponseTemplate {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call == 1 {
                return ResponseTemplate::new(503).set_delay(Duration::from_millis(40));
            }
            let first_page = call == 0 || call == 2;
            let number = if first_page { 1 } else { 2 };
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{"item": {
                    "id": format!("{number:022}"), "name": format!("Page {number}"),
                    "type": "track", "artists": [{"id": "7".repeat(22), "name": "Artist"}],
                    "duration_ms": 180000, "is_playable": true
                }}],
                "next": if first_page { serde_json::Value::String("more".into()) } else { serde_json::Value::Null }
            }))
        }
    }

    let server = MockServer::start().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let playlist_id = "9".repeat(22);
    Mock::given(path(format!("/playlists/{playlist_id}/items")))
        .respond_with(PlaylistRetry {
            calls: calls.clone(),
        })
        .mount(&server)
        .await;
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(Catalog::mock(&server.uri()), sender).unwrap();
    let mut app = crate::demo::app();
    let live_queue = app.queue.ids.clone();
    let (commands, _command_receiver) = mpsc::unbounded_channel();
    app.open_playlist_mix(playlist_id, "Retry fixture".into());
    tasks.start_playlist_mix_load(&app, "9".repeat(22));
    let first = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, first);
    let failure = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, failure);
    assert!(app.mix.source_retryable);
    app.mix.preview.entries[0].pinned = true;
    let pin = app.mix.preview.entries[0].clone();
    press(&mut app, &mut tasks, KeyCode::Char('g'), &commands);
    let retry_request = app.mix.request;
    background(&mut app, &mut tasks, receiver.recv().await.unwrap());
    background(&mut app, &mut tasks, receiver.recv().await.unwrap());
    assert_eq!(app.mix.preview.entries[0], pin);
    background(
        &mut app,
        &mut tasks,
        Background::MixPlaylistPage(retry_request.wrapping_sub(1), Vec::new(), true),
    );
    assert_eq!(app.mix.request, retry_request);
    press(&mut app, &mut tasks, KeyCode::Esc, &commands);
    assert_eq!(app.queue.ids, live_queue);
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}

#[test]
fn recipe_editor_resets_detail_scroll_and_preserves_failed_or_cancelled_text() {
    let mut app = crate::demo::app();
    let (mut tasks, _receiver) = demo_tasks();
    let (commands, _command_receiver) = mpsc::unbounded_channel();
    app.open_queue_mix();
    press(&mut app, &mut tasks, KeyCode::Char('?'), &commands);
    for _ in 0..12 {
        press(&mut app, &mut tasks, KeyCode::Down, &commands);
    }
    press(&mut app, &mut tasks, KeyCode::Char('w'), &commands);
    assert!(app.mix.naming);
    assert_eq!(app.mix.detail_scroll, 0);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| ui::draw(frame, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Recipe name: My mix_"), "{text}");
    press(&mut app, &mut tasks, KeyCode::Backspace, &commands);
    let cancelled_name = app.mix.recipe_name.clone();
    press(&mut app, &mut tasks, KeyCode::Esc, &commands);
    assert!(!app.mix.naming);
    assert_eq!(app.mix.recipe_name, cancelled_name);

    for index in 0..100 {
        assert_eq!(
            app.mix_recipes.save(MixRecipe {
                name: format!("Full {index}"),
                source: MixSource::Queue,
                settings: crate::mix::MixSettings::default(),
            }),
            crate::mix::RecipeSave::Added
        );
    }
    app.mix.naming = true;
    app.mix.recipe_name = "Capacity name stays visible".into();
    press(&mut app, &mut tasks, KeyCode::Enter, &commands);
    assert!(app.mix.naming);
    assert_eq!(app.mix.recipe_name, "Capacity name stays visible");
    assert!(app.status.contains("Recipe limit"));
}

async fn hydration_request_count(status: u16, retry_after: bool) -> (usize, App) {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
    let server = MockServer::start().await;
    let mut response = ResponseTemplate::new(status);
    if retry_after {
        response = response.insert_header("retry-after", "30");
    }
    Mock::given(method("GET"))
        .respond_with(response)
        .mount(&server)
        .await;
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(Catalog::mock(&server.uri()), sender).unwrap();
    let mut app = App::new(Config::default(), Queue::default());
    app.queue.replace(
        (0..20).map(|index| format!("{index:022}")).collect(),
        0,
        false,
    );
    tasks.open_mix(&mut app);
    let event = tokio::time::timeout(Duration::from_secs(3), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event, Background::MixMetadataBatch(_, _, true)));
    background(&mut app, &mut tasks, event);
    let count = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path().starts_with("/tracks/"))
        .count();
    (count, app)
}

#[tokio::test]
async fn queue_hydration_bounds_systemic_failures_and_summarizes_item_failures() {
    for (status, retry_after) in [(503, false), (429, true), (401, false)] {
        let (count, app) = hydration_request_count(status, retry_after).await;
        let bound = if status == 401 { 10 } else { 5 };
        assert!(count <= bound, "HTTP {status} scheduled {count} requests");
        assert!(app.mix.source_retryable);
        assert!(app.mix.source_error.as_deref().unwrap().len() < 500);
    }
    let (count, app) = hydration_request_count(404, false).await;
    assert_eq!(
        count, 20,
        "individual missing tracks must not stop hydration"
    );
    assert!(
        app.mix
            .source_error
            .as_deref()
            .unwrap()
            .contains("20 queue tracks")
    );
}

#[tokio::test]
async fn queue_hydration_network_failure_is_retryable_and_cancellation_is_stale_safe() {
    let (mut tasks, mut receiver) = tasks();
    let mut app = App::new(Config::default(), Queue::default());
    app.queue.replace(
        (0..20).map(|index| format!("{index:022}")).collect(),
        0,
        false,
    );
    tasks.open_mix(&mut app);
    let event = tokio::time::timeout(Duration::from_secs(3), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    background(&mut app, &mut tasks, event);
    assert!(app.mix.source_retryable);
    let stale = app.mix.request;
    tasks.cancel_mix(&mut app);
    background(
        &mut app,
        &mut tasks,
        Background::MixMetadataBatch(stale, Vec::new(), true),
    );
    assert_ne!(app.mix.request, stale);
    assert!(!app.mix.loading_source);
}

#[tokio::test]
async fn playlist_candidate_handler_caps_and_labels_coverage() {
    let mut app = crate::demo::app();
    app.open_playlist_mix("9".repeat(22), "Huge".into());
    let request = app.mix.request;
    let repeated = crate::demo::playlist_tracks()[0].clone();
    let tracks = vec![repeated; crate::mix::MAX_SOURCE_CANDIDATES + 1];
    let (mut tasks, _receiver) = tasks();
    background(
        &mut app,
        &mut tasks,
        Background::MixPlaylistPage(request, tracks, true),
    );
    assert_eq!(
        app.mix.source_candidates.len(),
        crate::mix::MAX_SOURCE_CANDIDATES
    );
    assert!(app.mix.source_partial);
    assert!(app.mix.source_error.as_deref().unwrap().contains("capped"));
}
