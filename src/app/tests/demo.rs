use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn demo_tasks() -> (Tasks, mpsc::UnboundedReceiver<Background>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (Tasks::demo(sender).unwrap(), receiver)
}

#[tokio::test]
async fn search_input_resolves_album_uri_and_url_directly_to_album_view() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    // 1. Spotify URI: spotify:album:<id>
    app.catalog.editing = true;
    app.catalog.query = "spotify:album:8000000000000000000001".into();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(!app.catalog.editing);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(
        app.catalog.browse,
        Browse::Album("8000000000000000000001".into())
    );
    assert_eq!(app.catalog.title, "Album");
    assert_eq!(app.catalog.selected, 0);

    // Pop back to initial view
    assert!(app.pop_navigation());
    assert_eq!(app.catalog.view, View::Search);

    // 2. HTTPS Web URL: https://open.spotify.com/album/<id>
    app.catalog.editing = true;
    app.catalog.query = "https://open.spotify.com/album/8000000000000000000001".into();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(!app.catalog.editing);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(
        app.catalog.browse,
        Browse::Album("8000000000000000000001".into())
    );

    assert!(app.pop_navigation());

    // 3. Internationalized URL with query parameters:
    app.catalog.editing = true;
    app.catalog.query =
        "https://open.spotify.com/intl-fr/album/8000000000000000000001?si=abc12345".into();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(!app.catalog.editing);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(
        app.catalog.browse,
        Browse::Album("8000000000000000000001".into())
    );
}

#[tokio::test]
async fn search_input_resolves_artist_uri_and_url_directly_to_artist_view() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    // 1. Spotify URI: spotify:artist:<id>
    app.catalog.editing = true;
    app.catalog.query = "spotify:artist:0000000000000000000500".into();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(!app.catalog.editing);
    assert_eq!(app.catalog.view, View::Artist);
    assert_eq!(
        app.catalog.browse,
        Browse::Artist("0000000000000000000500".into())
    );
    assert_eq!(app.catalog.title, "Artist • Top Tracks");
    assert_eq!(app.catalog.selected, 0);

    // Pop back to initial view
    assert!(app.pop_navigation());
    assert_eq!(app.catalog.view, View::Search);

    // 2. HTTPS Web URL: https://open.spotify.com/artist/<id>
    app.catalog.editing = true;
    app.catalog.query = "https://open.spotify.com/artist/0000000000000000000500".into();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(!app.catalog.editing);
    assert_eq!(app.catalog.view, View::Artist);
    assert_eq!(
        app.catalog.browse,
        Browse::Artist("0000000000000000000500".into())
    );

    assert!(app.pop_navigation());

    // 3. Internationalized URL with query parameters:
    app.catalog.editing = true;
    app.catalog.query =
        "https://open.spotify.com/intl-ja/artist/0000000000000000000500?si=trackradio".into();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(!app.catalog.editing);
    assert_eq!(app.catalog.view, View::Artist);
    assert_eq!(
        app.catalog.browse,
        Browse::Artist("0000000000000000000500".into())
    );
}

#[tokio::test]
async fn search_input_regular_query_retains_search_browse() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();

    app.catalog.editing = true;
    app.catalog.query = "Radiohead OK Computer".into();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(!app.catalog.editing);
    assert_eq!(app.catalog.view, View::Search);
    assert_eq!(
        app.catalog.browse,
        Browse::Search("Radiohead OK Computer".into())
    );
}

