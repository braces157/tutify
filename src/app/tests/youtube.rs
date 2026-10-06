use super::*;

fn app() -> App {
    let mut app = App::new(
        Config {
            source: crate::model::MusicSource::Youtube,
            ..Config::default()
        },
        Queue::default(),
    );
    app.catalog.search_scope = SearchScope::Youtube;
    app.catalog.title = "YouTube search".into();
    let mut one = test_track(1);
    one.id = "youtube:dQw4w9WgXcQ".into();
    let mut two = test_track(2);
    two.id = "youtube:aqz-KE-bpKQ".into();
    app.catalog.rows = Rows::Tracks(vec![one, two]);
    app
}

#[tokio::test]
async fn youtube_results_use_the_real_queue_controls_without_spotify_radio() {
    let mut app = app();
    let (mut tasks, _bg) = tasks();
    let (tx, mut commands) = mpsc::unbounded_channel();
    app.catalog.selected = 1;
    actions::apply(&mut app, Action::PlaySelected, &mut tasks, &tx);
    assert_eq!(app.queue.current(), Some("youtube:aqz-KE-bpKQ"));
    assert_eq!(app.queue.ids.len(), 2);
    assert!(app.radio_epoch.is_none());
    assert!(tasks.recommendations.is_none());
    assert!(
        matches!(commands.try_recv(), Ok(Command::Load { id, .. }) if id == "youtube:aqz-KE-bpKQ")
    );
    app.control(Control::Media(MediaAction::Pause), &tx);
    assert!(matches!(commands.try_recv(), Ok(Command::Pause)));
    app.control(Control::Seek(Seek::Position(12_000)), &tx);
    assert!(matches!(commands.try_recv(), Ok(Command::Seek(12_000))));
    assert_eq!(app.state, State::Paused);
    app.catalog.selected = 0;
    actions::apply(&mut app, Action::PlayNext, &mut tasks, &tx);
    assert_eq!(app.queue.ids.len(), 3);
    app.queue.validate().unwrap();
}

#[tokio::test]
async fn youtube_guarded_library_views_and_stale_metadata_preserve_the_session() {
    let mut app = app();
    let (mut tasks, _bg) = tasks();
    let (tx, mut commands) = mpsc::unbounded_channel();
    for view in [View::Playlists, View::Liked] {
        tasks.view(&mut app, view);
        assert_eq!(app.catalog.view, View::Search);
        assert!(!app.catalog.busy);
        assert!(app.status.contains("unavailable"));
    }
    choose_search(&mut app, SearchScope::Library, &mut tasks);
    assert_eq!(app.catalog.search_scope, SearchScope::Youtube);
    actions::apply(&mut app, Action::ViewAlbum, &mut tasks, &tx);
    assert!(commands.try_recv().is_err());
    app.queue
        .replace(vec!["youtube:dQw4w9WgXcQ".into()], 0, false);
    app.generation = 5;
    let mut track = test_track(1);
    track.id = "youtube:dQw4w9WgXcQ".into();
    app.playback_event(
        Event::Metadata {
            generation: 4,
            track: track.clone(),
        },
        &tx,
    );
    assert!(app.cache.get(&track.id).is_none());
    app.playback_event(
        Event::Metadata {
            generation: 5,
            track: track.clone(),
        },
        &tx,
    );
    assert_eq!(app.cache.get(&track.id).unwrap().name, track.name);
}

#[tokio::test]
async fn youtube_search_never_follows_spotify_album_or_artist_links() {
    let mut app = app();
    let (background_tx, _background_rx) = mpsc::unbounded_channel();
    let mut tasks = Tasks::demo(background_tx).unwrap();
    let (tx, _commands) = mpsc::unbounded_channel();
    for entity in ["album", "artist"] {
        app.catalog.query = format!("spotify:{entity}:{}", "0".repeat(22));
        app.catalog.editing = true;
        key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.catalog.view, View::Search);
        assert!(
            matches!(&app.catalog.browse, Browse::Search(query) if query.starts_with("spotify:"))
        );
    }
}

