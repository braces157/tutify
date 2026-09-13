use super::*;

fn route_key(app: &mut App, tasks: &mut Tasks, key_code: KeyCode, modifiers: KeyModifiers) -> bool {
    let (commands, _receiver) = mpsc::unbounded_channel();
    route_input(
        app,
        Input::Key(KeyEvent::new(key_code, modifiers)),
        tasks,
        &commands,
    )
}

fn sample_album_track(index: usize, album_name: &str, album_id: &str) -> Track {
    Track {
        id: format!("trk_album_{index:04}"),
        name: format!("Track {index}"),
        artists: "Test Artist".into(),
        artist_ids: vec!["artist_001".into()],
        duration_ms: 180_000 + (index as u32 * 10_000),
        playable: true,
        album: Some(album_name.into()),
        album_art_url: None,
        album_id: Some(album_id.into()),
        track_number: Some(index as u32),
    }
}

fn sample_artist_track(index: usize, artist_name: &str, artist_id: &str) -> Track {
    Track {
        id: format!("trk_artist_{index:04}"),
        name: format!("Popular Hit {index}"),
        artists: artist_name.into(),
        artist_ids: vec![artist_id.into()],
        duration_ms: 210_000,
        playable: true,
        album: Some("Greatest Hits".into()),
        album_art_url: None,
        album_id: Some("album_greatest_hits".into()),
        track_number: Some(index as u32),
    }
}

#[tokio::test]
async fn test_navigation_history_push_and_pop_restores_state() {
    let (mut tasks, _receiver) = tasks();
    let (commands, _cmd_rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());

    // Setup in View::Search with search results, cursor, and scroll
    app.catalog.view = View::Search;
    app.catalog.query = "radiohead".into();
    app.catalog.title = "Spotify search".into();
    let initial_tracks = vec![
        sample_album_track(1, "The Bends", "alb_bends"),
        sample_album_track(2, "OK Computer", "alb_okc"),
        sample_album_track(3, "Kid A", "alb_kida"),
        sample_album_track(4, "In Rainbows", "alb_rainbows"),
    ];
    app.catalog.rows = Rows::Tracks(initial_tracks.clone());
    app.catalog.selected = 1; // "OK Computer"
    app.ui.render.borrow_mut().catalog_scroll = 7;

    // Navigate to Album view for selected track
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);

    // Verify Album view state
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.browse, Browse::Album("alb_okc".into()));
    assert_eq!(app.catalog.title, "OK Computer");
    assert_eq!(app.catalog.selected, 0);
    assert_eq!(app.ui.render.borrow().catalog_scroll, 0);
    assert_eq!(app.catalog.history.len(), 1);

    // Verify history entry saved state
    let entry = &app.catalog.history[0];
    assert_eq!(entry.view, View::Search);
    assert_eq!(entry.query, "radiohead");
    assert_eq!(entry.selected, 1);
    assert_eq!(entry.scroll, 7);
    assert_eq!(entry.breadcrumb, "Search");

    // Pop navigation (simulate Esc)
    assert!(app.pop_navigation());

    // Verify restoration of all Search state
    assert_eq!(app.catalog.view, View::Search);
    assert_eq!(app.catalog.query, "radiohead");
    assert_eq!(app.catalog.title, "Spotify search");
    assert_eq!(app.catalog.selected, 1);
    assert_eq!(app.ui.render.borrow().catalog_scroll, 7);
    assert_eq!(app.catalog.history.len(), 0);

    // Verify tracks are intact without network re-fetch
    if let Rows::Tracks(restored_tracks) = &app.catalog.rows {
        assert_eq!(restored_tracks.len(), 4);
        assert_eq!(restored_tracks[1].name, "Track 2");
    } else {
        panic!("Expected Rows::Tracks");
    }
}

#[tokio::test]
async fn test_multi_level_navigation_stack_unwinding() {
    let (mut tasks, _receiver) = tasks();
    let (commands, _cmd_rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());

    // Level 0: View::Search
    app.catalog.view = View::Search;
    app.catalog.query = "pink floyd".into();
    app.catalog.title = "Spotify search".into();
    app.catalog.rows = Rows::Tracks(vec![sample_album_track(1, "The Wall", "alb_wall")]);
    app.catalog.selected = 0;
    app.ui.render.borrow_mut().catalog_scroll = 2;

    // Transition Level 0 -> Level 1: View::Album
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.title, "The Wall");
    assert_eq!(app.catalog.history.len(), 1);

    // Populate album tracks and select row 2
    let album_tracks = vec![
        sample_artist_track(1, "Pink Floyd", "art_floyd"),
        sample_artist_track(2, "Pink Floyd", "art_floyd"),
        sample_artist_track(3, "Pink Floyd", "art_floyd"),
    ];
    app.catalog.rows = Rows::Tracks(album_tracks);
    app.catalog.selected = 2;
    app.ui.render.borrow_mut().catalog_scroll = 4;

    // Transition Level 1 -> Level 2: View::Artist
    actions::apply(&mut app, Action::ViewArtist, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Artist);
    assert_eq!(app.catalog.title, "Pink Floyd • Top Tracks");
    assert_eq!(app.catalog.history.len(), 2);
    assert_eq!(app.catalog.history[0].breadcrumb, "Search");
    assert_eq!(app.catalog.history[1].breadcrumb, "The Wall");

    // Unwind Level 2 -> Level 1: Esc pops to Album
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.title, "The Wall");
    assert_eq!(app.catalog.selected, 2);
    assert_eq!(app.ui.render.borrow().catalog_scroll, 4);
    assert_eq!(app.catalog.history.len(), 1);
    assert!(!app.quit);

    // Unwind Level 1 -> Level 0: Esc pops to Search
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Search);
    assert_eq!(app.catalog.query, "pink floyd");
    assert_eq!(app.catalog.selected, 0);
    assert_eq!(app.ui.render.borrow().catalog_scroll, 2);
    assert_eq!(app.catalog.history.len(), 0);
    assert!(!app.quit);

    // Level 0 with empty stack: Esc triggers quit
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.quit);
}

#[tokio::test]
async fn test_queue_to_album_navigation_restores_queue_selected_and_scroll() {
    let (mut tasks, _receiver) = tasks();
    let (commands, _cmd_rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());

    // Setup queue with 10 tracks
    let mut track_ids = Vec::new();
    for i in 1..=10 {
        let t = sample_album_track(i, "Discovery", "alb_discovery");
        track_ids.push(t.id.clone());
        app.cache.insert(t.id.clone(), t);
    }
    app.queue.replace(track_ids, 0, false);
    app.catalog.view = View::Queue;
    app.queue.selected = 6;
    app.ui.render.borrow_mut().queue_scroll = 5;

    // View album for the selected track in queue
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);

    // In Album view
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.title, "Discovery");
    assert_eq!(app.catalog.history.len(), 1);
    assert_eq!(app.catalog.history[0].breadcrumb, "Queue");
    assert_eq!(app.catalog.history[0].queue_selected, 6);
    assert_eq!(app.catalog.history[0].queue_scroll, 5);

    // Esc back to Queue
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Queue);
    assert_eq!(app.queue.selected, 6);
    assert_eq!(app.ui.render.borrow().queue_scroll, 5);
    assert!(app.catalog.history.is_empty());
}

