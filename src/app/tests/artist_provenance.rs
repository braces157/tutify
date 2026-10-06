use super::*;
use crate::catalog::ArtistResultSource;
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{path, query_param},
};

const ARTIST: &str = "0000000000000000000500";

fn track_json(index: usize, artist: &str) -> serde_json::Value {
    json!({"id":format!("{index:022}"), "name":format!("Song {index}"), "type":"track",
        "artists":[{"id":artist,"name":"Target Artist"}], "is_playable":true})
}

fn draw_text(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| ui::draw(f, app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}

async fn apply_result(
    app: &mut App,
    tasks: &mut Tasks,
    rx: &mut mpsc::UnboundedReceiver<Background>,
) {
    let event = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(&event, Background::Page(_, Ok(_))));
    background(app, tasks, event);
}

#[tokio::test]
async fn artist_fallback_provenance_survives_empty_pages_pagination_history_and_f5_source_changes()
{
    let server = MockServer::start().await;
    Mock::given(path(format!("/artists/{ARTIST}/top-tracks")))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path(format!("/artists/{ARTIST}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name":"Target Artist"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/search"))
        .and(query_param("offset", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"tracks":{
            "items":[track_json(1,"0000000000000000000600")],"next":"next"
        }})))
        .expect(1)
        .mount(&server)
        .await;
    let (events, mut rx) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(Catalog::mock(&server.uri()), events).unwrap();
    let mut app = App::new(Config::default(), Queue::default());
    let mut seed = test_track(1);
    seed.artist_ids = vec![ARTIST.into()];
    seed.artists = "Target Artist".into();
    app.catalog.rows = Rows::Tracks(vec![seed]);
    let (commands, mut command_rx) = mpsc::unbounded_channel();
    actions::apply(&mut app, Action::ViewArtist, &mut tasks, &commands);
    assert_eq!(app.catalog.title, "Target Artist");
    assert_eq!(app.catalog.artist_source, None);
    assert!(!draw_text(&app, 100, 30).contains("Top Tracks"));
    apply_result(&mut app, &mut tasks, &mut rx).await;
    assert_eq!(
        app.catalog.artist_source,
        Some(ArtistResultSource::ArtistSearch)
    );
    assert_eq!(app.catalog.title, "Target Artist • Artist Search");
    assert_eq!(app.len(), 0);
    assert_eq!(app.catalog.next, Some(10));
    let empty = draw_text(&app, 100, 30);
    assert!(empty.contains("No verified artist matches"), "{empty}");
    assert!(empty.contains("Continue Artist Search"));
    assert!(!empty.contains("Top Tracks") && !empty.contains("top tracks"));
    server.verify().await;
    server.reset().await;

    // Access may change between pages. This must not append Top Tracks to
    // Artist Search or reinterpret the row order after an observation expires.
    tasks.catalog.refresh_capabilities();
    Mock::given(path(format!("/artists/{ARTIST}/top-tracks")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"tracks":[track_json(99,ARTIST)]})),
        )
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(path(format!("/artists/{ARTIST}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name":"Target Artist"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/search"))
        .and(query_param("offset", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"tracks":{
            "items":[track_json(2,ARTIST),track_json(3,"0000000000000000000600")],"next":null
        }})))
        .expect(1)
        .mount(&server)
        .await;
    key(
        &mut app,
        KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    apply_result(&mut app, &mut tasks, &mut rx).await;
    assert_eq!(app.len(), 1);
    assert_eq!(
        app.catalog.artist_source,
        Some(ArtistResultSource::ArtistSearch)
    );
    assert!(app.status.starts_with("Artist Search |"));
    app.status = "Volume 50%".into();
    let text = draw_text(&app, 100, 30);
    assert!(text.contains("ROW") && text.contains("search order"));
    assert!(!text.contains("Top Tracks"));
    for (width, height) in [(32, 10), (48, 18), (80, 24), (120, 35)] {
        assert!(draw_text(&app, width, height).contains("Artist Search"));
    }
    app.push_navigation(app.catalog.title.clone());
    app.catalog.view = View::Album;
    app.reset_rows();
    app.catalog.artist_label = "Other Artist".into();
    assert!(app.pop_navigation());
    assert_eq!(
        app.catalog.artist_source,
        Some(ArtistResultSource::ArtistSearch)
    );
    assert_eq!(app.catalog.artist_label, "Target Artist");
    assert!(draw_text(&app, 100, 30).contains("Artist Search"));
    server.verify().await;
    server.reset().await;
    Mock::given(path(format!("/artists/{ARTIST}/top-tracks")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"tracks":[track_json(99,ARTIST)]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/search"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let stale_request = app.catalog.request;
    key(
        &mut app,
        KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE),
        &mut tasks,
        &commands,
    );
    // Old rows remain truthfully labelled while refresh is in flight.
    assert_eq!(
        app.catalog.artist_source,
        Some(ArtistResultSource::ArtistSearch)
    );
    apply_result(&mut app, &mut tasks, &mut rx).await;
    assert_eq!(
        app.catalog.artist_source,
        Some(ArtistResultSource::TopTracks)
    );
    assert_eq!(app.catalog.title, "Target Artist • Top Tracks");
    let Rows::Tracks(rows) = &app.catalog.rows else {
        panic!("tracks expected")
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, format!("{:022}", 99));
    background(
        &mut app,
        &mut tasks,
        Background::Page(
            stale_request,
            Ok(Page {
                rows: Rows::Tracks(vec![]),
                offset: 0,
                next: None,
                artist_source: Some(ArtistResultSource::ArtistSearch),
            }),
        ),
    );
    assert_eq!(
        app.catalog.artist_source,
        Some(ArtistResultSource::TopTracks)
    );
    assert_eq!(app.len(), 1);
    assert!(!draw_text(&app, 100, 30).contains("search order"));
    assert!(command_rx.try_recv().is_err());
}

#[tokio::test]
async fn empty_artist_sources_have_distinct_labels_and_failures_do_not_invent_provenance() {
    for source in [
        ArtistResultSource::TopTracks,
        ArtistResultSource::ArtistSearch,
        ArtistResultSource::Demo,
    ] {
        let mut app = App::new(Config::default(), Queue::default());
        let (mut tasks, _) = tasks();
        app.catalog.view = View::Artist;
        app.catalog.browse = Browse::Artist(ARTIST.into());
        background(
            &mut app,
            &mut tasks,
            Background::Page(
                0,
                Ok(Page {
                    rows: Rows::Tracks(vec![]),
                    offset: 0,
                    next: None,
                    artist_source: Some(source),
                }),
            ),
        );
        let text = draw_text(&app, 100, 30);
        match source {
            ArtistResultSource::YoutubeMusic => {
                assert!(text.contains("No songs returned by YouTube Music"))
            }
            ArtistResultSource::TopTracks => assert!(text.contains("No top tracks found")),
            ArtistResultSource::ArtistSearch => {
                assert!(text.contains("No verified Artist Search matches"))
            }
            ArtistResultSource::Demo => assert!(text.contains("No demo tracks found")),
        }
        assert!(app.catalog.title.ends_with(source.label()));
    }
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    app.catalog.view = View::Artist;
    app.catalog.browse = Browse::Artist(ARTIST.into());
    app.catalog.title = "Artist".into();
    background(
        &mut app,
        &mut tasks,
        Background::Page(0, Err(anyhow::anyhow!("Service unavailable"))),
    );
    assert_eq!(app.catalog.artist_source, None);
    let text = draw_text(&app, 100, 30);
    assert!(text.contains("Artist tracks unavailable"));
    assert!(!text.contains("Top Tracks") && !text.contains("top tracks"));
}