#[test]
fn youtube_source_search_and_help_render_at_supported_sizes() {
    let mut app = app();
    app.catalog.rows = Rows::Tracks(Vec::new());
    for (width, height) in [(120, 32), (80, 24), (32, 10)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| ui::draw(frame, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("YouTube"), "{width}x{height}: {text}");
        assert!(!text.contains("Spotify track link"));
    }
    app.catalog.view = View::Help;
    draw_mouse(&app, 80, 24);
}

#[test]
fn public_music_search_identifies_its_provider_without_google_login() {
    let mut app = app();
    app.config.youtube_music = true;
    app.config.youtube_connected = false;
    app.catalog.rows = Rows::Tracks(Vec::new());
    app.catalog.title = "YouTube Music search".into();
    for (width, height) in [(120, 32), (80, 24), (32, 10)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| ui::draw(frame, &app, &mut app.ui.render.borrow_mut()))
            .unwrap();
        let cells = terminal.backend().buffer().content();
        let text: String = cells.iter().map(|cell| cell.symbol()).collect();
        assert!(
            text.contains("YouTube Music search"),
            "{width}x{height}: {text}"
        );
        let header: String = cells
            .iter()
            .take(width as usize)
            .map(|cell| cell.symbol())
            .collect();
        assert!(!header.contains("/ Spotify"));
        if width >= 80 {
            assert!(header.contains("/ YouTube Music"));
            assert!(text.contains("F2  Music"));
        }
        assert!(!text.contains("F3  Your library"));
    }
}

const MUSIC_FIXTURE: &str = r#"import json,sys
r=json.load(sys.stdin)
def song(i):
    return {'videoId':'v%010d'%i,'title':'Song '+str(i),'artists':[{'name':'Artist '+str(i%8),'id':'UC_artist'+str(i%8)}],'album':{'name':'Album','id':'MPRE_album'},'duration_seconds':180,'isAvailable':i!=60}
if r['operation']=='playlists':
    items=[{'playlistId':'PL_fixture','title':'Fixture','author':'Owner'},{'playlistId':'PL_restricted','title':'Restricted','author':'Owner'}]
elif r['operation']=='playlist' and r['id']=='PL_restricted':
    json.dump({'ok':False,'error':'restricted'},sys.stdout)
    sys.exit(0)
elif r['operation']=='recommendations':
    items=[song(0),dict(song(999),title='Song 1',artists=song(1)['artists'])]+[song(i) for i in range(200,230)]
else:
    items=[song(i) for i in range(min(r['limit'],123))]
json.dump({'ok':True,'items':items,'complete':len(items)<r['limit'],'title':'Fixture library'},sys.stdout)
"#;

fn music_tasks() -> (Tasks, mpsc::UnboundedReceiver<Background>) {
    let music = crate::youtube::music::Client::mock(MUSIC_FIXTURE, true);
    let catalog = Catalog::youtube(crate::youtube::Tools::music_fixture(music)).unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    (Tasks::new(catalog, tx).unwrap(), rx)
}

async fn next_music_event(rx: &mut mpsc::UnboundedReceiver<Background>) -> Background {
    tokio::time::timeout(Duration::from_secs(15), rx.recv())
        .await
        .expect("music fixture timed out")
        .expect("music fixture disconnected")
}

