use super::*;
use crate::{cache::MetadataCache, model::Track, queue::Queue, storage::Storage};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_string_contains, method, path},
};

fn snapshot_store() -> (tempfile::TempDir, Storage, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = Storage {
        root: dir.path().to_owned(),
    };
    let id = "0".repeat(22);
    let mut queue = Queue::default();
    queue.replace(vec![id.clone()], 0, false);
    store.save_queue(&queue).unwrap();
    let mut cache = MetadataCache::default();
    cache.insert(id.clone(), Track::unknown(&id));
    store.save_cache(&cache).unwrap();
    let mut stats = crate::stats::SongStats::default();
    stats.add_listened_ms(&id, 1000, "Song", "Artist");
    stats.add_play(&id, "Song", "Artist");
    store.save_stats(&stats).unwrap();
    (dir, store, id)
}

#[test]
fn same_account_relogin_preserves_queue_and_cache() {
    let (_dir, store, id) = snapshot_store();

    assert!(update_account_state(&store, Some("account"), "account").unwrap());
    assert_eq!(store.queue().unwrap().ids, vec![id.clone()]);
    assert!(store.cache().unwrap().contains_key(&id));
    assert!(store.stats().unwrap().tracks.contains_key(&id));
}

#[test]
fn changed_or_unknown_account_clears_queue_and_cache() {
    for previous in [Some("different"), None] {
        let (_dir, store, id) = snapshot_store();

        assert!(!update_account_state(&store, previous, "account").unwrap());
        assert!(store.queue().unwrap().ids.is_empty());
        assert!(!store.cache().unwrap().contains_key(&id));
        assert!(!store.stats().unwrap().tracks.contains_key(&id));
        assert!(!store.root.join("queue.json").exists());
        assert!(!store.root.join("cache.json").exists());
        assert!(!store.root.join("stats.json").exists());
    }
}

#[test]
fn account_matching_rejects_unknown_ids_for_streaming_reauth() {
    assert!(accounts_match(Some("account"), "account"));
    assert!(!accounts_match(Some("account"), "other"));
    assert!(!accounts_match(None, "account"));
    assert!(!accounts_match(Some(""), "account"));
    assert!(!accounts_match(Some("account"), ""));
}

#[test]
fn setup_only_opens_missing_logins() {
    use LoginStep::*;
    assert_eq!(
        setup_steps(false, false, false, false, false),
        vec![Catalog, Streaming]
    );
    assert_eq!(
        setup_steps(true, false, false, false, false),
        vec![Streaming]
    );
    assert!(setup_steps(true, true, false, false, false).is_empty());
    assert_eq!(
        setup_steps(false, true, false, false, false),
        vec![Catalog, Streaming]
    );
}
#[test]
fn setup_client_change_and_force_invalidate_the_right_credentials() {
    use LoginStep::*;
    assert_eq!(
        setup_steps(true, true, true, false, false),
        vec![Catalog, Streaming]
    );
    assert_eq!(
        setup_steps(true, true, false, true, false),
        vec![Catalog, Streaming]
    );
    assert_eq!(setup_steps(true, true, false, true, true), vec![Streaming]);
    assert_eq!(
        setup_steps(false, true, false, false, true),
        vec![Catalog, Streaming]
    );
    assert!(setup_steps(true, true, false, false, true).is_empty());
}
#[test]
fn saved_refresh_token_skips_browser_even_when_access_token_expired() {
    let tokens = Tokens {
        access_token: "expired".into(),
        refresh_token: "reusable".into(),
        expires_at: 0,
        account_id: "account".into(),
    };
    assert!(credential_saved(Ok(serde_json::to_string(&tokens).unwrap())).unwrap());
    assert!(!credential_saved(Err(keyring::Error::NoEntry)).unwrap());
    assert!(!credential_saved(Ok("broken json".into())).unwrap());
    let empty = Tokens {
        refresh_token: String::new(),
        ..tokens
    };
    assert!(!credential_saved(Ok(serde_json::to_string(&empty).unwrap())).unwrap());
}
#[test]
fn callback_reports_final_success_or_failure() {
    let (status, body) = callback_message(&Ok(()), false);
    assert_eq!(status, "200 OK");
    assert!(body.contains("automatically open the remaining"));
    let (_, body) = callback_message(&Ok(()), true);
    assert!(body.contains("return to the terminal"));
    let (status, body) =
        callback_message(&Err(anyhow::anyhow!("HTTP 429: wait 120 seconds")), false);
    assert_ne!(status, "200 OK");
    assert!(body.contains("setup did not finish"));
    assert!(body.contains("wait 120 seconds"));
    assert!(!body.contains("saved successfully"));
}

