use super::*;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn quota() -> ResponseTemplate {
    ResponseTemplate::new(429)
        .set_body_json(serde_json::json!({"error":{"reason":"QUOTA_EXCEEDED"}}))
}

#[tokio::test]
async fn quota_gate_is_shared_and_preserves_cause_without_more_requests() {
    let server = MockServer::start().await;
    Mock::given(path("/me/tracks"))
        .respond_with(quota())
        .expect(1)
        .mount(&server)
        .await;
    let catalog = Catalog::mock(&server.uri());
    for client in [catalog.clone(), catalog.clone()] {
        let error = client.page(&Browse::Liked, 0).await.unwrap_err();
        assert!(ServiceFailure::is(&error, FailureKind::QuotaExceeded));
        assert!(error.to_string().contains("Recovery time is unknown"));
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn second_401_refreshes_once_and_returns_authentication_without_fallback() {
    let server = MockServer::start().await;
    Mock::given(path("/artists/0000000000000000000001/top-tracks"))
        .respond_with(ResponseTemplate::new(401))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path("/token"))
        .and(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"access_token":"new", "expires_in":3600})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let error = Catalog::mock(&server.uri())
        .page(&Browse::Artist("0000000000000000000001".into()), 0)
        .await
        .unwrap_err();
    assert!(ServiceFailure::is(
        &error,
        FailureKind::AuthenticationRequired
    ));
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn artist_fallback_retains_its_actual_failure() {
    for stage in ["metadata", "search"] {
        for status in [401, 429, 503] {
            let server = MockServer::start().await;
            Mock::given(path("/artists/0000000000000000000001/top-tracks"))
                .respond_with(ResponseTemplate::new(403))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(path("/artists/0000000000000000000001"))
                .respond_with(if stage == "metadata" {
                    ResponseTemplate::new(status)
                } else {
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"name":"Artist"}))
                })
                .expect(if stage == "metadata" && status == 401 {
                    2
                } else {
                    1
                })
                .mount(&server)
                .await;
            Mock::given(path("/search"))
                .respond_with(ResponseTemplate::new(status))
                .expect(if stage == "metadata" {
                    0
                } else if status == 401 {
                    2
                } else {
                    1
                })
                .mount(&server)
                .await;
            if status == 401 {
                Mock::given(path("/token"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(
                        serde_json::json!({"access_token":"new", "expires_in":3600}),
                    ))
                    .expect(1)
                    .mount(&server)
                    .await;
            }
            let error = Catalog::mock(&server.uri())
                .page(&Browse::Artist("0000000000000000000001".into()), 0)
                .await
                .unwrap_err();
            let kind = match status {
                401 => FailureKind::AuthenticationRequired,
                429 => FailureKind::RateLimited,
                _ => FailureKind::Server,
            };
            assert!(ServiceFailure::is(&error, kind), "{stage}: {error:#}");
        }
    }
}

#[tokio::test]
async fn recommendations_never_hide_authentication_quota_or_outage() {
    for status in [401, 404, 429, 503] {
        let server = MockServer::start().await;
        Mock::given(path("/recommendations"))
            .respond_with(if status == 429 {
                quota()
            } else {
                ResponseTemplate::new(status)
            })
            .expect(if status == 401 { 2 } else { 1 })
            .mount(&server)
            .await;
        if status == 401 {
            Mock::given(path("/token"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(
                        serde_json::json!({"access_token":"new", "expires_in":3600}),
                    ),
                )
                .expect(1)
                .mount(&server)
                .await;
        }
        let seed = crate::demo::all_tracks().remove(0);
        let error = Catalog::mock(&server.uri())
            .recommendations(&seed)
            .await
            .unwrap_err();
        let kind = match status {
            401 => FailureKind::AuthenticationRequired,
            404 => FailureKind::MissingItem,
            429 => FailureKind::QuotaExceeded,
            _ => FailureKind::Server,
        };
        assert!(ServiceFailure::is(&error, kind));
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            if status == 401 { 3 } else { 1 }
        );
    }
}

