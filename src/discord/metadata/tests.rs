use super::*;
use crate::discord::{Snapshot, build_activity};
use crate::model::PlaybackState;
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{path, query_param},
};

const SPOTIFY_ID: &str = "0000000000000000000001";

fn original(id: &str) -> Track {
    Track {
        id: id.into(),
        name: "Song".into(),
        artists: "Artist".into(),
        album: Some("Album".into()),
        duration_ms: 200_000,
        playable: true,
        ..Default::default()
    }
}

fn spotify_track() -> Value {
    json!({"id":SPOTIFY_ID,"name":"Song","artists":[{"id":"0000000000000000000010","name":"Artist"}],
        "duration_ms":201000,"is_playable":false,"album":{"id":"0000000000000000000020","name":"Album",
        "images":[{"url":"https://example.com/spotify.jpg"}]}})
}

async fn resolver() -> (MockServer, MockServer, Metadata) {
    let spotify = MockServer::start().await;
    let deezer = MockServer::start().await;
    let catalog =
        Catalog::mock(&spotify.uri()).with_deezer(crate::deezer::Client::mock(&deezer.uri()));
    let metadata = Metadata::new(&crate::storage::Config::default(), &catalog);
    (spotify, deezer, metadata)
}

#[tokio::test]
async fn spotify_is_preferred_cached_and_only_changes_discord_metadata() {
    let (spotify, deezer, mut metadata) = resolver().await;
    Mock::given(path("/search"))
        .and(query_param("q", "track:\"Song\" artist:\"Artist\""))
        .and(query_param("type", "track"))
        .and(query_param("limit", "10"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"tracks":{"items":[spotify_track()]}})),
        )
        .expect(1)
        .mount(&spotify)
        .await;
    let source = original("deezer:1");
    let resolved = metadata.resolve(&source).await;
    assert_eq!(resolved.id, SPOTIFY_ID);
    assert_eq!(resolved.duration_ms, source.duration_ms);
    assert_eq!(metadata.resolve(&source).await, resolved);
    assert_eq!(source.id, "deezer:1");
    assert!(deezer.received_requests().await.unwrap().is_empty());
    let activity = build_activity(
        &Snapshot {
            track: Some(resolved),
            state: PlaybackState::Playing,
            position_ms: 10_000,
            quality: None,
        },
        1000,
        None,
    )
    .unwrap();
    assert_eq!(
        activity["details_url"],
        format!("https://open.spotify.com/track/{SPOTIFY_ID}")
    );
    assert_eq!(
        activity["state_url"],
        "https://open.spotify.com/artist/0000000000000000000010"
    );
    assert_eq!(
        activity["assets"]["large_url"],
        "https://open.spotify.com/album/0000000000000000000020"
    );
    assert_eq!(
        activity["assets"]["large_image"],
        "https://example.com/spotify.jpg"
    );
    assert_eq!(activity["timestamps"]["end"], 1190);
}

#[tokio::test]
async fn missing_denied_or_rate_limited_spotify_falls_back_to_deezer() {
    for status in [200, 403, 429] {
        let (spotify, deezer, mut metadata) = resolver().await;
        Mock::given(path("/search"))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(json!({"tracks":{"items":[]}})),
            )
            .expect(1)
            .mount(&spotify)
            .await;
        Mock::given(path("/search"))
            .and(query_param("q", "Song Artist"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"data":[crate::deezer::tests::item(2, "Song")]})),
            )
            .expect(1)
            .mount(&deezer)
            .await;
        let source = original("volta:recording");
        let resolved = metadata.resolve(&source).await;
        assert_eq!(resolved.id, "deezer:2");
        assert_eq!(metadata.resolve(&source).await, resolved);
        let activity = build_activity(
            &Snapshot {
                track: Some(resolved),
                state: PlaybackState::Paused,
                position_ms: 0,
                quality: None,
            },
            1000,
            None,
        )
        .unwrap();
        assert_eq!(activity["details_url"], "https://www.deezer.com/track/2");
        assert_eq!(
            activity["assets"]["large_url"],
            "https://www.deezer.com/album/20"
        );
        assert_eq!(activity["state_url"], "https://www.deezer.com/artist/10");
    }
}

#[tokio::test]
async fn existing_deezer_metadata_survives_a_bounded_spotify_timeout() {
    let (spotify, deezer, mut metadata) = resolver().await;
    Mock::given(path("/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"tracks":{"items":[spotify_track()]}}))
                .set_delay(IO_TIMEOUT + Duration::from_secs(1)),
        )
        .expect(1)
        .mount(&spotify)
        .await;
    let source = original("deezer:1");
    let resolved = tokio::time::timeout(
        IO_TIMEOUT + Duration::from_secs(1),
        metadata.resolve(&source),
    )
    .await
    .unwrap();
    assert_eq!(resolved, source);
    assert!(deezer.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn wrong_versions_artists_lengths_and_ambiguous_matches_keep_the_original() {
    for ambiguous in [false, true] {
        let (spotify, deezer, mut metadata) = resolver().await;
        let mut wrong_version = spotify_track();
        wrong_version["name"] = json!("Song (Live)");
        let mut wrong_artist = spotify_track();
        wrong_artist["artists"][0]["name"] = json!("Cover Artist");
        let mut wrong_length = spotify_track();
        wrong_length["duration_ms"] = json!(300000);
        let candidates = if ambiguous {
            let mut second = spotify_track();
            second["id"] = json!("0000000000000000000002");
            vec![spotify_track(), second]
        } else {
            vec![wrong_version, wrong_artist, wrong_length]
        };
        Mock::given(path("/search"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"tracks":{"items":candidates}})),
            )
            .expect(1)
            .mount(&spotify)
            .await;
        Mock::given(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[
                crate::deezer::tests::item(2, "Song (Live)"), crate::deezer::tests::item(3, "Other song")]})))
            .expect(1).mount(&deezer).await;
        let source = original("youtube:dQw4w9WgXcQ");
        assert_eq!(metadata.resolve(&source).await, source);
    }
}

