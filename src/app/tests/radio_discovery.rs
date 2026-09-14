use super::*;
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::path};

#[tokio::test]
async fn similar_artist_search_radio_and_smart_flow_preserves_variety() {
    let spotify = MockServer::start().await;
    let external = MockServer::start().await;
    Mock::given(path("/search/artist"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data":[{"id":1,"name":"Artist 1"}]})),
        )
        .expect(1)
        .mount(&external)
        .await;
    let related: Vec<_> = (20..25)
        .map(|i| serde_json::json!({"id":i,"name":format!("Artist {i}")}))
        .collect();
    Mock::given(path("/artist/1/related"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data":related})))
        .expect(1)
        .mount(&external)
        .await;
    Mock::given(path("/search"))
        .respond_with(|request: &Request| {
            let q: std::collections::HashMap<_, _> =
                request.url.query_pairs().into_owned().collect();
            let rows = if q["q"] == "track:Seed artist:Artist 1" {
                vec![item(1, "Seed".into(), 1)]
            } else {
                let artist: usize = q["q"]
                    .trim_start_matches("artist:\"Artist ")
                    .trim_end_matches('"')
                    .parse()
                    .unwrap();
                let offset: usize = q["offset"].parse().unwrap();
                (0..10)
                    .map(|i| {
                        item(
                            artist * 1000 + offset + i,
                            format!("Song {artist} {}", offset + i),
                            artist,
                        )
                    })
                    .collect()
            };
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"tracks":{"items":rows}}))
        })
        .mount(&spotify)
        .await;
    let (bg_tx, mut bg) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(
        Catalog::mock_with_similarity(&spotify.uri(), &external.uri()),
        bg_tx,
    )
    .unwrap();
    let (commands, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    app.catalog.query.clear();
    route_input(
        &mut app,
        Input::Paste("track:Seed artist:Artist 1".into()),
        &mut tasks,
        &commands,
    );
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    settle(&mut app, &mut tasks, &mut bg).await;
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    settle(&mut app, &mut tasks, &mut bg).await;
    assert_eq!(app.current_track().unwrap().name, "Seed");
    assert_eq!(app.queue.ids.len(), 16);
    let artists: HashSet<_> = app
        .queue
        .ids
        .iter()
        .skip(1)
        .map(|id| &app.cache.get(id).unwrap().artists)
        .collect();
    assert!(artists.len() >= 5);
    playing(&mut app, &commands);
    app.queue.select(13);
    app.load(&commands);
    playing(&mut app, &commands);
    tasks.refill_radio(&app);
    settle(&mut app, &mut tasks, &mut bg).await;
    assert_eq!(app.queue.ids.len(), 31);
    assert_eq!(tasks.radio_seed.as_ref().unwrap().name, "Seed");
    app.queue.select(0);
    app.load(&commands);
    let before = spotify.received_requests().await.unwrap().len();
    tasks.cycle_shuffle(&mut app);
    tasks.cycle_shuffle(&mut app);
    settle(&mut app, &mut tasks, &mut bg).await;
    let artists: HashSet<_> = app
        .queue
        .suggestions
        .iter()
        .map(|index| &app.cache.get(&app.queue.ids[*index]).unwrap().artists)
        .collect();
    assert!(artists.len() >= 3, "Smart must add different artists");
    assert_eq!(spotify.received_requests().await.unwrap().len(), before);
    assert_eq!(external.received_requests().await.unwrap().len(), 2);
    app.queue.validate().unwrap();
    tasks.cycle_shuffle(&mut app);
    assert_eq!(app.current_track().unwrap().name, "Seed");
}

fn item(id: usize, name: String, artist: usize) -> serde_json::Value {
    serde_json::json!({"id":format!("{id:022}"),"name":name,"type":"track","is_playable":true,
        "duration_ms":200000,"artists":[{"id":format!("{artist:022}"),"name":format!("Artist {artist}")}]})
}

async fn settle(app: &mut App, tasks: &mut Tasks, bg: &mut mpsc::UnboundedReceiver<Background>) {
    tokio::time::timeout(Duration::from_secs(45), async {
        while app.catalog.busy || tasks.recommendations.is_some() || tasks.smart.handle.is_some() {
            background(app, tasks, bg.recv().await.expect("background closed"));
        }
    })
    .await
    .expect("application did not settle");
}

fn playing(app: &mut App, commands: &mpsc::UnboundedSender<Command>) {
    app.playback_event(
        Event::Playing {
            generation: app.generation,
            position_ms: 2000,
        },
        commands,
    );
}

#[tokio::test]
async fn four_searches_radio_refill_and_smart_use_original_artist_context() {
    let server = MockServer::start().await;
    Mock::given(path("/search"))
        .respond_with(move |request: &Request| {
            let query: std::collections::HashMap<_, _> =
                request.url.query_pairs().into_owned().collect();
            let q = &query["q"];
            let rows = if q.starts_with("track:Seed ") {
                let number = q
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                vec![item(number, format!("Seed {number}"), number)]
            } else if q.starts_with("artist:") {
                let number = q
                    .split('"')
                    .nth(1)
                    .unwrap()
                    .strip_prefix("Artist ")
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                assert!(!q.contains("genre:"));
                let offset = query["offset"].parse::<usize>().unwrap();
                (0..10)
                    .map(|i| {
                        item(
                            10000 * (number + 1) + offset + i,
                            format!("Original {number} {}", offset + i),
                            number,
                        )
                    })
                    .collect()
            } else {
                panic!("Unexpected query {q}")
            };
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"tracks":{"items":rows},"next":null}))
        })
        .mount(&server)
        .await;
    Mock::given(path("/recommendations"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let (bg_tx, mut bg) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(Catalog::mock(&server.uri()), bg_tx).unwrap();
    let (commands, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());
    for number in 0..4 {
        key(
            &mut app,
            KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE),
            &mut tasks,
            &commands,
        );
        if !app.catalog.editing {
            key(
                &mut app,
                KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
                &mut tasks,
                &commands,
            );
        }
        assert!(app.catalog.editing);
        app.catalog.query.clear();
        route_input(
            &mut app,
            Input::Paste(format!("track:Seed {number} artist:Artist {number}")),
            &mut tasks,
            &commands,
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut tasks,
            &commands,
        );
        settle(&mut app, &mut tasks, &mut bg).await;
        key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut tasks,
            &commands,
        );
        settle(&mut app, &mut tasks, &mut bg).await;
        playing(&mut app, &commands);
        assert_eq!(app.current_track().unwrap().name, format!("Seed {number}"));
        assert_eq!(app.queue.ids.len(), 16);
        assert!(
            app.queue
                .ids
                .iter()
                .all(|id| app.cache.get(id).unwrap().artist_ids == vec![format!("{number:022}")])
        );

        for _ in 0..2 {
            key(
                &mut app,
                KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
                &mut tasks,
                &commands,
            );
            playing(&mut app, &commands);
        }
        assert_eq!(app.queue.cursor, Some(2));
        app.queue.select(13);
        app.load(&commands);
        playing(&mut app, &commands);
        tasks.refill_radio(&app);
        settle(&mut app, &mut tasks, &mut bg).await;
        assert_eq!(
            tasks.radio_seed.as_ref().unwrap().id,
            format!("{number:022}")
        );
        assert_eq!(app.queue.ids.len(), 31);
        let recordings: HashSet<_> = app
            .queue
            .ids
            .iter()
            .map(|id| crate::catalog::recording_key(app.cache.get(id).unwrap()))
            .collect();
        assert_eq!(recordings.len(), 31);
        let generation = app.generation;
        app.playback_event(Event::Completed(generation), &commands);
        assert_eq!(app.queue.cursor, Some(14));
        assert!(app.generation > generation);

        app.queue.select(0);
        app.load(&commands);
        let requests_before_smart = server.received_requests().await.unwrap().len();
        tasks.cycle_shuffle(&mut app);
        tasks.cycle_shuffle(&mut app);
        settle(&mut app, &mut tasks, &mut bg).await;
        assert!(
            !app.queue.suggestions.is_empty(),
            "artist-connected candidates should fill Smart Shuffle"
        );
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            requests_before_smart,
            "Smart Shuffle should reuse the original artist catalog"
        );
        app.queue.validate().unwrap();
        let current = app.queue.current().unwrap().to_string();
        tasks.cycle_shuffle(&mut app);
        assert_eq!(app.queue.current(), Some(current.as_str()));
        tasks.cancel_radio(&mut app);
        app.stop(&commands);
    }
}

