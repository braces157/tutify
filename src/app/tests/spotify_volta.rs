use super::*;

fn app() -> App {
    App::new(
        Config {
            source: crate::model::MusicSource::SpotifyVolta,
            ..Config::default()
        },
        Queue::default(),
    )
}

#[tokio::test]
async fn spotify_playlists_keep_original_ids_and_queue_order_with_volta_audio() {
    let mut app = app();
    let (mut tasks, _bg) = tasks();
    let (tx, mut commands) = mpsc::unbounded_channel();
    app.catalog.view = View::Playlists;
    app.catalog.browse = Browse::Playlist(format!("{:022}", 99));
    app.catalog.rows = Rows::Tracks(vec![test_track(1), test_track(2)]);
    app.catalog.selected = 1;
    actions::apply(&mut app, Action::PlaySelected, &mut tasks, &tx);
    assert_eq!(app.queue.current(), Some(format!("{:022}", 2).as_str()));
    assert_eq!(app.queue.ids, [format!("{:022}", 1), format!("{:022}", 2)]);
    assert!(matches!(commands.try_recv(),Ok(Command::Load { id,.. }) if id == format!("{:022}",2)));
    assert!(app.radio_epoch.is_none());
    app.queue.validate().unwrap();
}

#[tokio::test]
async fn spotify_search_keeps_radio_and_saved_library_in_combined_mode() {
    let mut app = app();
    app.demo = true;
    let (mut tasks, _bg) = tasks();
    let (tx, _commands) = mpsc::unbounded_channel();
    choose_search(&mut app, SearchScope::Library, &mut tasks);
    assert_eq!(app.catalog.search_scope, SearchScope::Library);
    choose_search(&mut app, SearchScope::Spotify, &mut tasks);
    assert_eq!(app.catalog.search_scope, SearchScope::Spotify);
    app.catalog.rows = Rows::Tracks(vec![test_track(1)]);
    actions::apply(&mut app, Action::PlaySelected, &mut tasks, &tx);
    assert!(app.radio_epoch.is_some());
    assert_eq!(app.queue.current(), Some(format!("{:022}", 1).as_str()));
}