#[tokio::test]
async fn demo_mode_browsing_album_and_artist_returns_fictional_data() {
    let (mut tasks, mut receiver) = demo_tasks();
    let mut app = crate::demo::app();

    // Request Album page in demo mode
    app.catalog.browse = Browse::Album(crate::demo::ALBUM_ID.into());
    tasks.request(&mut app, 0);
    let event = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, event);

    if let Rows::Tracks(tracks) = &app.catalog.rows {
        assert_eq!(tracks.len(), 10);
        for (i, t) in tracks.iter().enumerate() {
            assert_eq!(t.track_number, Some((i + 1) as u32));
            assert_eq!(t.album_id.as_deref(), Some(crate::demo::ALBUM_ID));
            assert!(crate::model::valid_id(&t.id));
        }
        assert!(tracks.iter().any(|t| !t.playable));
        assert!(tracks.iter().any(|t| t.playable));
    } else {
        panic!("Expected Rows::Tracks for demo album");
    }

    // Request Artist page for Mali & The Signals
    let artist_id = format!("{:022}", 500);
    app.catalog.browse = Browse::Artist(artist_id.clone());
    tasks.request(&mut app, 0);
    let event = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, event);

    if let Rows::Tracks(tracks) = &app.catalog.rows {
        assert_eq!(tracks.len(), 8);
        for t in tracks {
            assert_eq!(t.artists, "Mali & The Signals");
            assert_eq!(t.artist_ids, vec![artist_id.clone()]);
            assert!(crate::model::valid_id(&t.id));
        }
    } else {
        panic!("Expected Rows::Tracks for demo artist");
    }

    // Request fallback artist page for an arbitrary valid 22-char ID
    let fallback_id = "1234567890123456789012";
    app.catalog.browse = Browse::Artist(fallback_id.into());
    tasks.request(&mut app, 0);
    let event = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, event);

    if let Rows::Tracks(tracks) = &app.catalog.rows {
        assert_eq!(tracks.len(), 8);
        for t in tracks {
            assert_eq!(t.artist_ids, vec![fallback_id.to_string()]);
            assert!(crate::model::valid_id(&t.id));
        }
    } else {
        panic!("Expected Rows::Tracks for fallback demo artist");
    }
}

#[tokio::test]
async fn demo_album_and_artist_browsing_generates_zero_network_requests() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let (sender, mut receiver) = mpsc::unbounded_channel();
    let mut tasks = Tasks::demo(sender).unwrap();
    tasks.catalog = Catalog::mock(&server.uri());
    let mut app = crate::demo::app();
    let (tx, _rx) = mpsc::unbounded_channel();

    // 1. Enter album URL in search
    app.catalog.editing = true;
    app.catalog.query = "https://open.spotify.com/album/8000000000000000000001".into();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Album);

    // Apply background page
    let event = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, event);
    assert_eq!(app.len(), 10);

    // 2. Right-click track 0 to open context menu and click "View Artist"
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Catalog(0),
        MouseButton::Right,
        &mut tasks,
        &tx,
    );
    draw_mouse(&app, 120, 35);
    click_target(
        &mut app,
        MouseTarget::Menu(4),
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Artist);

    // Apply background artist page
    let event = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, event);
    assert_eq!(app.len(), 8);

    // 3. Trigger playback actions (enqueue 'e', play next 'p')
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );

    // 4. Pop back to Album view via Esc
    key(
        &mut app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Album);

    // 5. Pop back to previous view via Esc
    key(
        &mut app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.catalog.view, View::Queue);

    // 6. Check metadata resolution in demo mode
    tasks.metadata(&app);
    while let Ok(event) = receiver.try_recv() {
        background(&mut app, &mut tasks, event);
    }

    // 7. Verify exactly ZERO HTTP requests were received by the server
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn adversarial_absolute_network_isolation_across_all_demo_browsing_playback_and_tasks() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let (sender, mut receiver) = mpsc::unbounded_channel();
    let mut tasks = Tasks::demo(sender).unwrap();
    // Intentionally point catalog to a mock server returning 503 errors.
    // If any demo task bypasses offline isolation, it will hit this server and fail.
    tasks.catalog = Catalog::mock(&server.uri());
    let mut app = crate::demo::app();
    let (tx, _rx) = mpsc::unbounded_channel();

    // 1. Browse Album
    app.catalog.view = View::Album;
    app.catalog.browse = Browse::Album(crate::demo::ALBUM_ID.into());
    tasks.request(&mut app, 0);
    let event = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, event);
    assert_eq!(app.len(), 10);

    // 2. Browse Known Artist
    app.catalog.view = View::Artist;
    app.catalog.browse = Browse::Artist("0000000000000000000500".into());
    tasks.request(&mut app, 0);
    let event = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, event);
    assert_eq!(app.len(), 8);

    // 3. Browse Arbitrary Unknown Artist (Fallback)
    app.catalog.view = View::Artist;
    app.catalog.browse = Browse::Artist("4aawyAB9vmqN3uQ7FjRGTy".into());
    tasks.request(&mut app, 0);
    let event = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, event);
    assert_eq!(app.len(), 8);

    // 4. Browse Search with text
    app.catalog.view = View::Search;
    app.catalog.browse = Browse::Search("Monsoon".into());
    tasks.request(&mut app, 0);
    let event = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, event);
    assert!(app.len() > 0);

    // 5. Browse Search with empty query
    app.catalog.view = View::Search;
    app.catalog.browse = Browse::Search("".into());
    tasks.request(&mut app, 0);
    let event = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, event);
    assert_eq!(app.len(), 24);

    // 6. Browse Playlists list
    app.catalog.view = View::Playlists;
    app.catalog.browse = Browse::Playlists;
    tasks.request(&mut app, 0);
    let event = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, event);
    assert_eq!(app.len(), 1);

    // 7. Browse Playlist tracks
    app.catalog.view = View::Playlists;
    app.catalog.browse = Browse::Playlist(crate::demo::PLAYLIST_ID.into());
    tasks.request(&mut app, 0);
    let event = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, event);
    assert_eq!(app.len(), 24);

    // 8. Browse Liked songs
    app.catalog.view = View::Liked;
    app.catalog.browse = Browse::Liked;
    tasks.request(&mut app, 0);
    let event = receiver.recv().await.unwrap();
    background(&mut app, &mut tasks, event);
    assert_eq!(app.len(), 24);

    // 9. Playback action interactions (enqueue, play next, play selected)
    app.catalog.selected = 0;
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );

    // 10. Background task triggers
    tasks.refill_radio(&app);
    tasks.refill_smart_shuffle(&app);
    tasks.update_lyrics(&mut app);
    tasks.open_mix(&mut app);

    // 11. Empty cache metadata lookup
    app.cache = crate::cache::MetadataCache::default();
    tasks.metadata(&app);
    while let Ok(event) = receiver.try_recv() {
        background(&mut app, &mut tasks, event);
    }

    // 12. Strict assertion: EXACTLY ZERO requests received by the mock server
    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests.len(),
        0,
        "Offline demo mode must emit zero HTTP requests, but {} requests were received",
        requests.len()
    );
}