#[tokio::test]
#[ignore = "Live Spotify catalog through Tuitify input/background/queue handling; no audio or saved-state writes"]
async fn live_japanese_and_olivia_app_catalog_acceptance() -> Result<()> {
    let store = Storage::local()?;
    let catalog = Catalog::new(TokenManager::load(&store.config()?)?)?;
    let (bg_tx, mut bg) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(catalog, bg_tx)?;
    let (commands, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(Config::default(), Queue::default());
    for (query, title, artist) in [
        (
            "track:月面着陸計画 - Live artist:tuki.",
            "月面着陸計画",
            "tuki.",
        ),
        (
            "track:猫日 artist:suis from Yorushika",
            "猫日",
            "suis from Yorushika",
        ),
        (
            "track:good 4 u artist:Olivia Rodrigo",
            "good 4 u",
            "Olivia Rodrigo",
        ),
    ] {
        key(
            &mut app,
            KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE),
            &mut tasks,
            &commands,
        );
        if !app.catalog.editing {
            key(
                &mut app,
                KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
                &mut tasks,
                &commands,
            );
        }
        app.catalog.query.clear();
        route_input(&mut app, Input::Paste(query.into()), &mut tasks, &commands);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut tasks,
            &commands,
        );
        settle(&mut app, &mut tasks, &mut bg).await;
        assert!(
            matches!(&app.catalog.rows, Rows::Tracks(rows) if !rows.is_empty()),
            "search failed: {}",
            app.status
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut tasks,
            &commands,
        );
        settle(&mut app, &mut tasks, &mut bg).await;
        let seed = app.current_track().unwrap().clone();
        assert!(
            seed.name.starts_with(title) && seed.artists.contains(artist),
            "wrong seed: {} / {}",
            seed.name,
            seed.artists
        );
        assert!(!app.status.contains("rate limit"), "{}", app.status);
        assert!(app.queue.ids.len() >= 4, "sparse radio: {}", app.status);
        println!(
            "LIVE APP {} / {}: {} radio entries",
            seed.name,
            seed.artists,
            app.queue.ids.len()
        );
        for track in app.queue.ids.iter().filter_map(|id| app.cache.get(id)) {
            println!("  {} / {}", track.name, track.artists);
            if artist != "Olivia Rodrigo" {
                assert!(
                    !track.artists.contains("Taylor Swift")
                        && !track.artists.contains("Olivia Rodrigo"),
                    "Japanese screenshot regression"
                );
            }
        }
        let mut counts = std::collections::HashMap::<String, usize>::new();
        for track in app
            .queue
            .ids
            .iter()
            .skip(1)
            .filter_map(|id| app.cache.get(id))
        {
            *counts
                .entry(
                    track
                        .artist_ids
                        .first()
                        .cloned()
                        .unwrap_or_else(|| track.artists.clone()),
                )
                .or_default() += 1;
        }
        assert!(counts.len() >= 4, "single-artist regression: {counts:?}");
        assert!(
            counts.values().all(|count| *count <= 3),
            "artist domination: {counts:?}"
        );
        assert_eq!(
            app.radio_source,
            Some(crate::catalog::RecommendationSource::SimilarArtists)
        );
        println!(
            "  VARIETY {} artists, maximum {} tracks per artist",
            counts.len(),
            counts.values().max().unwrap()
        );
        let originals = app.queue.ids.iter().take(7).cloned().collect::<Vec<_>>();
        // Drive the real queue with synthetic playback acknowledgements; this
        // test deliberately leaves the user's currently running audio alone.
        playing(&mut app, &commands);
        for _ in 0..2 {
            key(
                &mut app,
                KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
                &mut tasks,
                &commands,
            );
            playing(&mut app, &commands);
        }
        assert_eq!(app.queue.cursor, Some(2));
        let initial = app.queue.ids.len();
        app.queue.select(initial - 3);
        app.load(&commands);
        playing(&mut app, &commands);
        tasks.refill_radio(&app);
        settle(&mut app, &mut tasks, &mut bg).await;
        assert!(!app.status.contains("rate limit"), "{}", app.status);
        assert_eq!(tasks.radio_seed.as_ref().unwrap().id, seed.id);
        let recordings: HashSet<_> = app
            .queue
            .ids
            .iter()
            .map(|id| crate::catalog::recording_key(app.cache.get(id).unwrap()))
            .collect();
        assert_eq!(recordings.len(), app.queue.ids.len());
        println!(
            "  REFILL added={}, anchored seed, no duplicates",
            app.queue.ids.len() - initial
        );
        tasks.cancel_radio(&mut app);
        app.queue.replace(originals, 0, false);
        app.config.shuffle = false;
        app.load(&commands);
        tasks.cycle_shuffle(&mut app);
        tasks.cycle_shuffle(&mut app);
        settle(&mut app, &mut tasks, &mut bg).await;
        assert!(!app.status.contains("rate limit"), "{}", app.status);
        assert!(
            !app.queue.suggestions.is_empty(),
            "Smart had no candidates: {}",
            app.status
        );
        for index in &app.queue.suggestions {
            let track = app.cache.get(&app.queue.ids[*index]).unwrap();
            println!("  SMART {} / {}", track.name, track.artists);
        }
        let smart_artists: HashSet<_> = app
            .queue
            .suggestions
            .iter()
            .map(|index| {
                let track = app.cache.get(&app.queue.ids[*index]).unwrap();
                assert!(
                    !track
                        .artist_ids
                        .iter()
                        .any(|id| seed.artist_ids.contains(id)),
                    "Smart must discover other artists"
                );
                track
                    .artist_ids
                    .first()
                    .cloned()
                    .unwrap_or_else(|| track.artists.clone())
            })
            .collect();
        assert!(
            smart_artists.len() >= 2,
            "Smart needs multiple other artists"
        );
        app.queue.validate()?;
        let current = app.queue.current().unwrap().to_string();
        tasks.cycle_shuffle(&mut app);
        assert_eq!(app.queue.current(), Some(current.as_str()));
        assert!(app.queue.suggestions.is_empty());
        app.stop(&commands);
    }
    Ok(())
}