#[tokio::test]
async fn test_esc_hierarchy_context_menu_editing_filtering_history() {
    let (mut tasks, _receiver) = tasks();
    let mut app = App::new(Config::default(), Queue::default());

    // Push an entry to history
    app.catalog.view = View::Search;
    app.catalog.query = "daft punk".into();
    app.push_navigation("Search".into());
    app.catalog.view = View::Album;
    app.catalog.title = "Homework".into();
    assert_eq!(app.catalog.history.len(), 1);

    // 1. Context menu open: Esc closes context menu, does NOT pop navigation
    app.context_menu = Some(ContextMenu {
        selected: 0,
        filter: String::new(),
        x: 10,
        y: 10,
        actions: vec![("Play", Action::PlaySelected)],
        view: View::Album,
        row: 0,
        revision: 0,
    });
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.context_menu.is_none());
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.history.len(), 1);

    // 2. Editing active: Esc exits editing, does NOT pop navigation
    app.catalog.editing = true;
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.catalog.editing);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.history.len(), 1);

    // 3. Filtering active: Esc exits filtering, does NOT pop navigation
    app.catalog.filtering = true;
    app.catalog.filter = "alive".into();
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.catalog.filtering);
    assert!(app.catalog.filter.is_empty());
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.history.len(), 1);

    // 4. Clean view with non-empty history: Esc pops navigation
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Search);
    assert_eq!(app.catalog.query, "daft punk");
    assert!(app.catalog.history.is_empty());
    assert!(!app.quit);

    // 5. In Help with empty history: Esc returns to Search
    app.catalog.view = View::Help;
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Search);
    assert!(!app.quit);

    // 6. In Search with empty history: Esc quits
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.quit);
}

#[tokio::test]
async fn test_key_routing_a_shift_a_e_p() {
    let (mut tasks, _receiver) = tasks();
    let mut app = App::new(Config::default(), Queue::default());

    let track = sample_album_track(1, "A Rush of Blood to the Head", "alb_arobth");
    app.catalog.rows = Rows::Tracks(vec![track.clone()]);
    app.catalog.selected = 0;

    // In View::Search:
    // 'a' triggers ViewAlbum
    app.catalog.view = View::Search;
    route_key(&mut app, &mut tasks, KeyCode::Char('a'), KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.title, "A Rush of Blood to the Head");

    // Pop back to Search
    app.pop_navigation();
    assert_eq!(app.catalog.view, View::Search);

    // 'A' (Shift+A) triggers ViewArtist
    route_key(
        &mut app,
        &mut tasks,
        KeyCode::Char('A'),
        KeyModifiers::SHIFT,
    );
    assert_eq!(app.catalog.view, View::Artist);
    assert_eq!(app.catalog.title, "Test Artist • Top Tracks");

    // Pop back to Search
    app.pop_navigation();
    assert_eq!(app.catalog.view, View::Search);

    // 'a' with Shift (as sent by some terminals on Windows) also triggers ViewArtist
    route_key(
        &mut app,
        &mut tasks,
        KeyCode::Char('a'),
        KeyModifiers::SHIFT,
    );
    assert_eq!(app.catalog.view, View::Artist);
    assert_eq!(app.catalog.title, "Test Artist • Top Tracks");

    // Pop back to Search
    app.pop_navigation();
    assert_eq!(app.catalog.view, View::Search);

    // 'e' triggers EnqueueSelected
    assert_eq!(app.queue.ids.len(), 0);
    route_key(&mut app, &mut tasks, KeyCode::Char('e'), KeyModifiers::NONE);
    assert_eq!(app.queue.ids.len(), 1);
    assert_eq!(app.queue.ids[0], track.id);

    // 'p' in Search routes to playback control (Previous)
    route_key(&mut app, &mut tasks, KeyCode::Char('p'), KeyModifiers::NONE);
    // Queue didn't insert next because it was media control
    assert_eq!(app.queue.ids.len(), 1);

    // Now in View::Album:
    app.catalog.view = View::Album;
    app.catalog.rows = Rows::Tracks(vec![
        sample_album_track(2, "A Rush of Blood to the Head", "alb_arobth"),
        sample_album_track(3, "A Rush of Blood to the Head", "alb_arobth"),
    ]);
    app.catalog.selected = 1;

    // 'p' in View::Album triggers Action::PlayNext (inserts selected track after cursor)
    route_key(&mut app, &mut tasks, KeyCode::Char('p'), KeyModifiers::NONE);
    assert_eq!(app.queue.ids.len(), 2);
    assert_eq!(app.queue.ids[1], format!("trk_album_{:04}", 3));

    // 'e' in View::Album triggers EnqueueSelected
    app.catalog.selected = 0;
    route_key(&mut app, &mut tasks, KeyCode::Char('e'), KeyModifiers::NONE);
    assert_eq!(app.queue.ids.len(), 3);
    assert_eq!(app.queue.ids[2], format!("trk_album_{:04}", 2));
}

#[tokio::test]
async fn test_playback_actions_in_album_and_artist_views() {
    let (mut tasks, _receiver) = tasks();
    let (commands, _cmd_rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());

    // Setup Album view with 5 tracks
    app.catalog.view = View::Album;
    let album_tracks = vec![
        sample_album_track(1, "Parachutes", "alb_parachutes"),
        sample_album_track(2, "Parachutes", "alb_parachutes"),
        sample_album_track(3, "Parachutes", "alb_parachutes"),
        sample_album_track(4, "Parachutes", "alb_parachutes"),
        sample_album_track(5, "Parachutes", "alb_parachutes"),
    ];
    app.catalog.rows = Rows::Tracks(album_tracks.clone());
    app.catalog.selected = 2; // Track 3

    // Enter / PlaySelected replaces entire queue with tracks starting at selected row
    actions::apply(&mut app, Action::PlaySelected, &mut tasks, &commands);
    assert_eq!(app.queue.ids.len(), 5);
    assert_eq!(app.queue.cursor, Some(2));
    assert_eq!(app.queue.selected, 2);
    assert_eq!(app.queue.ids[2], album_tracks[2].id);

    // Setup Artist view with 4 tracks
    app.catalog.view = View::Artist;
    let artist_tracks = vec![
        sample_artist_track(1, "Coldplay", "art_coldplay"),
        sample_artist_track(2, "Coldplay", "art_coldplay"),
        sample_artist_track(3, "Coldplay", "art_coldplay"),
        sample_artist_track(4, "Coldplay", "art_coldplay"),
    ];
    app.catalog.rows = Rows::Tracks(artist_tracks.clone());
    app.catalog.selected = 1; // Popular Hit 2

    // Enter / PlaySelected in Artist view replaces entire queue
    actions::apply(&mut app, Action::PlaySelected, &mut tasks, &commands);
    assert_eq!(app.queue.ids.len(), 4);
    assert_eq!(app.queue.cursor, Some(1));
    assert_eq!(app.queue.selected, 1);
    assert_eq!(app.queue.ids[1], artist_tracks[1].id);
}

#[tokio::test]
async fn test_history_bounded_to_20_entries() {
    let mut app = App::new(Config::default(), Queue::default());

    // Push 25 entries
    for i in 0..25 {
        app.push_navigation(format!("Step {i}"));
        app.catalog.selected = i;
    }

    assert_eq!(app.catalog.history.len(), 20);
    // Oldest 5 entries (0..5) were discarded, entry 0 should now be Step 5
    assert_eq!(app.catalog.history[0].breadcrumb, "Step 5");
    // Latest entry should be Step 24
    assert_eq!(app.catalog.history.last().unwrap().breadcrumb, "Step 24");
}

