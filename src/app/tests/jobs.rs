use super::*;

#[tokio::test]
async fn radio_error_survives_playback_updates_and_is_cleared_on_restart() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _rx) = mpsc::unbounded_channel();
    tasks.start_radio(&mut app, test_track(1), &tx);
    let epoch = app.queue.epoch;
    background(
        &mut app,
        &mut tasks,
        Background::Recommendations(epoch, Err(anyhow::anyhow!("similarity unavailable"))),
    );
    app.playback_event(
        Event::Playing {
            generation: app.generation,
            position_ms: 2000,
        },
        &tx,
    );
    assert_eq!(app.radio_error.as_deref(), Some("similarity unavailable"));
    assert!(!tasks.radio_active);
    tasks.start_radio(&mut app, test_track(1), &tx);
    assert!(app.radio_error.is_none());
    tasks.cancel_radio(&mut app);
}

#[tokio::test]
async fn search_starts_radio_and_manual_additions_precede_suggestions() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    app.catalog.rows = Rows::Tracks(vec![test_track(1), test_track(2)]);
    app.catalog.selected = 1;
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.queue.ids, vec![test_track(2).id]);
    assert_eq!(app.radio_epoch, Some(app.queue.epoch));
    assert!(tasks.radio_active);
    let epoch = app.queue.epoch;
    background(
        &mut app,
        &mut tasks,
        Background::Recommendations(
            epoch,
            Ok(Recommendations {
                tracks: vec![test_track(3), test_track(4)],
                source: crate::catalog::RecommendationSource::Spotify,
            }),
        ),
    );
    assert!(app.status.contains("Spotify recommendations"));
    assert!(app.enqueue_manual(test_track(5).id));
    assert!(app.enqueue_manual(test_track(6).id));
    let ordered: Vec<_> = app
        .queue
        .order
        .iter()
        .map(|&i| app.queue.ids[i].clone())
        .collect();
    assert_eq!(ordered, [2, 5, 6, 3, 4].map(|i| test_track(i).id));
    assert_eq!(app.queue.current(), Some(test_track(2).id.as_str()));
    app.queue.validate().unwrap();
}

#[tokio::test]
async fn radio_refill_is_bounded_and_cancelled_with_queue() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    tasks.start_radio(&mut app, test_track(1), &tx);
    let epoch = app.queue.epoch;
    background(
        &mut app,
        &mut tasks,
        Background::Recommendations(
            epoch,
            Ok(Recommendations {
                tracks: vec![test_track(2)],
                source: crate::catalog::RecommendationSource::ArtistSearch,
            }),
        ),
    );
    app.state = State::Playing;
    tasks.refill_radio(&app);
    assert!(tasks.recommendations.is_none()); // Already requested for this seed.
    app.queue.select(1);
    tasks.refill_radio(&app);
    assert!(tasks.recommendations.is_some());
    assert_eq!(tasks.radio_attempted.len(), 2);
    tasks.refill_radio(&app);
    assert_eq!(tasks.radio_attempted.len(), 2);
    background(
        &mut app,
        &mut tasks,
        Background::Recommendations(
            epoch,
            Ok(Recommendations {
                tracks: vec![],
                source: crate::catalog::RecommendationSource::ArtistSearch,
            }),
        ),
    );
    assert!(!tasks.radio_active);
    tasks.refill_radio(&app);
    assert!(tasks.recommendations.is_none());
    app.queue.clear();
    tasks.sync_queue_epoch(app.queue.epoch);
    assert!(!tasks.radio_active);
    assert!(tasks.radio_attempted.is_empty());
}