#[tokio::test]
async fn adversarial_arbitrary_artist_id_fallback_stress_and_edge_cases() {
    let test_ids = [
        // Valid 22-character Spotify IDs
        "4aawyAB9vmqN3uQ7FjRGTy",
        "0000000000000000000001",
        "ZZZZZZZZZZZZZZZZZZZZZZ",
        "abcdefghijklmnopqrstuv",
        "1234567890123456789012",
        "A1B2C3D4E5F6G7H8I9J0K1",
        "0000000000000000000000",
        "9999999999999999999999",
        "zZzZzZzZzZzZzZzZzZzZzZ",
        "1111111111111111111111",
        // Non-standard edge cases
        "",
        "short",
        "toolong0000000000000000000001",
        "special!@#$%^&*()_+",
        "日本語アーティスト名",
    ];

    for &artist_id in &test_ids {
        let tracks = crate::demo::artist_tracks(artist_id);
        // Fallback must ALWAYS return exactly 8 fictional tracks, never empty
        assert_eq!(
            tracks.len(),
            8,
            "Expected 8 fallback tracks for id '{}', got {}",
            artist_id,
            tracks.len()
        );

        // Every track ID must be a valid 22-char Spotify ID
        for (idx, track) in tracks.iter().enumerate() {
            assert!(
                crate::model::valid_id(&track.id),
                "Fallback track id '{}' is not valid",
                track.id
            );
            assert_eq!(track.artist_ids, vec![artist_id.to_string()]);
            assert_eq!(track.album_id.as_deref(), Some(crate::demo::ALBUM_ID));
            assert!(track.duration_ms >= 170_000);
            assert!(!track.name.is_empty());
            assert_eq!(track.track_number, None);
            // Track 8 (idx 7) is unplayable, rest are playable
            if idx == 7 {
                assert!(!track.playable, "Track 8 in fallback must be unplayable");
            } else {
                assert!(
                    track.playable,
                    "Track {} in fallback must be playable",
                    idx + 1
                );
            }
        }

        if crate::model::valid_id(artist_id) {
            assert_eq!(tracks[0].artists, "Fictional Artist");
        } else {
            assert_eq!(tracks[0].artists, "Demo Artist");
        }
    }
}