#[tokio::test]
async fn test_tab_switching_clears_history_stack() {
    let (mut tasks, _receiver) = tasks();
    let mut app = App::new(Config::default(), Queue::default());

    // Push history
    app.catalog.view = View::Search;
    app.push_navigation("Search".into());
    app.catalog.view = View::Album;
    assert_eq!(app.catalog.history.len(), 1);

    // Press '2' (Playlists) -> clears history stack
    route_key(&mut app, &mut tasks, KeyCode::Char('2'), KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Playlists);
    assert!(app.catalog.history.is_empty());
}

#[tokio::test]
async fn test_missing_album_or_artist_id_sets_status() {
    let (mut tasks, _receiver) = tasks();
    let (commands, _cmd_rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());

    let track_no_album = Track {
        id: "trk_no_alb".into(),
        name: "Single".into(),
        artists: "Indie Artist".into(),
        artist_ids: vec!["art_indie".into()],
        duration_ms: 180_000,
        playable: true,
        album: None,
        album_art_url: None,
        album_id: None,
        track_number: None,
    };
    app.catalog.rows = Rows::Tracks(vec![track_no_album]);
    app.catalog.selected = 0;

    // ViewAlbum when album_id is None
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);
    assert!(app.status.contains("No album information"));
    assert_eq!(app.catalog.view, View::Search); // Did not transition

    let track_no_artist = Track {
        id: "trk_no_art".into(),
        name: "Unknown Track".into(),
        artists: "".into(),
        artist_ids: vec![],
        duration_ms: 180_000,
        playable: true,
        album: Some("Some Album".into()),
        album_art_url: None,
        album_id: Some("alb_some".into()),
        track_number: None,
    };
    app.catalog.rows = Rows::Tracks(vec![track_no_artist]);
    app.catalog.selected = 0;

    // ViewArtist when artist_ids is empty
    actions::apply(&mut app, Action::ViewArtist, &mut tasks, &commands);
    assert!(app.status.contains("No artist information"));
    assert_eq!(app.catalog.view, View::Search); // Did not transition
}

#[tokio::test]
async fn test_adversarial_key_a_metadata_variations() {
    let (mut tasks, _receiver) = tasks();
    let mut app = App::new(Config::default(), Queue::default());

    // 1. With valid album metadata
    let t_valid = sample_album_track(1, "The Dark Side of the Moon", "alb_dsotm");
    app.catalog.rows = Rows::Tracks(vec![t_valid]);
    app.catalog.selected = 0;
    app.catalog.view = View::Search;
    route_key(&mut app, &mut tasks, KeyCode::Char('a'), KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.browse, Browse::Album("alb_dsotm".into()));
    assert_eq!(app.catalog.title, "The Dark Side of the Moon");
    assert_eq!(app.catalog.history.len(), 1);

    // Pop back to Search
    assert!(app.pop_navigation());
    assert_eq!(app.catalog.view, View::Search);

    // 2. With album_id: None
    let mut t_no_id = sample_album_track(2, "No ID Album", "alb_dummy");
    t_no_id.album_id = None;
    app.catalog.rows = Rows::Tracks(vec![t_no_id]);
    app.catalog.selected = 0;
    route_key(&mut app, &mut tasks, KeyCode::Char('a'), KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Search);
    assert_eq!(app.catalog.history.len(), 0);
    assert!(app.status.contains("No album information available"));

    // 3. With album_id: Some("")
    let mut t_empty_id = sample_album_track(3, "Empty ID Album", "");
    t_empty_id.album_id = Some("".into());
    app.catalog.rows = Rows::Tracks(vec![t_empty_id]);
    app.catalog.selected = 0;
    route_key(&mut app, &mut tasks, KeyCode::Char('a'), KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Search);
    assert_eq!(app.catalog.history.len(), 0);
    assert!(app.status.contains("No album information available"));

    // 4. With empty track rows
    app.catalog.rows = Rows::Tracks(vec![]);
    app.catalog.selected = 0;
    route_key(&mut app, &mut tasks, KeyCode::Char('a'), KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Search);
    assert_eq!(app.catalog.history.len(), 0);
    assert!(app.status.contains("Select a track to view album"));

    // 5. In View::Help
    app.catalog.view = View::Help;
    app.status.clear();
    route_key(&mut app, &mut tasks, KeyCode::Char('a'), KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Help);
    assert!(app.status.is_empty());
}

#[tokio::test]
async fn test_adversarial_key_shift_a_metadata_variations() {
    let (mut tasks, _receiver) = tasks();
    let mut app = App::new(Config::default(), Queue::default());

    // 1. With valid artist metadata
    let t_valid = sample_artist_track(1, "Pink Floyd", "art_floyd");
    app.catalog.rows = Rows::Tracks(vec![t_valid]);
    app.catalog.selected = 0;
    app.catalog.view = View::Search;
    route_key(
        &mut app,
        &mut tasks,
        KeyCode::Char('A'),
        KeyModifiers::SHIFT,
    );
    assert_eq!(app.catalog.view, View::Artist);
    assert_eq!(app.catalog.browse, Browse::Artist("art_floyd".into()));
    assert_eq!(app.catalog.title, "Pink Floyd • Top Tracks");
    assert_eq!(app.catalog.history.len(), 1);

    // Pop back to Search
    assert!(app.pop_navigation());
    assert_eq!(app.catalog.view, View::Search);

    // 2. With empty artist_ids
    let mut t_no_artist = sample_artist_track(2, "Unknown", "art_dummy");
    t_no_artist.artist_ids = vec![];
    app.catalog.rows = Rows::Tracks(vec![t_no_artist]);
    app.catalog.selected = 0;
    route_key(
        &mut app,
        &mut tasks,
        KeyCode::Char('A'),
        KeyModifiers::SHIFT,
    );
    assert_eq!(app.catalog.view, View::Search);
    assert_eq!(app.catalog.history.len(), 0);
    assert!(app.status.contains("No artist information available"));

    // 3. With artist_ids: vec!["".into()]
    let mut t_empty_artist = sample_artist_track(3, "Blank Artist", "art_dummy");
    t_empty_artist.artist_ids = vec!["".into()];
    app.catalog.rows = Rows::Tracks(vec![t_empty_artist]);
    app.catalog.selected = 0;
    route_key(
        &mut app,
        &mut tasks,
        KeyCode::Char('A'),
        KeyModifiers::SHIFT,
    );
    assert_eq!(app.catalog.view, View::Search);
    assert_eq!(app.catalog.history.len(), 0);
    assert!(app.status.contains("No artist information available"));

    // 4. With empty track rows
    app.catalog.rows = Rows::Tracks(vec![]);
    app.catalog.selected = 0;
    route_key(
        &mut app,
        &mut tasks,
        KeyCode::Char('A'),
        KeyModifiers::SHIFT,
    );
    assert_eq!(app.catalog.view, View::Search);
    assert_eq!(app.catalog.history.len(), 0);
    assert!(app.status.contains("Select a track to view artist"));

    // 5. In View::Help
    app.catalog.view = View::Help;
    app.status.clear();
    route_key(
        &mut app,
        &mut tasks,
        KeyCode::Char('A'),
        KeyModifiers::SHIFT,
    );
    assert_eq!(app.catalog.view, View::Help);
    assert!(app.status.is_empty());
}