#[tokio::test]
async fn recommendation_fallback_preserves_auth_rate_quota_and_server_causes() {
    for (status, quota_body, expected) in [
        (401, false, FailureKind::AuthenticationRequired),
        (429, false, FailureKind::RateLimited),
        (429, true, FailureKind::QuotaExceeded),
        (503, false, FailureKind::Server),
    ] {
        let server = MockServer::start().await;
        Mock::given(path("/recommendations"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/search"))
            .respond_with(if quota_body {
                quota()
            } else {
                ResponseTemplate::new(status)
            })
            .expect(if status == 401 { 2 } else { 1 })
            .mount(&server)
            .await;
        if status == 401 {
            Mock::given(path("/token"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(
                        serde_json::json!({"access_token":"new", "expires_in":3600}),
                    ),
                )
                .expect(1)
                .mount(&server)
                .await;
        }
        let seed = crate::demo::all_tracks().remove(0);
        let error = Catalog::mock(&server.uri())
            .recommendations(&seed)
            .await
            .unwrap_err();
        assert!(ServiceFailure::is(&error, expected), "{error:#}");
        assert!(!ServiceFailure::is(&error, FailureKind::AccessRestricted));
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            if status == 401 { 4 } else { 2 }
        );
    }
}

#[tokio::test]
async fn valid_empty_and_malformed_recommendations_never_start_an_alternate_lookup() {
    for malformed in [false, true] {
        let server = MockServer::start().await;
        Mock::given(path("/recommendations"))
            .respond_with(ResponseTemplate::new(200).set_body_json(if malformed {
                serde_json::json!({})
            } else {
                serde_json::json!({"tracks":[]})
            }))
            .expect(1)
            .mount(&server)
            .await;
        let seed = crate::demo::all_tracks().remove(0);
        let result = Catalog::mock(&server.uri()).recommendations(&seed).await;
        if malformed {
            assert!(ServiceFailure::is(
                &result.unwrap_err(),
                FailureKind::InvalidResponse
            ));
        } else {
            let result = result.unwrap();
            assert!(result.tracks.is_empty());
            assert_eq!(result.source, RecommendationSource::Spotify);
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn login_denial_is_never_cached_as_an_endpoint_capability_or_hidden_by_fallback() {
    for browse in [
        Browse::Artist("0000000000000000000001".into()),
        Browse::Search("seed".into()),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let mut catalog = Catalog::mock(&server.uri());
        catalog.tokens = TokenManager::mock(format!("{}/token", server.uri()), true);
        let error = if matches!(browse, Browse::Artist(_)) {
            catalog.page(&browse, 0).await.unwrap_err()
        } else {
            let seed = crate::demo::all_tracks().remove(0);
            catalog.recommendations(&seed).await.unwrap_err()
        };
        assert!(ServiceFailure::is(&error, FailureKind::AccessRestricted));
        assert!(!error.is::<capabilities::Denied>());
        let key = if let Browse::Artist(id) = &browse {
            capabilities::Key::for_request("GET", &format!("/artists/{id}/top-tracks"), &[])
                .unwrap()
        } else {
            let seed = crate::demo::all_tracks().remove(0);
            capabilities::Key::for_request("GET", "/recommendations", &[("seed_tracks", seed.id)])
                .unwrap()
        };
        assert!(catalog.capabilities.slot(key).lock().await.is_none());
    }
}

#[tokio::test]
async fn fallback_transport_failure_does_not_return_the_prior_endpoint_denial() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for artist in [true, false] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let responder = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 2048];
            let mut headers = Vec::new();
            while !headers.windows(4).any(|part| part == b"\r\n\r\n") {
                let read = socket.read(&mut buffer).await.unwrap();
                assert!(read > 0);
                headers.extend_from_slice(&buffer[..read]);
                assert!(headers.len() < 8192);
            }
            drop(listener);
            socket
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            socket.shutdown().await.unwrap();
        });
        let catalog = Catalog::mock(&base);
        let lookup = async {
            if artist {
                catalog
                    .page(&Browse::Artist("0000000000000000000001".into()), 0)
                    .await
                    .unwrap_err()
            } else {
                let seed = crate::demo::all_tracks().remove(0);
                catalog.recommendations(&seed).await.unwrap_err()
            }
        };
        let error = tokio::time::timeout(std::time::Duration::from_secs(10), lookup)
            .await
            .unwrap();
        responder.await.unwrap();
        assert!(
            ServiceFailure::is(&error, FailureKind::Transport),
            "{error:#}"
        );
        assert!(!ServiceFailure::is(&error, FailureKind::AccessRestricted));
        assert!(!format!("{error:#}").contains(&base));
        let key = if artist {
            capabilities::Key::for_request("GET", "/artists/0000000000000000000001/top-tracks", &[])
                .unwrap()
        } else {
            let seed = crate::demo::all_tracks().remove(0);
            capabilities::Key::for_request("GET", "/recommendations", &[("seed_tracks", seed.id)])
                .unwrap()
        };
        assert!(matches!(
            catalog
                .capabilities
                .slot(key)
                .lock()
                .await
                .unwrap()
                .outcome(),
            Some(capabilities::Outcome::Denied(_))
        ));
    }
}

#[tokio::test]
async fn transport_and_invalid_json_are_typed_and_redacted() {
    let server = MockServer::start().await;
    Mock::given(path("/me/tracks"))
        .respond_with(ResponseTemplate::new(200).set_body_string("invalid-json-bearer-secret"))
        .mount(&server)
        .await;
    let error = Catalog::mock(&server.uri())
        .page(&Browse::Liked, 0)
        .await
        .unwrap_err();
    assert!(ServiceFailure::is(&error, FailureKind::InvalidResponse));
    assert!(!format!("{error:#}").contains("bearer-secret"));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!(
        "http://{}/callback?code=private",
        listener.local_addr().unwrap()
    );
    drop(listener);
    let error = Catalog::mock(&base)
        .page(&Browse::Liked, 0)
        .await
        .unwrap_err();
    assert!(ServiceFailure::is(&error, FailureKind::Transport));
    assert!(!format!("{error:#}").contains("private"));
    assert!(!format!("{error:?}").contains("callback"));
}