#[tokio::test]
async fn verification_long_rate_limit_does_not_retry_or_blame_premium() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "120"))
        .expect(1)
        .mount(&server)
        .await;
    let message = profile_id_at("test-token", &server.uri())
        .await
        .unwrap_err()
        .to_string();
    assert!(message.contains("120 seconds"));
    assert!(message.contains("Premium is not the issue"));
}

#[tokio::test]
async fn verification_short_rate_limit_retries_once_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"account"})))
        .with_priority(2)
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        profile_id_at("test-token", &server.uri()).await.unwrap(),
        "account"
    );
}

#[tokio::test]
async fn verification_repeated_rate_limit_is_bounded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .expect(2)
        .mount(&server)
        .await;
    assert!(profile_id_at("test-token", &server.uri()).await.is_err());
}

#[tokio::test]
async fn verification_denied_access_does_not_retry() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    assert!(
        profile_id_at("test-token", &server.uri())
            .await
            .unwrap_err()
            .to_string()
            .contains("allowed in your Developer app")
    );
}
#[test]
fn oauth_validation() {
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    assert_eq!(
        validate_callback("/callback?state=abc&code=xyz", "abc").unwrap(),
        "xyz"
    );
    assert_eq!(
        validate_callback_path("/login?state=abc&code=xyz", "abc", "/login").unwrap(),
        "xyz"
    );
    for target in [
        "/callback?code=x",
        "/callback?state=wrong&code=x",
        "/callback?state=abc&state=abc&code=x",
        "/callback?state=abc&error=access_denied",
        "/other?state=abc&code=x",
        "/callback?state=abc&code=x&code=y",
    ] {
        assert!(validate_callback(target, "abc").is_err());
    }
}
#[test]
fn oauth_settings_match_spotatui_redirects_and_scopes() {
    let (_, redirect, path, scopes) = oauth_settings(SHARED_CLIENT_ID, false);
    assert_eq!(redirect, STREAMING_REDIRECT);
    assert_eq!(path, "/login");
    assert!(scopes.contains("playlist-read-private"));

    let (_, redirect, path, scopes) = oauth_settings("0123456789abcdef0123456789abcdef", false);
    assert_eq!(redirect, REDIRECT);
    assert_eq!(path, "/callback");
    assert_eq!(scopes, SCOPES);

    let (client, redirect, path, scopes) = oauth_settings("ignored", true);
    assert_eq!(client, STREAMING_CLIENT_ID);
    assert_eq!(redirect, STREAMING_REDIRECT);
    assert_eq!(path, "/login");
    assert!(scopes.contains("streaming"));
}
#[tokio::test]
async fn refresh_is_serialized_and_preserves_refresh_token() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("refresh_token=refresh"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"access_token":"new", "expires_in":3600})),
        )
        .expect(1)
        .mount(&s)
        .await;
    let manager = TokenManager::mock(format!("{}/token", s.uri()), true);
    let (a, b) = tokio::join!(manager.access(), manager.access());
    assert_eq!(a.unwrap(), "new");
    assert_eq!(b.unwrap(), "new");
    assert_eq!(manager.state.lock().await.refresh_token, "refresh");
}
#[tokio::test]
async fn revoked_refresh_is_actionable() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&s)
        .await;
    assert!(
        TokenManager::mock(s.uri(), true)
            .access()
            .await
            .unwrap_err()
            .to_string()
            .contains("tuitify auth")
    );
}
#[tokio::test]
async fn token_quota_respects_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "120"))
        .expect(1)
        .mount(&server)
        .await;
    let manager = TokenManager::mock(server.uri(), true);
    assert!(manager.access().await.is_err());
    assert!(
        manager
            .access()
            .await
            .unwrap_err()
            .to_string()
            .contains("rate limit")
    );
}
#[test]
fn retry_header_seconds_and_http_date() {
    assert_eq!(retry_delay(Some("120")), Duration::from_secs(120));
    let date = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(90));
    assert!((88..=90).contains(&retry_delay(Some(&date)).as_secs()));
    assert_eq!(retry_delay(Some("invalid")), Duration::from_secs(60));
}
