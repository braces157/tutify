use super::*;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{path, query_param},
};

const ARTIST: &str = "0000000000000000000001";
const PLAYLIST: &str = "0000000000000000000011";
const OTHER: &str = "0000000000000000000012";

#[tokio::test]
async fn token_refresh_quota_supersedes_remembered_access_restrictions() {
    let server = MockServer::start().await;
    Mock::given(path(format!("/playlists/{PLAYLIST}/items")))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path(format!("/tracks/{ARTIST}")))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/token"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(serde_json::json!({"error":{"reason":"QUOTA_EXCEEDED"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let catalog = Catalog::mock(&server.uri());
    assert!(
        catalog
            .page(&Browse::Playlist(PLAYLIST.into()), 0)
            .await
            .is_err()
    );
    let error = catalog.track(ARTIST).await.unwrap_err();
    assert!(ServiceFailure::is(&error, FailureKind::QuotaExceeded));
    for client in [catalog.clone(), catalog.clone()] {
        let error = client
            .page(&Browse::Playlist(PLAYLIST.into()), 0)
            .await
            .unwrap_err();
        assert!(ServiceFailure::is(&error, FailureKind::QuotaExceeded));
        client.refresh_capabilities();
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn metadata_only_access_is_typed_but_malformed_playlist_data_is_not_memoized_as_denied() {
    let server = MockServer::start().await;
    Mock::given(path(format!("/playlists/{PLAYLIST}/items")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":PLAYLIST, "type":"playlist", "name":"Restricted", "owner":{"id":"owner"}})))
        .expect(1).mount(&server).await;
    Mock::given(path(format!("/playlists/{OTHER}/items")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"name":"Incomplete payload"})),
        )
        .expect(2)
        .mount(&server)
        .await;
    let catalog = Catalog::mock(&server.uri());
    for _ in 0..2 {
        let error = catalog
            .page(&Browse::Playlist(PLAYLIST.into()), 0)
            .await
            .unwrap_err();
        assert!(ServiceFailure::is(&error, FailureKind::AccessRestricted));
        assert_eq!(
            error.downcast_ref::<ServiceFailure>().unwrap().status,
            Some(200)
        );
        assert!(!format!("{error:#}").contains("HTTP 403"));
        let error = catalog
            .page(&Browse::Playlist(OTHER.into()), 0)
            .await
            .unwrap_err();
        assert!(ServiceFailure::is(&error, FailureKind::InvalidResponse));
        let key = capabilities::Key::for_request("GET", &format!("/playlists/{OTHER}/items"), &[])
            .unwrap();
        assert!(catalog.capabilities.slot(key).lock().await.is_none());
    }
}

#[tokio::test]
async fn denied_artist_browsing_skips_primary_requests_across_clones_and_pages() {
    let server = MockServer::start().await;
    Mock::given(path(format!("/artists/{ARTIST}/top-tracks")))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path(format!("/artists/{ARTIST}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"name":"Artist"})),
        )
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path("/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"tracks":{"items":[],"next":"next"}})),
        )
        .expect(2)
        .mount(&server)
        .await;
    let catalog = Catalog::mock(&server.uri());
    for (client, offset) in [(catalog.clone(), 0), (catalog.clone(), 10)] {
        let page = client
            .page(&Browse::Artist(ARTIST.into()), offset)
            .await
            .unwrap();
        assert_eq!(page.offset, offset);
        assert_eq!(page.next, Some(offset + 10));
    }
    let key = capabilities::Key::for_request("GET", &format!("/artists/{ARTIST}/top-tracks"), &[])
        .unwrap();
    let observation = *catalog.capabilities.slot(key).lock().await;
    assert!(matches!(
        observation.unwrap().outcome().unwrap(),
        capabilities::Outcome::Denied(_)
    ));
}

#[tokio::test]
async fn playlist_denial_is_resource_specific_and_successful_pages_still_load() {
    let server = MockServer::start().await;
    Mock::given(path(format!("/playlists/{PLAYLIST}/items")))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path(format!("/playlists/{OTHER}/items")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"items":[],"next":"next"})),
        )
        .expect(2)
        .mount(&server)
        .await;
    let catalog = Catalog::mock(&server.uri());
    for offset in [0, 50] {
        let error = catalog
            .page(&Browse::Playlist(PLAYLIST.into()), offset)
            .await
            .unwrap_err();
        assert!(ServiceFailure::is(&error, FailureKind::AccessRestricted));
        let page = catalog
            .page(&Browse::Playlist(OTHER.into()), offset)
            .await
            .unwrap();
        assert_eq!(page.next, Some(offset + 50));
    }
    let key =
        capabilities::Key::for_request("GET", &format!("/playlists/{OTHER}/items"), &[]).unwrap();
    assert_eq!(
        catalog
            .capabilities
            .slot(key)
            .lock()
            .await
            .unwrap()
            .outcome()
            .unwrap(),
        capabilities::Outcome::Supported
    );
}