#[tokio::test]
async fn adversarial_metadata_resolution_all_106_demo_tracks_zero_missing_items() {
    let all_tracks = crate::demo::all_tracks();
    // 24 playlist + 16 recommendation + 10 album + 48 known artist + 8 fallback artist = 106
    assert_eq!(
        all_tracks.len(),
        106,
        "Expected exactly 106 total demo tracks"
    );

    let (mut tasks, mut receiver) = demo_tasks();

    for track in &all_tracks {
        let mut app = App::new(Config::default(), Queue::default());
        app.demo = true;
        // Verify with empty cache: every single track MUST resolve from demo catalog
        app.cache = crate::cache::MetadataCache::default();
        app.queue.replace(vec![track.id.clone()], 0, false);

        tasks.metadata(&app);

        let event = receiver
            .recv()
            .await
            .expect("Expected metadata background event");
        match event {
            Background::Metadata(_req, id, result) => {
                assert_eq!(id, track.id);
                let resolved = result.unwrap_or_else(|e| {
                    panic!(
                        "Track '{}' ({}) failed metadata resolution: {:?}",
                        track.name, track.id, e
                    )
                });
                assert_eq!(resolved.id, track.id);
                assert_eq!(resolved.name, track.name);
                assert_eq!(resolved.artists, track.artists);
                assert_eq!(resolved.duration_ms, track.duration_ms);
                assert_eq!(resolved.playable, track.playable);
            }
            _ => panic!("Expected Background::Metadata"),
        }

        let done = receiver.recv().await.expect("Expected metadata done event");
        match done {
            Background::MetadataDone(_req) => {}
            _ => panic!("Expected Background::MetadataDone"),
        }

        tasks.requested.clear();
    }
}

#[tokio::test]
async fn adversarial_album_tracklist_playability_and_sequential_numbering() {
    let tracks = crate::demo::album_tracks(crate::demo::ALBUM_ID);
    assert_eq!(
        tracks.len(),
        10,
        "Album tracklist must contain exactly 10 tracks"
    );

    // 1. Verify strict sequential track numbering 1..=10
    for (i, t) in tracks.iter().enumerate() {
        assert_eq!(
            t.track_number,
            Some((i + 1) as u32),
            "Track at index {} must have track_number Some({})",
            i,
            i + 1
        );
        assert_eq!(t.album_id.as_deref(), Some(crate::demo::ALBUM_ID));
        assert_eq!(t.artists, "Mali & The Signals");
        assert!(crate::model::valid_id(&t.id));
    }

    // 2. Verify track 5 (index 4) is unplayable and all others are playable
    for (i, t) in tracks.iter().enumerate() {
        if i == 4 {
            assert!(!t.playable, "Track 5 (index 4) must be unplayable");
        } else {
            assert!(t.playable, "Track {} (index {}) must be playable", i + 1, i);
        }
    }

    // 3. Test interactive behavior in App for track 5 (unplayable)
    let (mut tasks, _) = demo_tasks();
    let (tx, _) = mpsc::unbounded_channel();
    let mut app = crate::demo::app();
    app.catalog.view = View::Album;
    app.catalog.browse = Browse::Album(crate::demo::ALBUM_ID.into());
    app.catalog.rows = Rows::Tracks(tracks.clone());
    app.catalog.selected = 4; // Track 5 selected

    let initial_queue_len = app.queue.ids.len();

    // A. Press Enter on unplayable track 5
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(
        app.status,
        "This track is unavailable for your account or region."
    );
    assert_eq!(app.queue.ids.len(), initial_queue_len);

    // B. Press 'e' (Enqueue) on unplayable track 5
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.status, "Unavailable track cannot be queued.");
    assert_eq!(app.queue.ids.len(), initial_queue_len);

    // C. Press 'p' (Play Next) on unplayable track 5
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.queue.ids.len(), initial_queue_len);

    // 4. Test interactive behavior on playable track 6 (index 5)
    app.catalog.selected = 5; // Track 6 selected
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    // When playing from album, queue is replaced with all PLAYABLE tracks (9 tracks)
    assert_eq!(
        app.queue.ids.len(),
        9,
        "Queue should contain only the 9 playable tracks"
    );
    // Track 5 id "8000000000000000000105" must NOT be in queue
    assert!(
        !app.queue
            .ids
            .iter()
            .any(|id| id == "8000000000000000000105"),
        "Unplayable track 5 must not be included in playback queue"
    );
    // Cursor index in playable queue should be 4 (track 6 is the 5th playable track)
    assert_eq!(app.queue.selected, 4);
    assert_eq!(app.queue.current(), Some("8000000000000000000106"));
}