#[tokio::test]
async fn absent_spotify_connection_uses_deezer_without_requiring_login() {
    let (_, deezer, mut metadata) = resolver().await;
    metadata.spotify = None;
    Mock::given(path("/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[crate::deezer::tests::item(2, "Song")]})),
        )
        .expect(1)
        .mount(&deezer)
        .await;
    assert_eq!(
        metadata.resolve(&original("volta:recording")).await.id,
        "deezer:2"
    );
}

#[tokio::test]
async fn known_spotify_and_invalid_metadata_make_no_lookup_requests() {
    let (spotify, deezer, mut metadata) = resolver().await;
    for id in [SPOTIFY_ID, "https://untrusted.example/song", "deezer:1"] {
        let mut source = original(id);
        if id == "deezer:1" {
            source.artists.clear();
        }
        assert_eq!(metadata.resolve(&source).await, source);
    }
    assert!(spotify.received_requests().await.unwrap().is_empty());
    assert!(deezer.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn cache_expires_rechecks_corrected_metadata_and_stays_bounded() {
    let (spotify, _, mut metadata) = resolver().await;
    Mock::given(path("/search"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"tracks":{"items":[spotify_track()]}})),
        )
        .expect(3)
        .mount(&spotify)
        .await;
    let mut source = original("deezer:1");
    assert_eq!(metadata.resolve(&source).await.id, SPOTIFY_ID);
    metadata.cache.get_mut(&source.id).unwrap().at = Instant::now() - Duration::from_secs(301);
    assert_eq!(metadata.resolve(&source).await.id, SPOTIFY_ID);
    source.name = "Corrected title".into();
    assert_eq!(metadata.resolve(&source).await, source);

    let mut metadata = Metadata::default();
    for id in 1..=257 {
        metadata.resolve(&original(&format!("deezer:{id}"))).await;
    }
    assert_eq!(metadata.cache.len(), 1);
}

#[tokio::test]
async fn skip_during_lookup_publishes_the_newest_song_and_pause_state() {
    use crate::discord::{Update, read_packet, send_packet, worker_with_connector};
    use std::sync::Arc;
    use tokio::{
        net::windows::named_pipe::{ClientOptions, ServerOptions},
        sync::{Notify, watch},
    };

    let (spotify, _, metadata) = resolver().await;
    let started = Arc::new(Notify::new());
    let request_started = started.clone();
    Mock::given(path("/search"))
        .respond_with(move |_: &wiremock::Request| {
            request_started.notify_one();
            ResponseTemplate::new(200)
                .set_body_json(json!({"tracks":{"items":[spotify_track()]}}))
                .set_delay(Duration::from_millis(150))
        })
        .expect(1)
        .mount(&spotify)
        .await;
    let pipe_name = format!(
        r"\\.\pipe\tuitify-test-presence-metadata-{}",
        std::process::id()
    );
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_name)
        .unwrap();
    let (tx, mut rx) = watch::channel(Some(Update {
        snapshot: Snapshot {
            track: Some(original("deezer:1")),
            state: PlaybackState::Playing,
            position_ms: 0,
            quality: None,
        },
        captured: Instant::now(),
    }));
    let mut task = tokio::spawn(async move {
        worker_with_connector(
            &mut rx,
            "123".into(),
            || {
                let name = pipe_name.clone();
                async move { ClientOptions::new().open(&name) }
            },
            Duration::from_millis(30),
            metadata,
        )
        .await;
    });
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        server.connect().await.unwrap();
        assert_eq!(read_packet(&mut server).await.unwrap().0, 0);
        send_packet(&mut server, 1, r#"{"cmd":"DISPATCH","evt":"READY"}"#)
            .await
            .unwrap();
        started.notified().await;
        let mut next = original("0000000000000000000002");
        next.name = "Next song".into();
        tx.send_replace(Some(Update {
            snapshot: Snapshot {
                track: Some(next),
                state: PlaybackState::Paused,
                position_ms: 5000,
                quality: None,
            },
            captured: Instant::now(),
        }));
        let (_, payload) = read_packet(&mut server).await.unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["args"]["activity"]["details"], "Next song");
        assert_eq!(
            payload["args"]["activity"]["details_url"],
            "https://open.spotify.com/track/0000000000000000000002"
        );
        assert_eq!(payload["args"]["activity"]["state"], "Paused • Artist");
        assert!(payload["args"]["activity"].get("timestamps").is_none());
        send_packet(
            &mut server,
            1,
            &json!({"cmd":"SET_ACTIVITY","nonce":payload["nonce"]}).to_string(),
        )
        .await
        .unwrap();
        drop(tx);
        (&mut task).await.unwrap();
    })
    .await;
    if result.is_err() {
        task.abort();
    }
    result.unwrap();
}