#[tokio::test]
async fn test_adversarial_key_e_across_all_views() {
    let (mut tasks, _receiver) = tasks();
    let mut app = App::new(Config::default(), Queue::default());

    let track = sample_album_track(1, "Album Test", "alb_test");

    // 1. Search view
    app.catalog.view = View::Search;
    app.catalog.rows = Rows::Tracks(vec![track.clone()]);
    app.catalog.selected = 0;
    route_key(&mut app, &mut tasks, KeyCode::Char('e'), KeyModifiers::NONE);
    assert_eq!(app.queue.ids, vec![track.id.clone()]);

    // 2. Album view
    let track_alb = sample_album_track(2, "Album Test", "alb_test");
    app.catalog.view = View::Album;
    app.catalog.rows = Rows::Tracks(vec![track_alb.clone()]);
    app.catalog.selected = 0;
    route_key(&mut app, &mut tasks, KeyCode::Char('e'), KeyModifiers::NONE);
    assert_eq!(app.queue.ids, vec![track.id.clone(), track_alb.id.clone()]);

    // 3. Artist view
    let track_art = sample_artist_track(3, "Artist Test", "art_test");
    app.catalog.view = View::Artist;
    app.catalog.rows = Rows::Tracks(vec![track_art.clone()]);
    app.catalog.selected = 0;
    route_key(&mut app, &mut tasks, KeyCode::Char('e'), KeyModifiers::NONE);
    assert_eq!(
        app.queue.ids,
        vec![track.id.clone(), track_alb.id.clone(), track_art.id.clone()]
    );

    // 4. Liked view
    let track_lik = sample_album_track(4, "Album Test", "alb_test");
    app.catalog.view = View::Liked;
    app.catalog.rows = Rows::Tracks(vec![track_lik.clone()]);
    app.catalog.selected = 0;
    route_key(&mut app, &mut tasks, KeyCode::Char('e'), KeyModifiers::NONE);
    assert_eq!(app.queue.ids.len(), 4);

    // 5. Unplayable track in Album view
    let mut unplayable_trk = sample_album_track(5, "Album Test", "alb_test");
    unplayable_trk.playable = false;
    app.catalog.view = View::Album;
    app.catalog.rows = Rows::Tracks(vec![unplayable_trk]);
    app.catalog.selected = 0;
    route_key(&mut app, &mut tasks, KeyCode::Char('e'), KeyModifiers::NONE);
    assert_eq!(app.queue.ids.len(), 4); // Did not add unplayable track
    assert!(app.status.contains("Unavailable track cannot be queued"));
}

#[tokio::test]
async fn test_adversarial_mode_switching_no_misfires() {
    let (mut tasks, _receiver) = tasks();
    let mut app = App::new(Config::default(), Queue::default());

    let track = sample_album_track(1, "Album Test", "alb_test");
    app.catalog.rows = Rows::Tracks(vec![track.clone()]);
    app.catalog.selected = 0;
    app.catalog.view = View::Search;

    // 1. Search box editing mode
    app.catalog.editing = true;
    app.catalog.query.clear();
    route_key(&mut app, &mut tasks, KeyCode::Char('a'), KeyModifiers::NONE);
    route_key(
        &mut app,
        &mut tasks,
        KeyCode::Char('A'),
        KeyModifiers::SHIFT,
    );
    route_key(&mut app, &mut tasks, KeyCode::Char('e'), KeyModifiers::NONE);
    route_key(&mut app, &mut tasks, KeyCode::Char('p'), KeyModifiers::NONE);
    assert_eq!(app.catalog.query, "aAep");
    assert_eq!(app.catalog.view, View::Search);
    assert_eq!(app.queue.ids.len(), 0);

    // 2. Filter mode in Liked view
    app.catalog.editing = false;
    app.catalog.view = View::Liked;
    app.catalog.filtering = true;
    app.catalog.filter.clear();
    route_key(&mut app, &mut tasks, KeyCode::Char('a'), KeyModifiers::NONE);
    route_key(
        &mut app,
        &mut tasks,
        KeyCode::Char('A'),
        KeyModifiers::SHIFT,
    );
    route_key(&mut app, &mut tasks, KeyCode::Char('e'), KeyModifiers::NONE);
    route_key(&mut app, &mut tasks, KeyCode::Char('p'), KeyModifiers::NONE);
    assert_eq!(app.catalog.filter, "aAep");
    assert_eq!(app.catalog.view, View::Liked);
    assert_eq!(app.queue.ids.len(), 0);

    // 3. Context menu open in Album view
    app.catalog.filtering = false;
    app.catalog.view = View::Album;
    app.context_menu = Some(ContextMenu {
        selected: 0,
        filter: String::new(),
        x: 0,
        y: 0,
        actions: vec![("Play", Action::PlaySelected)],
        view: View::Album,
        row: 0,
        revision: 0,
    });
    route_key(&mut app, &mut tasks, KeyCode::Char('a'), KeyModifiers::NONE);
    route_key(
        &mut app,
        &mut tasks,
        KeyCode::Char('A'),
        KeyModifiers::SHIFT,
    );
    route_key(&mut app, &mut tasks, KeyCode::Char('e'), KeyModifiers::NONE);
    route_key(&mut app, &mut tasks, KeyCode::Char('p'), KeyModifiers::NONE);
    assert!(app.context_menu.is_some());
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.queue.ids.len(), 0);
}

#[tokio::test]
async fn test_adversarial_playback_actions_in_album_and_artist_views() {
    let (mut tasks, _receiver) = tasks();
    let (commands, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());

    // Album with mixed playable and unplayable tracks
    let t0 = sample_album_track(1, "Album A", "alb_a");
    let mut t1 = sample_album_track(2, "Album A", "alb_a");
    let t2 = sample_album_track(3, "Album A", "alb_a");
    let t3 = sample_album_track(4, "Album A", "alb_a");
    t1.playable = false; // Track 2 is unplayable

    app.catalog.view = View::Album;
    app.catalog.rows = Rows::Tracks(vec![t0.clone(), t1.clone(), t2.clone(), t3.clone()]);

    // Press Enter on unplayable track (t1, index 1)
    app.catalog.selected = 1;
    actions::apply(&mut app, Action::PlaySelected, &mut tasks, &commands);
    assert!(
        app.status
            .contains("unavailable for your account or region")
    );
    assert_eq!(app.queue.ids.len(), 0);

    // Press Enter on playable track t2 (index 2 in album, but index 1 among playable tracks [t0, t2, t3])
    app.catalog.selected = 2;
    actions::apply(&mut app, Action::PlaySelected, &mut tasks, &commands);
    assert_eq!(
        app.queue.ids,
        vec![t0.id.clone(), t2.id.clone(), t3.id.clone()]
    );
    assert_eq!(app.queue.cursor, Some(1));
    assert_eq!(app.queue.selected, 1);
}