#[tokio::test]
async fn recommendation_denials_are_scoped_to_seeds_without_hiding_other_failures() {
    let server = MockServer::start().await;
    Mock::given(path("/recommendations"))
        .and(query_param("seed_tracks", ARTIST))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/recommendations"))
        .and(query_param("seed_tracks", OTHER))
        .respond_with(ResponseTemplate::new(503))
        .expect(2)
        .mount(&server)
        .await;
    let catalog = Catalog::mock(&server.uri());
    for _ in 0..2 {
        let error = catalog
            .get("/recommendations", &[("seed_tracks", ARTIST.into())])
            .await
            .unwrap_err();
        assert!(ServiceFailure::is(&error, FailureKind::AccessRestricted));
        let error = catalog
            .get("/recommendations", &[("seed_tracks", OTHER.into())])
            .await
            .unwrap_err();
        assert!(ServiceFailure::is(&error, FailureKind::Server));
    }
}

#[tokio::test]
async fn explicit_refresh_rechecks_denials_but_keeps_the_shared_quota_gate() {
    let server = MockServer::start().await;
    let denied_path = format!("/playlists/{PLAYLIST}/items");
    Mock::given(path(&denied_path))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    let catalog = Catalog::mock(&server.uri());
    assert!(catalog.get(&denied_path, &[]).await.is_err());
    server.reset().await;
    Mock::given(path(&denied_path))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"items":[]})))
        .expect(1)
        .mount(&server)
        .await;
    assert!(catalog.get(&denied_path, &[]).await.is_err());
    catalog.refresh_capabilities();
    assert!(catalog.get(&denied_path, &[]).await.is_ok());
    Mock::given(path("/me/tracks"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(serde_json::json!({"error":{"reason":"QUOTA_EXCEEDED"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let error = catalog.get("/me/tracks", &[]).await.unwrap_err();
    assert!(ServiceFailure::is(&error, FailureKind::QuotaExceeded));
    catalog.refresh_capabilities();
    let error = catalog.get(&denied_path, &[]).await.unwrap_err();
    assert!(ServiceFailure::is(&error, FailureKind::QuotaExceeded));
}

#[tokio::test]
async fn concurrent_denied_requests_share_a_probe_without_blocking_another_resource() {
    let server = MockServer::start().await;
    let denied_path = format!("/playlists/{PLAYLIST}/items");
    Mock::given(path(&denied_path))
        .respond_with(ResponseTemplate::new(403).set_delay(std::time::Duration::from_secs(2)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/me/tracks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"items":[]})))
        .expect(1)
        .mount(&server)
        .await;
    let catalog = Catalog::mock(&server.uri());
    let denied = catalog.get(&denied_path, &[]);
    let duplicate = catalog.get(&denied_path, &[]);
    let ordinary = async {
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            catalog.get("/me/tracks", &[]),
        )
        .await
        .unwrap()
        .unwrap()
    };
    let (first, second, _) = tokio::join!(denied, duplicate, ordinary);
    assert!(ServiceFailure::is(
        &first.unwrap_err(),
        FailureKind::AccessRestricted
    ));
    assert!(ServiceFailure::is(
        &second.unwrap_err(),
        FailureKind::AccessRestricted
    ));
}

#[tokio::test]
async fn new_client_and_account_never_inherit_a_denial() {
    let server = MockServer::start().await;
    let denied_path = format!("/playlists/{PLAYLIST}/items");
    Mock::given(path(&denied_path))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    let mut first = Catalog::new(TokenManager::mock_for_account(
        format!("{}/token", server.uri()),
        "test-client",
        "same-account",
    ))
    .unwrap();
    first.base = server.uri();
    assert!(first.get(&denied_path, &[]).await.is_err());
    server.reset().await;
    Mock::given(path(&denied_path))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"items":[]})))
        .expect(2)
        .mount(&server)
        .await;
    for (client, account) in [
        ("replacement-client", "same-account"),
        ("test-client", "replacement-account"),
    ] {
        let tokens =
            TokenManager::mock_for_account(format!("{}/token", server.uri()), client, account);
        let mut replacement = Catalog::new(tokens).unwrap();
        replacement.base = server.uri();
        assert!(replacement.get(&denied_path, &[]).await.is_ok());
    }
    assert!(first.get(&denied_path, &[]).await.is_err());
}

#[tokio::test]
async fn systemic_errors_do_not_establish_cached_denials() {
    for status in [401, 503] {
        let server = MockServer::start().await;
        let denied_path = format!("/playlists/{PLAYLIST}/items");
        Mock::given(path(&denied_path))
            .respond_with(ResponseTemplate::new(status))
            .expect(2)
            .mount(&server)
            .await;
        if status == 401 {
            Mock::given(path("/token"))
                .respond_with(ResponseTemplate::new(400))
                .expect(2)
                .mount(&server)
                .await;
        }
        let catalog = Catalog::mock(&server.uri());
        for _ in 0..2 {
            assert!(catalog.get(&denied_path, &[]).await.is_err());
        }
        let key = capabilities::Key::for_request("GET", &denied_path, &[]).unwrap();
        assert!(catalog.capabilities.slot(key).lock().await.is_none());
    }
}