#[tokio::test]
async fn connected_music_playlists_and_likes_fill_the_queue_and_keep_metadata_for_shuffle() {
    for (view, browse) in [
        (
            View::Playlists,
            Browse::Playlist("youtube:playlist:PL_fixture".into()),
        ),
        (View::Liked, Browse::Liked),
    ] {
        let mut app = app();
        app.config.youtube_music = true;
        app.config.youtube_connected = true;
        app.catalog.view = view;
        app.catalog.browse = browse;
        let (mut tasks, mut events) = music_tasks();
        let (tx, mut commands) = mpsc::unbounded_channel();
        tasks.request(&mut app, 0);
        background(&mut app, &mut tasks, next_music_event(&mut events).await);
        assert_eq!(app.catalog.next, Some(50));
        app.catalog.selected = 10;
        actions::apply(&mut app, Action::PlaySelected, &mut tasks, &tx);
        let current = app.queue.current().unwrap().to_owned();
        assert_eq!(current, "youtube:v0000000010");
        assert!(matches!(commands.try_recv(), Ok(Command::Load { id, .. }) if id == current));
        while tasks.playlist.is_some() {
            background(&mut app, &mut tasks, next_music_event(&mut events).await);
        }
        assert_eq!(app.queue.ids.len(), 122);
        assert_eq!(app.queue.ids.iter().collect::<HashSet<_>>().len(), 122);
        assert_eq!(app.queue.current(), Some(current.as_str()));
        assert!(!app.queue.ids.contains(&"youtube:v0000000060".to_owned()));
        let original = app.cache.get(&current).unwrap().clone();
        app.playback_event(
            Event::Metadata {
                generation: app.generation,
                track: Track {
                    id: current.clone(),
                    name: "Generic video title".into(),
                    artists: "Uploader".into(),
                    duration_ms: 181000,
                    ..Default::default()
                },
            },
            &tx,
        );
        let updated = app.cache.get(&current).unwrap();
        assert_eq!(updated.name, original.name);
        assert_eq!(updated.artist_ids, original.artist_ids);
        assert_eq!(updated.album_id, original.album_id);
        assert_eq!(updated.duration_ms, 181000);
        tasks.cycle_shuffle(&mut app);
        tasks.cycle_shuffle(&mut app);
        assert!(app.queue.smart_shuffle);
        background(&mut app, &mut tasks, next_music_event(&mut events).await);
        assert!(!app.queue.suggestions.is_empty(), "{}", app.status);
        assert!(app.status.contains("YouTube Music"));
        assert!(!app.queue.ids.contains(&"youtube:v0000000999".to_owned()));
        assert_eq!(app.queue.current(), Some(current.as_str()));
        app.queue.validate().unwrap();
        tasks.cycle_shuffle(&mut app);
        assert_eq!(app.queue.ids.len(), 122);
        assert!(app.queue.suggestions.is_empty());
        // Playing a queue row must not append the library tail a second time.
        app.catalog.view = View::Queue;
        actions::apply(&mut app, Action::PlaySelected, &mut tasks, &tx);
        assert!(tasks.playlist.is_none());
    }
}

#[tokio::test]
async fn replacing_a_music_queue_rejects_late_playlist_pages() {
    let mut app = app();
    let (mut tasks, mut events) = music_tasks();
    tasks.enqueue_playlist(
        &mut app,
        "youtube:playlist:PL_fixture".into(),
        "Fixture".into(),
    );
    let event = next_music_event(&mut events).await;
    app.queue
        .replace(vec!["youtube:dQw4w9WgXcQ".into()], 0, false);
    background(&mut app, &mut tasks, event);
    assert_eq!(app.queue.ids, vec!["youtube:dQw4w9WgXcQ"]);
}

#[tokio::test]
async fn music_saved_library_search_deduplicates_and_reports_inaccessible_playlists() {
    let mut app = app();
    app.config.youtube_music = true;
    app.config.youtube_connected = true;
    app.catalog.search_scope = SearchScope::Library;
    app.catalog.query = "Song 122".into();
    app.catalog.browse = Browse::Search(app.catalog.query.clone());
    let (mut tasks, mut events) = music_tasks();
    tasks.request(&mut app, 0);
    while app.catalog.busy {
        background(&mut app, &mut tasks, next_music_event(&mut events).await);
    }
    assert_eq!(app.raw_len(), 1, "{}", app.status);
    assert_eq!(app.catalog.library_scanned, 246);
    assert_eq!(app.catalog.library_skipped.len(), 1);
    assert_eq!(
        app.catalog.library_skipped[0].id,
        "youtube:playlist:PL_restricted"
    );
    assert!(app.catalog.title.contains("partial coverage"));
    assert!(app.status.contains("F4 lists sources"));
}