#[tokio::test]
async fn test_adversarial_key_p_play_next_vs_previous_in_queue_view() {
    let (mut tasks, _receiver) = tasks();
    let (commands, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());

    let t0 = sample_album_track(0, "A", "alb_a");
    let t1 = sample_album_track(1, "A", "alb_a");
    let t2 = sample_album_track(2, "A", "alb_a");
    app.cache.insert(t0.id.clone(), t0.clone());
    app.cache.insert(t1.id.clone(), t1.clone());
    app.cache.insert(t2.id.clone(), t2.clone());
    app.queue
        .replace(vec![t0.id.clone(), t1.id.clone(), t2.id.clone()], 1, false);

    // Case 1: Fresh Queue view -> 'p' must trigger MediaAction::Previous, moving cursor from 1 to 0
    app.catalog.view = View::Queue;
    app.queue.selected = 1;
    assert_eq!(app.queue.cursor, Some(1));
    route_input(
        &mut app,
        Input::Key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        &mut tasks,
        &commands,
    );
    assert_eq!(
        app.queue.cursor,
        Some(0),
        "In fresh Queue view, 'p' must rewind to previous track"
    );
    assert_eq!(app.queue.ids.len(), 3, "Queue length must not grow");

    // Case 2: In View::Album -> 'p' must trigger Action::PlayNext
    app.catalog.view = View::Album;
    app.catalog.browse = Browse::Album("alb_a".into());
    let alb_track = sample_album_track(10, "A", "alb_a");
    app.catalog.rows = Rows::Tracks(vec![alb_track.clone()]);
    app.catalog.selected = 0;
    route_input(
        &mut app,
        Input::Key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        &mut tasks,
        &commands,
    );
    assert_eq!(
        app.queue.ids.len(),
        4,
        "In Album view, 'p' must insert next track"
    );

    // Case 3: CRITICAL BUG CHECK
    // Navigate from Album view to Queue view (e.g. by pressing '4')
    route_input(
        &mut app,
        Input::Key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE)),
        &mut tasks,
        &commands,
    );
    assert_eq!(app.catalog.view, View::Queue);

    // Set cursor to 1 so we can test rewinding to previous track
    app.queue.cursor = Some(1);
    app.queue.selected = 1;
    let len_before = app.queue.ids.len();

    // Now press 'p' while in Queue view!
    // Per requirement: Key `p` acting as PlayNext STRICTLY in `View::Album` and `View::Artist`,
    // and falling through to `MediaAction::Previous` in Search, Playlists, Liked, and Queue.
    route_input(
        &mut app,
        Input::Key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        &mut tasks,
        &commands,
    );
    assert_eq!(
        app.queue.cursor,
        Some(0),
        "In Queue view (after visiting Album), 'p' MUST trigger MediaAction::Previous"
    );
    assert_eq!(
        app.queue.ids.len(),
        len_before,
        "In Queue view, 'p' must NOT insert track next"
    );
}

#[tokio::test]
async fn test_adversarial_deep_nesting_fifo_eviction_and_stress() {
    let mut app = App::new(Config::default(), Queue::default());

    // Push 35 entries
    for i in 0..35 {
        app.catalog.view = View::Search;
        app.catalog.query = format!("query_{i}");
        app.catalog.selected = i;
        app.push_navigation(format!("Level {i}"));
    }

    // Must be capped at exactly 20 entries
    assert_eq!(app.catalog.history.len(), 20);

    // Oldest surviving entry should be Level 15 (indices 0..15 evicted)
    assert_eq!(app.catalog.history[0].breadcrumb, "Level 15");
    assert_eq!(app.catalog.history[0].query, "query_15");
    assert_eq!(app.catalog.history[0].selected, 15);

    // Latest entry should be Level 34
    assert_eq!(app.catalog.history.last().unwrap().breadcrumb, "Level 34");
    assert_eq!(app.catalog.history.last().unwrap().query, "query_34");
    assert_eq!(app.catalog.history.last().unwrap().selected, 34);

    // Pop all 20 entries in LIFO order
    for expected_i in (15..=34).rev() {
        assert!(app.pop_navigation());
        assert_eq!(app.catalog.query, format!("query_{expected_i}"));
        assert_eq!(app.catalog.selected, expected_i);
    }

    // 21st pop: stack is exhausted
    assert_eq!(app.catalog.history.len(), 0);
    assert!(!app.pop_navigation());

    // Stress test: 1,000 pushes and pops with large track payloads
    let large_tracks: Vec<Track> = (0..500)
        .map(|k| sample_album_track(k, "Heavy Album", "alb_heavy"))
        .collect();
    app.catalog.rows = Rows::Tracks(large_tracks);

    for i in 0..1000 {
        app.catalog.query = format!("stress_{i}");
        app.push_navigation(format!("Stress {i}"));
        assert_eq!(app.catalog.history.len(), (i + 1).min(20));
    }
    assert_eq!(app.catalog.history.len(), 20);
    assert_eq!(app.catalog.history[0].breadcrumb, "Stress 980");
    assert_eq!(app.catalog.history.last().unwrap().breadcrumb, "Stress 999");

    for _ in 0..20 {
        assert!(app.pop_navigation());
    }
    assert!(!app.pop_navigation());
}

#[tokio::test]
async fn test_adversarial_rapid_esc_unwinding_back_to_root() {
    let (mut tasks, _receiver) = tasks();
    let (commands, _cmd_rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());

    // Root: Search
    app.catalog.view = View::Search;
    app.catalog.query = "radiohead".into();
    app.catalog.title = "Spotify search".into();
    app.catalog.selected = 1;
    app.ui.render.borrow_mut().catalog_scroll = 5;
    app.catalog.rows = Rows::Tracks(vec![
        sample_album_track(1, "The Bends", "alb_bends"),
        sample_album_track(2, "OK Computer", "alb_okc"),
        sample_album_track(3, "Kid A", "alb_kida"),
    ]);

    // Level 1: View Album (OK Computer)
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.title, "OK Computer");
    app.catalog.rows = Rows::Tracks(vec![sample_artist_track(1, "Radiohead", "art_rh")]);
    app.catalog.selected = 0;
    app.ui.render.borrow_mut().catalog_scroll = 3;

    // Level 2: View Artist (Radiohead)
    actions::apply(&mut app, Action::ViewArtist, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Artist);
    assert_eq!(app.catalog.title, "Radiohead • Top Tracks");
    app.catalog.rows = Rows::Tracks(vec![sample_album_track(1, "In Rainbows", "alb_rainbows")]);
    app.catalog.selected = 0;
    app.ui.render.borrow_mut().catalog_scroll = 8;

    // Level 3: View Album (In Rainbows)
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.title, "In Rainbows");
    assert_eq!(app.catalog.history.len(), 3);

    // Rapid unwinding via Esc key
    // Esc 1 -> unwinds to Level 2 (Artist Radiohead)
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Artist);
    assert_eq!(app.catalog.title, "Radiohead • Top Tracks");
    assert_eq!(app.catalog.selected, 0);
    assert_eq!(app.ui.render.borrow().catalog_scroll, 8);
    assert_eq!(app.catalog.history.len(), 2);
    assert!(!app.quit);

    // Esc 2 -> unwinds to Level 1 (Album OK Computer)
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.title, "OK Computer");
    assert_eq!(app.catalog.selected, 0);
    assert_eq!(app.ui.render.borrow().catalog_scroll, 3);
    assert_eq!(app.catalog.history.len(), 1);
    assert!(!app.quit);

    // Esc 3 -> unwinds to Level 0 (Root: Search)
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Search);
    assert_eq!(app.catalog.query, "radiohead");
    assert_eq!(app.catalog.selected, 1);
    assert_eq!(app.ui.render.borrow().catalog_scroll, 5);
    assert_eq!(app.catalog.history.len(), 0);
    assert!(!app.quit);

    // Esc 4 -> at Root with empty history -> Quits app
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.quit);
}