#[tokio::test]
async fn liked_playback_keeps_list_order_without_radio() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    app.catalog.view = View::Liked;
    app.catalog.rows = Rows::Tracks(vec![test_track(1), test_track(2)]);
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.queue.ids, [test_track(1).id, test_track(2).id]);
    assert!(!tasks.radio_active);
    assert!(tasks.recommendations.is_none());
}
#[tokio::test]
async fn cold_restore_lyrics_wait_for_real_metadata_and_explicit_open() {
    let (mut tasks, _) = tasks();
    let mut q = Queue::default();
    q.replace(vec![test_track(1).id], 0, false);
    let mut app = App::new(Config::default(), q);
    app.ui.overlay = Overlay::Lyrics;
    tasks.update_lyrics(&mut app);
    assert!(!app.lyrics.loading);
    assert!(tasks.lyrics.is_none());
    app.ui.close(Overlay::Lyrics);
    app.cache.insert(test_track(1).id, test_track(1));
    tasks.update_lyrics(&mut app);
    assert!(tasks.lyrics.is_none());
    app.ui.overlay = Overlay::Lyrics;
    tasks.update_lyrics(&mut app);
    assert!(app.lyrics.loading);
    // Drop cancels before the mock-independent task gets a chance to access Lrclib.
    tasks.lyrics.take().unwrap().abort();
    let request = app.lyrics.request;
    background(
        &mut app,
        &mut tasks,
        Background::Lyrics(request, Err(anyhow::anyhow!("offline"))),
    );
    assert!(app.lyrics.error.as_ref().unwrap().contains("offline"));
    tasks.retry_metadata(&mut app);
    assert!(app.lyrics.error.is_none());
    app.queue.clear();
    tasks.update_lyrics(&mut app);
    assert!(app.lyrics.track_id.is_none());
    assert!(app.lyrics.content.is_none());
}
#[tokio::test]
async fn stale_background_queue_and_lyrics_results_are_rejected() {
    let (mut tasks, _) = tasks();
    let mut app = App::new(Config::default(), Queue::default());
    app.queue.replace(vec![test_track(0).id], 0, false);
    let old = app.queue.epoch;
    app.queue.clear();
    background(
        &mut app,
        &mut tasks,
        Background::Recommendations(
            old,
            Ok(Recommendations {
                tracks: vec![test_track(1)],
                source: crate::catalog::RecommendationSource::Spotify,
            }),
        ),
    );
    background(
        &mut app,
        &mut tasks,
        Background::PlaylistPage(old, 0, vec![test_track(1)], true),
    );
    assert!(app.queue.ids.is_empty());
    app.lyrics.request = 2;
    background(
        &mut app,
        &mut tasks,
        Background::Lyrics(1, Ok(Some(crate::lyrics::Lyrics::default()))),
    );
    assert!(app.lyrics.content.is_none());
    let epoch = app.queue.epoch;
    app.radio_epoch = Some(epoch);
    background(
        &mut app,
        &mut tasks,
        Background::Recommendations(
            epoch,
            Ok(Recommendations {
                tracks: vec![test_track(1), test_track(1)],
                source: crate::catalog::RecommendationSource::Spotify,
            }),
        ),
    );
    assert_eq!(app.queue.ids.len(), 1);
}
#[tokio::test]
async fn metadata_failure_is_visible_and_f5_allows_retry() {
    let (mut tasks, _) = tasks();
    let mut app = App::new(Config::default(), Queue::default());
    let id = test_track(1).id;
    app.queue.replace(vec![id.clone()], 0, false);
    tasks.requested.insert(id.clone());
    background(
        &mut app,
        &mut tasks,
        Background::Metadata(0, id.clone(), Err(anyhow::anyhow!("HTTP 503"))),
    );
    assert!(app.status.contains("503"));
    assert!(tasks.metadata_blocked);
    assert!(!tasks.requested.contains(&id));
    app.cache.insert(id.clone(), test_track(1));
    tasks.retry_metadata(&mut app);
    assert!(!tasks.metadata_blocked);
    assert!(app.metadata_error.is_none());
    assert!(app.cache.get(&id).is_none());
    background(
        &mut app,
        &mut tasks,
        Background::Metadata(0, id.clone(), Ok(test_track(1))),
    );
    assert!(app.cache.get(&id).is_none()); // Response predates F5.
}
#[tokio::test]
async fn missing_track_does_not_block_other_metadata() {
    let (mut tasks, _) = tasks();
    let mut app = App::new(Config::default(), Queue::default());
    let id = test_track(1).id;
    background(
        &mut app,
        &mut tasks,
        Background::Metadata(0, id.clone(), Err(crate::catalog::MissingItem.into())),
    );
    assert!(!tasks.metadata_blocked);
    assert!(!app.cache.get(&id).unwrap().playable);
    background(
        &mut app,
        &mut tasks,
        Background::Metadata(0, test_track(2).id, Ok(test_track(2))),
    );
    assert!(app.cache.get(&test_track(2).id).is_some());
}
#[tokio::test]
async fn playlist_job_stays_active_until_final_message_is_applied() {
    let (mut tasks, _) = tasks();
    let mut app = App::new(Config::default(), Queue::default());
    tasks.playlist = Some(tokio::spawn(async {}));
    tokio::task::yield_now().await;
    tasks.enqueue_playlist(&mut app, test_track(1).id, "Again".into());
    assert_eq!(tasks.playlist_request, 0);
    assert!(app.status.contains("already being added"));
    background(
        &mut app,
        &mut tasks,
        Background::PlaylistPage(0, 0, vec![test_track(1)], true),
    );
    assert!(tasks.playlist.is_none());
    assert_eq!(app.queue.ids.len(), 1);
}
#[tokio::test]
async fn playlist_enqueue_follows_pages_counts_playable_tracks_and_reports_partial_failure() {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{path, query_param},
    };
    let server = MockServer::start().await;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(Catalog::mock(&server.uri()), tx).unwrap();
    let mut app = App::new(Config::default(), Queue::default());
    for (offset, next, status) in [
        (0, Some("next"), 200),
        (50, Some("next"), 200),
        (100, None, 503),
    ] {
        let mut template = ResponseTemplate::new(status);
        if status == 200 {
            template=template.set_body_json(serde_json::json!({"items":[{"item":{"id":format!("{:022}",offset),"name":"Song","type":"track","is_playable":true}},{"item":{"id":format!("{:022}",offset+1),"type":"track","is_playable":false}}],"next":next}));
        }
        Mock::given(path("/playlists/0000000000000000000001/items"))
            .and(query_param("offset", offset.to_string()))
            .respond_with(template)
            .expect(1)
            .mount(&server)
            .await;
    }
    tasks.enqueue_playlist(&mut app, "0000000000000000000001".into(), "Test".into());
    for _ in 0..3 {
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        background(&mut app, &mut tasks, event);
    }
    assert_eq!(app.queue.ids.len(), 2);
    assert!(app.status.contains("after 2 additions"));
    assert!(app.status.contains("503"));
}
#[tokio::test]
async fn library_progress_keeps_partial_matches_and_ignores_results_after_cancel_or_scope_change() {
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
            crate::library::LibraryProgress {
                tracks: vec![test_track(1)],
                scanned: 150,
                complete: false,
            },
        ),
    );
    assert_eq!(app.len(), 1);
    assert!(app.catalog.busy);
    assert_eq!(app.catalog.library_scanned, 150);
    key(
        &mut app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(!app.catalog.busy);
    assert!(!app.quit);
    assert!(app.catalog.title.contains("partial"));
    background(
        &mut app,
        &mut tasks,
        Background::LibraryProgress(
            10,
            crate::library::LibraryProgress {
                tracks: vec![test_track(2)],
                scanned: 200,
                complete: true,
            },
        ),
    );
    assert_eq!(app.len(), 1);
    let request = app.catalog.request;
    background(
        &mut app,
        &mut tasks,
        Background::LibraryDone(request, Err(anyhow::anyhow!("rate limit"))),
    );
    assert_eq!(app.len(), 1);
    assert!(app.status.contains("partial matches retained"));
    app.catalog.query.clear();
    key(
        &mut app,
        KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.search_scope, SearchScope::Spotify);
    assert!(app.catalog.editing);
    assert!(!app.catalog.busy);
    background(
        &mut app,
        &mut tasks,
        Background::LibraryDone(request, Ok(())),
    );
    assert_eq!(app.catalog.title, "Spotify search");
}
