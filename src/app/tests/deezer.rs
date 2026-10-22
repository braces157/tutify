use super::*;
use crate::deezer::tests::item;
use serde_json::json;
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::path};

#[tokio::test]
async fn deezer_search_starts_radio_and_continues_on_the_youtube_audio_source() {
    let server = MockServer::start().await;
    let native = MockServer::start().await;
    let mut song = item(1, "Song");
    song["readable"] = json!(false);
    Mock::given(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[song]})))
        .mount(&server)
        .await;
    Mock::given(path("/artist/10/related"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[{"id":11},{"id":12},{"id":13},{"id":14},{"id":15}]})),
        )
        .mount(&server)
        .await;
    for artist in 11..=15 {
        Mock::given(path(format!("/artist/{artist}/top")))
            .respond_with(move |request: &Request| {
                let query: std::collections::HashMap<_, _> =
                    request.url.query_pairs().into_owned().collect();
                let offset: u64 = query["index"].parse().unwrap();
                let songs: Vec<_> = (0..20)
                    .map(|i| {
                        let mut song = item(
                            artist * 1000 + offset + i,
                            &format!("Related {artist} {}", offset + i),
                        );
                        song["readable"] = json!(false);
                        song["artist"] =
                            json!({"id":artist,"name":format!("Related artist {artist}")});
                        song
                    })
                    .collect();
                ResponseTemplate::new(200).set_body_json(json!({"data":songs}))
            })
            .mount(&server)
            .await;
    }
    let (bg_tx, mut bg) = mpsc::unbounded_channel();
    let catalog =
        Catalog::mock(&native.uri()).with_deezer(crate::deezer::Client::mock(&server.uri()));
    let mut tasks = Tasks::new(catalog, bg_tx).unwrap();
    let mut app = App::new(
        Config {
            deezer_search: true,
            source: crate::model::MusicSource::Youtube,
            ..Config::default()
        },
        Queue::default(),
    );
    app.catalog.search_scope = SearchScope::Deezer;
    app.catalog.query = "Song".into();
    app.catalog.browse = Browse::Search("Song".into());
    let (tx, mut commands) = mpsc::unbounded_channel();
    tasks.request(&mut app, 0);
    let event = tokio::time::timeout(Duration::from_secs(2), bg.recv())
        .await
        .unwrap()
        .unwrap();
    background(&mut app, &mut tasks, event);
    assert_eq!(app.catalog.title, "Deezer search");
    actions::apply(&mut app, Action::PlaySelected, &mut tasks, &tx);
    assert!(matches!(commands.try_recv().unwrap(),Command::Load {id,..} if id=="deezer:1"));
    let event = tokio::time::timeout(Duration::from_secs(2), bg.recv())
        .await
        .unwrap()
        .unwrap();
    background(&mut app, &mut tasks, event);
    assert_eq!(app.queue.ids.len(), 16);
    let mut artist_counts = std::collections::HashMap::<String, usize>::new();
    for track in app
        .queue
        .ids
        .iter()
        .skip(1)
        .filter_map(|id| app.cache.get(id))
    {
        *artist_counts
            .entry(track.artist_ids[0].clone())
            .or_default() += 1;
    }
    assert_eq!(artist_counts.len(), 5);
    assert!(artist_counts.values().all(|count| *count <= 3));
    app.queue.select(15);
    app.state = State::Playing;
    tasks.refill_radio(&app);
    app.playback_event(Event::Completed(app.generation), &tx);
    let event = tokio::time::timeout(Duration::from_secs(2), bg.recv())
        .await
        .unwrap()
        .unwrap();
    background(&mut app, &mut tasks, event);
    tasks.update_autoplay(&mut app, &tx);
    assert_eq!(app.state, State::Loading);
    assert_eq!(app.queue.cursor, Some(16));
    assert_eq!(tasks.radio_seed.as_ref().unwrap().id, "deezer:1");
    tasks.cycle_shuffle(&mut app);
    tasks.cycle_shuffle(&mut app);
    assert!(app.queue.smart_shuffle);
    let event = tokio::time::timeout(Duration::from_secs(2), bg.recv())
        .await
        .unwrap()
        .unwrap();
    background(&mut app, &mut tasks, event);
    assert!(!app.queue.suggestions.is_empty());
    let suggested_artists: HashSet<_> = app
        .queue
        .suggestions
        .iter()
        .filter_map(|&index| app.cache.get(&app.queue.ids[index]))
        .map(|track| track.artist_ids[0].clone())
        .collect();
    assert!(suggested_artists.len() >= 2);
    assert!(native.received_requests().await.unwrap().is_empty());
    app.queue.validate().unwrap();
}