#[tokio::test]
async fn test_adversarial_queue_to_album_modifications_and_restoration() {
    let (mut tasks, _receiver) = tasks();
    let (commands, _cmd_rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());

    // Setup queue with 15 tracks
    let mut track_ids = Vec::new();
    for i in 1..=15 {
        let t = sample_album_track(i, "Random Access Memories", "alb_ram");
        track_ids.push(t.id.clone());
        app.cache.insert(t.id.clone(), t);
    }
    app.queue.replace(track_ids, 0, false);
    app.catalog.view = View::Queue;
    app.queue.selected = 11;
    app.ui.render.borrow_mut().queue_scroll = 8;

    // Navigate to Album from Queue
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.title, "Random Access Memories");

    // Populate album tracks
    let new_track_1 = sample_album_track(101, "RAM Track 1", "alb_ram");
    let new_track_2 = sample_album_track(102, "RAM Track 2", "alb_ram");
    app.cache
        .insert(new_track_1.id.clone(), new_track_1.clone());
    app.cache
        .insert(new_track_2.id.clone(), new_track_2.clone());
    app.catalog.rows = Rows::Tracks(vec![new_track_1.clone(), new_track_2.clone()]);
    app.catalog.selected = 0;

    // Enqueue track 1 ('e')
    route_key(&mut app, &mut tasks, KeyCode::Char('e'), KeyModifiers::NONE);
    assert_eq!(app.queue.ids.len(), 16);

    // PlayNext track 2 ('p')
    app.catalog.selected = 1;
    route_key(&mut app, &mut tasks, KeyCode::Char('p'), KeyModifiers::NONE);
    assert_eq!(app.queue.ids.len(), 17);

    // Esc back to Queue
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Queue);
    // Verified: queue cursor and scroll are restored
    assert_eq!(app.queue.selected, 11);
    assert_eq!(app.ui.render.borrow().queue_scroll, 8);
    // Verified: the 2 enqueued tracks are retained in queue!
    assert_eq!(app.queue.ids.len(), 17);

    // ADVERSARIAL CASE: User views album, then plays entire album (replacing queue with 2 tracks),
    // then Esc back to Queue where original queue.selected was 11 (now out-of-bounds!)
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Album);
    app.catalog.rows = Rows::Tracks(vec![new_track_1.clone(), new_track_2.clone()]);
    app.catalog.selected = 0;

    // Enter / PlaySelected replaces entire queue with 2 tracks
    actions::apply(&mut app, Action::PlaySelected, &mut tasks, &commands);
    assert_eq!(app.queue.ids.len(), 2);

    // Esc back to Queue
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Queue);
    // Prior queue_selected (11) was restored, but queue has length 2
    assert_eq!(app.queue.selected, 11);

    // Verify app handles out-of-bounds queue.selected gracefully without panicking:
    assert!(app.selected_track().is_none()); // Does not panic!
    // Down key clamps safely
    route_key(&mut app, &mut tasks, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(app.queue.selected, 1); // Recovered to valid index!
}

#[tokio::test]
async fn test_adversarial_esc_pop_from_search_and_filtered_views() {
    let (mut tasks, _receiver) = tasks();
    let (commands, _cmd_rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());

    // 1. Search view with query and results
    app.catalog.view = View::Search;
    app.catalog.query = "beatles".into();
    app.catalog.rows = Rows::Tracks(vec![
        sample_album_track(1, "Abbey Road", "alb_abbey"),
        sample_album_track(2, "Revolver", "alb_revolver"),
    ]);
    app.catalog.selected = 1;

    // Navigate to Album
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Album);

    // Esc back to Search
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Search);
    assert_eq!(app.catalog.query, "beatles");
    assert_eq!(app.catalog.selected, 1);
    assert!(!app.catalog.editing);

    // 2. Liked Songs with active filter matching track name
    app.catalog.view = View::Liked;
    let mut t1 = sample_album_track(1, "Abbey Road", "alb_abbey");
    t1.name = "Come Together".into();
    let mut t2 = sample_album_track(2, "Revolver", "alb_revolver");
    t2.name = "Eleanor Rigby".into();
    app.catalog.rows = Rows::Tracks(vec![t1, t2]);
    app.catalog.filter = "together".into();
    app.catalog.filtering = false;
    app.catalog.selected = 0;

    // Navigate to Album from filtered Liked Songs
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Album);
    assert!(app.catalog.filter.is_empty());

    // Esc back to Liked Songs
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Liked);
    assert_eq!(app.catalog.filter, "together");
    assert_eq!(app.catalog.selected, 0);

    // Esc when filter is active clears the filter FIRST before quitting
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.catalog.filter.is_empty());
    assert!(!app.quit);

    // Next Esc with empty history quits
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.quit);
}

#[tokio::test]
async fn test_adversarial_pop_navigation_emits_zero_network_requests_wiremock() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;
    let valid_album_id = "4aawyAB9vmqN3uQ7FjRGTy"; // exactly 22 alphanumeric chars
    let valid_artist_id = "art1234567890123456789"; // exactly 22 alphanumeric chars

    // Setup mock server for album tracks
    let album_json = serde_json::json!({
        "items": [
            {
                "id": "trk_wm_01",
                "name": "Wiremock Track 1",
                "artists": [{"name": "Test Artist", "id": valid_artist_id}],
                "duration_ms": 180000,
                "is_playable": true,
                "track_number": 1
            }
        ],
        "next": null
    });

    Mock::given(method("GET"))
        .and(path(format!("/albums/{valid_album_id}/tracks")))
        .respond_with(ResponseTemplate::new(200).set_body_json(album_json))
        .expect(1) // EXACTLY 1 request when entering the album view
        .mount(&mock_server)
        .await;

    let (tx, _rx) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(Catalog::mock(&mock_server.uri()), tx).unwrap();
    let (commands, _cmd_rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());

    // Start in Search
    app.catalog.view = View::Search;
    app.catalog.query = "test".into();
    let track = Track {
        id: "trk1234567890123456789".into(),
        name: "Test Track".into(),
        artists: "Test Artist".into(),
        artist_ids: vec![valid_artist_id.into()],
        duration_ms: 180000,
        playable: true,
        album: Some("Test Album".into()),
        album_art_url: None,
        album_id: Some(valid_album_id.into()),
        track_number: Some(1),
    };
    app.catalog.rows = Rows::Tracks(vec![track]);
    app.catalog.selected = 0;

    // Step 1: Navigate to Album view -> triggers exactly 1 HTTP request
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Album);

    // Allow background request to finish
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    // Capture the number of received HTTP requests so far
    let requests_before_pop = mock_server.received_requests().await.unwrap().len();
    assert_eq!(requests_before_pop, 1);

    // Step 2: Pop navigation via Esc
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Search);

    // Yield to let any potential background tasks execute
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    // Verify ZERO additional HTTP requests were made upon popping navigation!
    let requests_after_pop = mock_server.received_requests().await.unwrap().len();
    assert_eq!(
        requests_after_pop, requests_before_pop,
        "Popping navigation must emit zero network requests!"
    );

    // Step 3: In-flight cancellation test
    // Mount a delayed endpoint for artist top tracks
    Mock::given(method("GET"))
        .and(path(format!("/artists/{valid_artist_id}/top-tracks")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_millis(500))
                .set_body_json(serde_json::json!({ "tracks": [] })),
        )
        .mount(&mock_server)
        .await;

    // Navigate to Artist view -> request in flight
    actions::apply(&mut app, Action::ViewArtist, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Artist);
    assert!(tasks.browse.is_some());
    let req_epoch = app.catalog.request;

    // Immediately pop before network request finishes
    route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.catalog.view, View::Search);
    // Task was aborted
    assert!(tasks.browse.is_none());
    // Request counter was incremented to invalidate any delayed response
    assert!(app.catalog.request > req_epoch);
}

