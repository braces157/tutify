use super::*;
use crate::deezer::tests::item;
use serde_json::json;
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::path};

#[tokio::test]
async fn deezer_search_starts_radio_and_continues_on_the_youtube_audio_source() {
    let server = MockServer::start().await;
    let native = MockServer::start().await;
    Mock::given(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[item(1,"Song")]})))
        .mount(&server)
        .await;
    Mock::given(path("/artist/10/related"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":11}]})))
        .mount(&server)
        .await;
    Mock::given(path("/artist/11/top"))
        .respond_with(|request: &Request| {
            let query: std::collections::HashMap<_, _> =
                request.url.query_pairs().into_owned().collect();
            let offset: u64 = query["index"].parse().unwrap();
            let songs: Vec<_> = (0..20)
                .map(|i| {
                    let mut song = item(100 + offset + i, &format!("Related {}", offset + i));
                    song["artist"] = json!({"id":11,"name":"Related artist"});
                    song
                })
                .collect();
            ResponseTemplate::new(200).set_body_json(json!({"data":songs}))
        })
        .mount(&server)
        .await;
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
    assert!(native.received_requests().await.unwrap().is_empty());
    app.queue.validate().unwrap();
}
