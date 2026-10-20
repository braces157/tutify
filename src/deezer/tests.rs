use super::*;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header_exists, path, query_param},
};

pub(crate) fn item(id: u64, title: &str) -> Value {
    json!({"id":id,"title":title,"type":"track","duration":200,"readable":true,
        "artist":{"id":10,"name":"Artist"},"album":{"id":20,"title":"Album","cover_medium":"https://example.com/cover.jpg"}})
}

#[test]
fn identities_links_and_recording_codes_remain_scoped() {
    for id in ["deezer:1", "deezer:138547415"] {
        assert!(crate::model::valid_track_id(id));
    }
    for id in [
        "deezer:0",
        "deezer:-1",
        "deezer:1/stream",
        "deezer:../token",
        "deezer:18446744073709551616",
        "1",
    ] {
        assert!(!crate::model::valid_track_id(id), "{id}");
    }
    assert_eq!(
        track_id("https://www.deezer.com/en/track/123?x=1"),
        Some("deezer:123".into())
    );
    assert!(track_id("https://deezer.com.attacker.test/track/123").is_none());
    assert!(track_id("https://user@deezer.com/track/123").is_none());
    assert_eq!(
        crate::model::track_url("deezer:123").unwrap(),
        "https://www.deezer.com/track/123"
    );
    assert_eq!(isrc("gbaye9200070"), Some("GBAYE9200070".into()));
    assert!(isrc("US/../../token").is_none());
}

#[tokio::test]
async fn search_pages_are_public_cached_and_keep_provider_offsets() {
    let server = MockServer::start().await;
    Mock::given(path("/search")).and(query_param("index", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[item(1,"猫"),{"id":2}],"next":"https://api.deezer.com/search?index=2","total":3})))
        .expect(1).mount(&server).await;
    Mock::given(path("/search"))
        .and(query_param("index", "2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data":[item(3,"Mưa")],"total":3})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let client = Client::mock(&server.uri());
    let browse = Browse::Search("artist".into());
    for _ in 0..2 {
        let first = client.page(&browse, 0).await.unwrap();
        assert_eq!(first.next, Some(2));
        let Rows::Tracks(tracks) = first.rows else {
            panic!()
        };
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].name, "猫");
        assert_eq!(tracks[0].album_id.as_deref(), Some("deezer:20"));
    }
    let second = client.page(&browse, 2).await.unwrap();
    assert!(second.next.is_none());
}

#[tokio::test]
async fn album_tracks_inherit_album_and_related_artists_drive_radio() {
    let server = MockServer::start().await;
    let mut song = item(1, "Song");
    song.as_object_mut().unwrap().remove("album");
    Mock::given(path("/album/20"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id":20,"title":"Album","tracks":{"data":[song]}})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/artist/10/related"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":11},{"id":12}]})),
        )
        .mount(&server)
        .await;
    for artist in [11, 12] {
        let mut songs = vec![];
        for i in 0..4 {
            let mut song = item(artist * 10 + i, &format!("Song {artist} {i}"));
            song["artist"] = json!({"id":artist,"name":format!("Artist {artist}")});
            songs.push(song);
        }
        Mock::given(path(format!("/artist/{artist}/top")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":songs})))
            .mount(&server)
            .await;
    }
    let client = Client::mock(&server.uri());
    let page = client
        .page(&Browse::Album("deezer:20".into()), 0)
        .await
        .unwrap();
    let Rows::Tracks(tracks) = page.rows else {
        panic!()
    };
    assert_eq!(tracks[0].album.as_deref(), Some("Album"));
    let recs = client.recommendations(&tracks[0], &[], 0).await.unwrap();
    assert_eq!(recs.source, RecommendationSource::Deezer);
    assert_eq!(recs.tracks.len(), 8);
    assert_ne!(recs.tracks[0].artists, recs.tracks[1].artists);
    assert!(
        client
            .recommendations(&tracks[0], &recs.tracks, 0)
            .await
            .unwrap()
            .tracks
            .is_empty()
    );
    let mut q = crate::queue::Queue::default();
    q.replace(
        recs.tracks.iter().map(|track| track.id.clone()).collect(),
        0,
        false,
    );
    let restored: crate::queue::Queue =
        serde_json::from_str(&serde_json::to_string(&q).unwrap()).unwrap();
    restored.validate().unwrap();
}

#[tokio::test]
async fn service_errors_are_redacted_and_cooldown_prevents_retry_loops() {
    let server = MockServer::start().await;
    Mock::given(path("/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"error":{"code":4,"message":"secret-body"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::mock(&server.uri());
    for _ in 0..2 {
        let error = client
            .page(&Browse::Search("song".into()), 0)
            .await
            .unwrap_err();
        assert!(ServiceFailure::is(&error, FailureKind::RateLimited));
        assert!(!format!("{error:#}").contains("secret-body"));
    }
}

#[tokio::test]
#[ignore = "Read-only live Deezer catalog acceptance; no accounts, audio, or saved-state writes"]
async fn live_deezer_multilingual_search_and_discovery() -> Result<()> {
    let client = Client::new()?;
    for query in [
        "Radiohead Creep",
        "Mưa Tháng Sáu Văn Mai Hương",
        "ヨルシカ だから僕は音楽を辞めた",
    ] {
        let page = client.page(&Browse::Search(query.into()), 0).await?;
        let Rows::Tracks(tracks) = page.rows else {
            unreachable!()
        };
        ensure!(!tracks.is_empty(), "Empty Deezer search for {query}");
        let (track, code) = client.recording(&tracks[0].id).await?;
        println!(
            "{query}: {} / {} ({}, ISRC {})",
            track.name,
            track.artists,
            track.id,
            code.is_some()
        );
        let recs = client.recommendations(&track, &tracks, 0).await?;
        ensure!(!recs.tracks.is_empty(), "No related recordings for {query}");
        println!("  {} related songs", recs.tracks.len());
    }
    Ok(())
}