#[tokio::test]
async fn test_empirical_challenger_m5_2_adversarial_integration_stress_harness() {
    // SCENARIO 1: Search -> Album -> Artist -> Esc back to Album -> Esc back to Search -> Esc quit
    {
        let (mut tasks, _receiver) = tasks();
        let (_commands, _cmd_rx) = mpsc::unbounded_channel::<Command>();
        let mut app = App::new(Config::default(), Queue::default());

        app.catalog.view = View::Search;
        app.catalog.query = "pink floyd".into();
        app.catalog.title = "Spotify search".into();
        let initial_tracks: Vec<Track> = (1..=50)
            .map(|i| sample_album_track(i, "The Dark Side of the Moon", "alb_dsotm"))
            .collect();
        app.catalog.rows = Rows::Tracks(initial_tracks);
        app.catalog.selected = 25;
        app.ui.render.borrow_mut().catalog_scroll = 20;

        // Render initial Search state (height 35 -> catalog height ~25; scroll 20 is valid)
        draw_mouse(&app, 120, 35);
        assert_eq!(app.ui.render.borrow().catalog_scroll, 20);

        // Step 1: Navigate to Album view via key 'a'
        route_key(&mut app, &mut tasks, KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(app.catalog.view, View::Album);
        assert_eq!(app.catalog.browse, Browse::Album("alb_dsotm".into()));
        assert_eq!(app.catalog.title, "The Dark Side of the Moon");
        assert_eq!(app.catalog.selected, 0);
        assert_eq!(app.ui.render.borrow().catalog_scroll, 0);
        assert_eq!(app.catalog.history.len(), 1);

        // Render Album state
        draw_mouse(&app, 120, 35);

        // Populate album tracks, select row 25, scroll 20
        let album_tracks: Vec<Track> = (1..=50)
            .map(|i| sample_artist_track(i, "Pink Floyd", "art_floyd"))
            .collect();
        app.catalog.rows = Rows::Tracks(album_tracks);
        app.catalog.selected = 25;
        app.ui.render.borrow_mut().catalog_scroll = 20;

        // Render Album before pushing to Artist
        draw_mouse(&app, 120, 35);
        assert_eq!(app.ui.render.borrow().catalog_scroll, 20);

        // Step 2: Navigate to Artist view via 'Shift+A'
        route_key(
            &mut app,
            &mut tasks,
            KeyCode::Char('A'),
            KeyModifiers::SHIFT,
        );
        assert_eq!(app.catalog.view, View::Artist);
        assert_eq!(app.catalog.browse, Browse::Artist("art_floyd".into()));
        assert_eq!(app.catalog.title, "Pink Floyd • Top Tracks");
        assert_eq!(app.catalog.selected, 0);
        assert_eq!(app.ui.render.borrow().catalog_scroll, 0);
        assert_eq!(app.catalog.history.len(), 2);

        // Render Artist state
        draw_mouse(&app, 120, 35);

        // Populate artist tracks and perform playback interactions
        let artist_tracks: Vec<Track> = (1..=8)
            .map(|i| sample_album_track(i, "Echoes", "alb_echoes"))
            .collect();
        app.catalog.rows = Rows::Tracks(artist_tracks);
        app.catalog.selected = 2;

        // Enqueue ('e') and PlayNext ('p') in Artist view
        route_key(&mut app, &mut tasks, KeyCode::Char('e'), KeyModifiers::NONE);
        route_key(&mut app, &mut tasks, KeyCode::Char('p'), KeyModifiers::NONE);
        assert_eq!(app.queue.ids.len(), 2);

        // Step 3: Esc 1 -> Unwinds from Artist back to Album
        route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.catalog.view, View::Album);
        assert_eq!(app.catalog.title, "The Dark Side of the Moon");
        assert_eq!(
            app.catalog.selected, 25,
            "Album cursor must be restored to 25"
        );
        assert_eq!(
            app.ui.render.borrow().catalog_scroll,
            20,
            "Album scroll must be restored to 20"
        );
        assert_eq!(app.catalog.history.len(), 1);
        assert!(!app.quit);
        draw_mouse(&app, 120, 35);
        assert_eq!(app.ui.render.borrow().catalog_scroll, 20);

        // Step 4: Esc 2 -> Unwinds from Album back to Search
        route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.catalog.view, View::Search);
        assert_eq!(app.catalog.query, "pink floyd");
        assert_eq!(
            app.catalog.selected, 25,
            "Search cursor must be restored to 25"
        );
        assert_eq!(
            app.ui.render.borrow().catalog_scroll,
            20,
            "Search scroll must be restored to 20"
        );
        assert_eq!(app.catalog.history.len(), 0);
        assert!(!app.quit);
        draw_mouse(&app, 120, 35);
        assert_eq!(app.ui.render.borrow().catalog_scroll, 20);

        // Step 5: Esc 3 -> At root with empty history triggers app.quit
        route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
        assert!(
            app.quit,
            "Esc on root view with empty history must quit app"
        );
    }

    // SCENARIO 2: Queue -> Album -> Esc back to Queue (Exact cursor & scroll restoration)
    {
        let (mut tasks, _receiver) = tasks();
        let (commands, _cmd_rx) = mpsc::unbounded_channel::<Command>();
        let mut app = App::new(Config::default(), Queue::default());

        // Fill queue with 200 tracks
        let mut track_ids = Vec::with_capacity(200);
        for i in 1..=200 {
            let t = sample_album_track(i, "Discovery", "alb_discovery");
            track_ids.push(t.id.clone());
            app.cache.insert(t.id.clone(), t);
        }
        app.queue.replace(track_ids, 50, false);
        app.catalog.view = View::Queue;
        app.queue.selected = 142;
        app.ui.render.borrow_mut().queue_scroll = 135;

        // Render queue initial state
        draw_mouse(&app, 120, 35);

        // View album from queue
        actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);
        assert_eq!(app.catalog.view, View::Album);
        assert_eq!(app.catalog.title, "Discovery");
        assert_eq!(app.catalog.history.len(), 1);
        assert_eq!(app.catalog.history[0].breadcrumb, "Queue");
        assert_eq!(app.catalog.history[0].queue_selected, 142);
        assert_eq!(app.catalog.history[0].queue_scroll, 135);

        // Populate album tracks and mutate queue from Album view
        let alb_tracks: Vec<Track> = (1..=12)
            .map(|i| sample_album_track(i, "Discovery", "alb_discovery"))
            .collect();
        app.catalog.rows = Rows::Tracks(alb_tracks);
        app.catalog.selected = 3;

        // Enqueue 2 tracks from album
        route_key(&mut app, &mut tasks, KeyCode::Char('e'), KeyModifiers::NONE);
        app.catalog.selected = 4;
        route_key(&mut app, &mut tasks, KeyCode::Char('p'), KeyModifiers::NONE);
        assert_eq!(app.queue.ids.len(), 202);

        // Esc back to Queue
        route_key(&mut app, &mut tasks, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.catalog.view, View::Queue);
        assert_eq!(
            app.queue.selected, 142,
            "Queue selected cursor must be exactly restored to 142"
        );
        assert_eq!(
            app.ui.render.borrow().queue_scroll,
            135,
            "Queue scroll offset must be exactly restored to 135"
        );
        assert_eq!(
            app.queue.ids.len(),
            202,
            "Enqueued tracks must persist in queue"
        );
        assert!(app.catalog.history.is_empty());

        // Render restored queue
        draw_mouse(&app, 120, 35);
    }

    // SCENARIO 3: Offline demo launch wiremock 0-network test
    {
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let mock_server = MockServer::start().await;
        // Mount 503 responder on all requests to ensure any leak fails immediately
        Mock::given(wiremock::matchers::method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&mock_server)
            .await;
        Mock::given(wiremock::matchers::method("POST"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&mock_server)
            .await;

        let (sender, mut receiver) = mpsc::unbounded_channel();
        let mut tasks = Tasks::demo(sender).unwrap();
        tasks.catalog = Catalog::mock(&mock_server.uri());
        let mut app = crate::demo::app();
        let (tx, _rx) = mpsc::unbounded_channel();

        // Browse demo album
        app.catalog.view = View::Album;
        app.catalog.browse = Browse::Album(crate::demo::ALBUM_ID.into());
        tasks.request(&mut app, 0);
        let event = receiver.recv().await.unwrap();
        background(&mut app, &mut tasks, event);
        assert_eq!(app.len(), 10);

        // Browse known demo artist
        app.catalog.view = View::Artist;
        app.catalog.browse = Browse::Artist("0000000000000000000500".into());
        tasks.request(&mut app, 0);
        let event = receiver.recv().await.unwrap();
        background(&mut app, &mut tasks, event);
        assert_eq!(app.len(), 8);

        // Browse arbitrary fallback artist
        app.catalog.browse = Browse::Artist("arbitrary_unknown_artist_id".into());
        tasks.request(&mut app, 0);
        let event = receiver.recv().await.unwrap();
        background(&mut app, &mut tasks, event);
        assert_eq!(app.len(), 8);

        // Perform playback interactions
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

        // Esc back
        key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );

        // Assert 0 network requests recorded by wiremock server
        let requests = mock_server.received_requests().await.unwrap();
        assert_eq!(
            requests.len(),
            0,
            "Demo mode must generate strictly 0 network requests"
        );
    }

    // SCENARIO 4: Pathological terminal geometries stress testing
    {
        let mut app = App::new(Config::default(), Queue::default());
        // Populate album data
        app.catalog.view = View::Album;
        app.catalog.title = "A Night at the Opera (Super Deluxe Edition)".into();
        let album_tracks: Vec<Track> = (1..=30)
            .map(|i| sample_album_track(i, "A Night at the Opera", "alb_queen"))
            .collect();
        app.catalog.rows = Rows::Tracks(album_tracks);
        app.push_navigation("Search".into());
        app.push_navigation("Queen".into());

        // Extreme dimension matrix
        let test_geometries = [
            (1, 1),
            (5, 5),
            (10, 5),
            (20, 5),
            (31, 9),
            (32, 10),
            (33, 10),
            (40, 10),
            (50, 10),
            (77, 18),
            (78, 18),
            (79, 18),
            (115, 24),
            (116, 24),
            (117, 24),
            (32, 200),
            (500, 10),
            (500, 200),
            (1000, 500),
        ];

        for (w, h) in test_geometries {
            // Normal Album view
            draw_mouse(&app, w, h);

            // Artist view
            app.catalog.view = View::Artist;
            draw_mouse(&app, w, h);

            // Queue view
            app.catalog.view = View::Queue;
            draw_mouse(&app, w, h);

            // Search view with query and editing
            app.catalog.view = View::Search;
            app.catalog.editing = true;
            app.catalog.query = "bohemian rhapsody".into();
            draw_mouse(&app, w, h);

            // Filter active
            app.catalog.editing = false;
            app.catalog.filtering = true;
            app.catalog.filter = "queen".into();
            draw_mouse(&app, w, h);

            // Reset state
            app.catalog.filtering = false;
            app.catalog.filter.clear();
            app.catalog.view = View::Album;
        }
    }
}

