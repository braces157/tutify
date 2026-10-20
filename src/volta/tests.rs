use super::*;

#[tokio::test]
#[ignore = "Live personal Volta API; reads one stream response, no saved-state writes"]
async fn live_reported_audio_quality() {
    let audio = Client::load()
        .unwrap()
        .stream("volta:JPW462364925")
        .await
        .unwrap();
    assert!(audio.quality.sample_rate.is_some());
    println!("Reported stream quality: {}", audio.quality.label());
}
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path, query_param},
};

fn client(server: &MockServer) -> Client {
    Client::new(
        Credentials {
            client_id: "volta_id_test".into(),
            client_secret: "volta_sk_not-a-real-secret".into(),
        },
        &format!("{}/api/v1", server.uri()),
    )
    .unwrap()
}
async fn token(server: &MockServer, scopes: &str) {
    Mock::given(method("POST"))
        .and(path("/api/v1/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"access_token":"volta_at_test", "scope":scopes, "expires_in":3600}),
        ))
        .expect(1)
        .mount(server)
        .await;
}

#[test]
fn identities_are_namespaced_and_cannot_inject_paths() {
    assert_eq!(
        track_id("JPW462364925").as_deref(),
        Some("volta:JPW462364925")
    );
    assert_eq!(
        track_id("volta:JPW462364925").as_deref(),
        Some("volta:JPW462364925")
    );
    assert!(crate::model::valid_track_id("volta:JPW462364925"));
    for input in [
        "",
        "volta:",
        "volta:../oauth/token",
        "volta:x?quality=low",
        "volta:x\r\nCookie:x",
        "https://other.example/song",
    ] {
        assert!(key(input).is_none());
    }
}
#[test]
fn metadata_preserves_music_names_and_provider_identity() {
    let track = catalog::parse_track(&json!({"id":"JPU902501324", "title":"Seimei", "artist":"tuki.", "album":"Seimei", "album_id":"vc_-12", "duration_ms":177000, "artist_id":"tuki", "cover_url":"https://example.com/art.jpg"})).unwrap();
    assert_eq!(track.id, "volta:JPU902501324");
    assert_eq!(track.duration_ms, 177000);
    assert_eq!(track.artist_ids, ["volta:tuki"]);
    assert_eq!(track.album_id.as_deref(), Some("volta:vc_-12"));
}
#[tokio::test]
async fn authentication_reuses_token_and_never_sends_client_secret_to_catalogue() {
    let server = MockServer::start().await;
    token(&server, "catalog:read stream:read").await;
    Mock::given(method("GET")).and(path("/api/v1/search")).and(header("Authorization", "Bearer volta_at_test")).and(query_param("q", "tuki"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results":{"tracks":[{"id":"JPW462364925", "name":"Song", "artist":"tuki.", "duration_ms":242000}]}}))).expect(2).mount(&server).await;
    let client = client(&server);
    for _ in 0..2 {
        let page = client
            .page(&crate::catalog::Browse::Search("tuki".into()), 0)
            .await
            .unwrap();
        assert!(matches!(page.rows, crate::catalog::Rows::Tracks(t) if t.len()==1));
    }
    for request in server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method == "GET")
    {
        assert!(!String::from_utf8_lossy(&request.body).contains("volta_sk_"));
        assert!(!request.url.as_str().contains("volta_sk_"));
        assert!(
            request
                .headers
                .get("user-agent")
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("Tuitify/")
        );
    }
}
#[tokio::test]
async fn stream_preserves_session_cookie_and_native_high_resolution_rate() {
    let server = MockServer::start().await;
    token(&server, "catalog:read stream:read").await;
    Mock::given(method("GET")).and(path("/api/v1/stream")).and(query_param("quality", "lossless_hq")).and(query_param("track_id", "JPW462364925"))
        .respond_with(ResponseTemplate::new(200).insert_header("set-cookie", "volta_stream=bound-session; HttpOnly; Path=/api/v1/stream_relay")
            .set_body_json(json!({"stream_url":"/api/v1/stream_relay?url=opaque&sig=test&exp=9999999999", "bit_depth":24, "sample_rate":384000, "source":"arcod_qobuz"}))).expect(1).mount(&server).await;
    let audio = client(&server).stream("volta:JPW462364925").await.unwrap();
    assert_eq!(audio.quality.sample_rate, Some(384000));
    assert!(
        audio
            .headers
            .contains("Cookie: volta_stream=bound-session\r\n")
    );
    assert!(!audio.headers.contains("HttpOnly"));
    assert!(!audio.headers.contains("Authorization"));
    assert!(audio.fresh());
}
#[tokio::test]
async fn stream_rejects_external_links_before_forwarding_cookies() {
    let server = MockServer::start().await;
    token(&server, "catalog:read stream:read").await;
    Mock::given(path("/api/v1/stream")).respond_with(ResponseTemplate::new(200).insert_header("set-cookie", "private=secret")
        .set_body_json(json!({"stream_url":"https://untrusted.example/audio.flac", "sample_rate":44100}))).mount(&server).await;
    let error = client(&server)
        .stream("volta:JPW462364925")
        .await
        .err()
        .unwrap();
    assert!(!error.to_string().contains("secret"));
    assert!(error.to_string().contains("unsupported audio relay"));
}
#[tokio::test]
async fn stream_accepts_application_relay_on_the_same_api_origin() {
    let server = MockServer::start().await;
    token(&server, "catalog:read stream:read").await;
    let relay = format!("{}/api/v1/app_stream_relay?sig=temporary", server.uri());
    Mock::given(path("/api/v1/stream"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("set-cookie", "volta_stream=application-session; HttpOnly")
                .set_body_json(json!({"stream_url":relay, "sample_rate":44100})),
        )
        .mount(&server)
        .await;
    let audio = client(&server).stream("volta:JPW462364925").await.unwrap();
    assert_eq!(audio.url, relay);
    assert!(
        audio
            .headers
            .contains("Cookie: volta_stream=application-session\r\n")
    );
}
#[tokio::test]
async fn missing_audio_scope_stops_before_any_catalogue_or_stream_request() {
    let server = MockServer::start().await;
    token(&server, "catalog:read").await;
    let error = client(&server)
        .stream("volta:JPW462364925")
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("stream:read"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
#[tokio::test]
async fn denied_requests_are_not_retried_and_never_export_upstream_secrets() {
    let server = MockServer::start().await;
    token(&server, "catalog:read stream:read").await;
    Mock::given(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(403).set_body_string("private-token-in-upstream-body"))
        .expect(1)
        .mount(&server)
        .await;
    let error = client(&server).get("/search", &[]).await.err().unwrap();
    assert!(crate::service::ServiceFailure::is(
        &error,
        crate::service::FailureKind::AccessRestricted
    ));
    assert!(!error.to_string().contains("private-token"));
}
#[tokio::test]
async fn rate_limit_blocks_subsequent_requests_instead_of_retrying() {
    let server = MockServer::start().await;
    token(&server, "catalog:read stream:read").await;
    Mock::given(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "60"))
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server);
    for _ in 0..2 {
        assert!(crate::service::ServiceFailure::is(
            &client.get("/search", &[]).await.err().unwrap(),
            crate::service::FailureKind::RateLimited
        ));
    }
}
#[tokio::test]
async fn separate_source_storage_keeps_existing_queue_and_credentials() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::storage::Storage {
        root: temp.path().to_owned(),
    };
    root.save_config(&crate::storage::Config::default())
        .unwrap();
    let volta = root.volta().unwrap();
    assert_eq!(volta.root, temp.path().join("volta"));
    assert!(volta.queue().unwrap().ids.is_empty());
    assert_ne!(volta.root, root.youtube().unwrap().root);
}
