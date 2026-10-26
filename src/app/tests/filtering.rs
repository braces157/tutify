use super::*;

#[test]
fn local_filters_share_word_matching_but_keep_their_search_fields() {
    let mut app = App::new(Config::default(), Queue::default());
    let mut first = test_track(0);
    first.name = "Night Drive".into();
    first.artists = "Élan 日本語".into();
    first.album = Some("City Lights".into());
    first.playable = false;
    let mut second = test_track(1);
    second.name = "Night Rain".into();
    second.artists = "Élan".into();
    for track in [&first, &second] {
        app.cache.insert(track.id.clone(), track.clone());
    }
    app.catalog.rows = Rows::Tracks(vec![first.clone(), second.clone()]);
    app.catalog.view = View::Liked;
    app.queue
        .replace(vec![first.id.clone(), second.id, first.id], 0, false);

    for (query, catalog, queue) in [
        ("  nIGHT\tÉLAN\n日本語  ", vec![0], vec![0, 2]),
        ("night élan", vec![0, 1], vec![0, 1, 2]),
        ("city drive", vec![], vec![0, 2]),
        ("driveÉlan", vec![], vec![]),
        ("absent", vec![], vec![]),
        (" \t\n", vec![0, 1], vec![0, 1, 2]),
    ] {
        app.catalog.filter = query.into();
        app.ui.queue.query = query.into();
        assert_eq!(*app.filtered_indices(), catalog, "catalog query {query:?}");
        assert_eq!(*app.queue_rows(), queue, "queue query {query:?}");
    }
}

#[test]
fn queue_filter_can_match_incomplete_metadata_and_show_uncached_rows_when_blank() {
    let mut app = App::new(Config::default(), Queue::default());
    let mut track = test_track(0);
    track.name.clear();
    track.artists = "Élan".into();
    track.album = Some("City Lights".into());
    app.cache.insert(track.id.clone(), track.clone());
    app.queue
        .replace(vec![track.id, test_track(1).id], 0, false);

    app.ui.queue.query = "éLAN city".into();
    assert_eq!(*app.queue_rows(), vec![0]);
    assert_eq!(app.ui.queue.missing(&app.queue, &app.cache), 2);

    app.ui.queue.query = " \t ".into();
    let blank_rows = app.queue_rows();
    assert_eq!(*blank_rows, vec![0, 1]);
    assert_eq!(app.ui.queue.missing(&app.queue, &app.cache), 0);
    let loaded = test_track(1);
    app.cache.insert(loaded.id.clone(), loaded);
    assert!(Arc::ptr_eq(&blank_rows, &app.queue_rows()));
}

#[test]
fn catalog_track_selection_uses_visible_positions_only_in_filterable_views() {
    let mut app = App::new(Config::default(), Queue::default());
    app.catalog.rows = Rows::Tracks((0..3).map(test_track).collect());
    app.catalog.view = View::Liked;
    app.catalog.filter = "Song 2".into();
    assert_eq!(app.selected_track().unwrap().id, test_track(2).id);
    app.catalog.selected = 1;
    assert!(app.selected_track().is_none());

    for view in [View::Search, View::Album, View::Artist] {
        app.catalog.view = view;
        assert_eq!(app.selected_track().unwrap().id, test_track(1).id);
    }
    app.catalog.view = View::Liked;
    app.catalog.filter = " \t ".into();
    assert_eq!(app.selected_track().unwrap().id, test_track(1).id);
    app.catalog.selected = 3;
    assert!(app.selected_track().is_none());
}

#[tokio::test]
async fn playlist_actions_and_mix_use_the_same_filtered_selection() {
    for selected in [0, 1] {
        for action in [Action::PlaySelected, Action::EnqueueSelected] {
            let mut app = App::new(Config::default(), Queue::default());
            app.catalog.view = View::Playlists;
            app.catalog.rows = Rows::Playlists(vec![
                crate::model::Playlist {
                    id: "0".repeat(22),
                    name: "Day Drive".into(),
                    owner: "Other".into(),
                },
                crate::model::Playlist {
                    id: "1".repeat(22),
                    name: "Night Drive".into(),
                    owner: "Élan 日本語".into(),
                },
            ]);
            app.catalog.filter = "  nIGHT\tÉLAN 日本語 ".into();
            app.catalog.selected = selected;
            assert_eq!(*app.filtered_indices(), vec![1]);
            let (sender, mut receiver) = mpsc::unbounded_channel();
            let mut tasks = Tasks::demo(sender).unwrap();
            let (commands, _) = mpsc::unbounded_channel();

            tasks.open_mix(&mut app);
            assert_eq!(
                app.mix.source,
                Some(if selected == 0 {
                    MixSource::Playlist {
                        id: "1".repeat(22),
                        name: "Night Drive".into(),
                    }
                } else {
                    MixSource::Queue
                })
            );
            app.ui.overlay = Overlay::None;
            actions::apply(&mut app, action, &mut tasks, &commands);
            if selected == 1 {
                assert_eq!(app.catalog.browse, Browse::Search(String::new()));
                assert!(app.queue.ids.is_empty());
            } else if action == Action::PlaySelected {
                assert_eq!(app.catalog.browse, Browse::Playlist("1".repeat(22)));
                assert_eq!(app.catalog.title, "Night Drive");
            } else {
                assert_eq!(app.status, "Adding playlist 'Night Drive'...");
                while let Ok(event) = receiver.try_recv() {
                    background(&mut app, &mut tasks, event);
                }
                assert!(!app.queue.ids.is_empty());
            }
        }
    }
}