#[tokio::test]
async fn test_mouse_tab_switch_clears_navigation_history() {
    let (mut tasks, _receiver) = tasks();
    let (commands, _cmd_rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());

    // Set up in search view with a track and push to album view
    app.catalog.view = View::Search;
    app.catalog.rows = Rows::Tracks(vec![sample_album_track(1, "OK Computer", "alb_okc")]);
    app.catalog.selected = 0;
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.history.len(), 1);

    // Click on Liked Songs tab using mouse navigation target
    app.ui.render.borrow_mut().mouse_hits = vec![(
        ratatui::layout::Rect::new(0, 0, 10, 1),
        MouseTarget::Navigation(View::Liked),
    )];

    let consumed = crate::app::mouse::mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 2,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        &mut tasks,
        &commands,
    );
    assert!(consumed);
    assert_eq!(app.catalog.view, View::Liked);
    assert!(
        app.catalog.history.is_empty(),
        "Mouse tab switch must clear navigation history"
    );
}

#[tokio::test]
async fn test_self_navigation_deduplication_album_and_artist() {
    let (mut tasks, _receiver) = tasks();
    let (commands, _cmd_rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());

    // Setup Album view
    app.catalog.view = View::Search;
    let track = sample_album_track(1, "The Bends", "alb_bends");
    app.catalog.rows = Rows::Tracks(vec![track.clone()]);
    app.catalog.selected = 0;

    // First navigation from Search to Album
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Album);
    assert_eq!(app.catalog.history.len(), 1);

    // Track list inside Album view containing tracks from the same album
    app.catalog.rows = Rows::Tracks(vec![
        sample_album_track(1, "The Bends", "alb_bends"),
        sample_album_track(2, "The Bends", "alb_bends"),
    ]);
    app.catalog.selected = 1;

    // Attempting to view album of a track within the same album should deduplicate
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &commands);
    assert_eq!(
        app.catalog.history.len(),
        1,
        "Duplicate self-navigation to same album must not push history"
    );
    assert!(app.status.contains("Already viewing album"));

    // Now test artist view deduplication
    // Navigate from Album to Artist
    actions::apply(&mut app, Action::ViewArtist, &mut tasks, &commands);
    assert_eq!(app.catalog.view, View::Artist);
    assert_eq!(app.catalog.history.len(), 2);

    // Track list inside Artist view
    app.catalog.rows = Rows::Tracks(vec![sample_artist_track(1, "Test Artist", "artist_001")]);
    app.catalog.selected = 0;

    // Attempting to view artist when already on that artist
    actions::apply(&mut app, Action::ViewArtist, &mut tasks, &commands);
    assert_eq!(
        app.catalog.history.len(),
        2,
        "Duplicate self-navigation to same artist must not push history"
    );
    assert!(app.status.contains("Already viewing artist"));
}
