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

fn snapshot_bytes(store: &Storage) -> Vec<(String, Vec<u8>)> {
    let mut files = std::fs::read_dir(&store.root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn profile(stable: &str, legacy: &str) -> AccountIdentity {
    AccountIdentity {
        stable_web_api_id: stable.into(),
        legacy_web_api_id: legacy.into(),
        ..AccountIdentity::default()
    }
}

#[test]
fn stable_identity_wins_over_changed_or_colliding_legacy_aliases() {
    assert_eq!(
        profile("stable", "old").relation(&profile("stable", "new")),
        AccountRelation::Same
    );
    assert_eq!(
        profile("left", "same-alias").relation(&profile("right", "same-alias")),
        AccountRelation::Different
    );
    assert_eq!(
        profile("stable", "").relation(&profile("", "")),
        AccountRelation::Ambiguous
    );
    // Matching strings in different namespaces do not establish an identity.
    assert_eq!(
        profile("username", "").relation(&profile("", "username")),
        AccountRelation::Ambiguous
    );
}

#[test]
fn old_credentials_keep_role_specific_identity_and_unknown_schemas_are_preserved() {
    let tokens = credential_tokens(Ok(r#"{"access_token":"access","refresh_token":"refresh","expires_at":0,"account_id":"legacy"}"#.into())).unwrap().unwrap();
    let catalog = token_identity(&tokens, false).unwrap();
    let streaming = token_identity(&tokens, true).unwrap();
    assert_eq!(catalog.legacy_web_api_id, "legacy");
    assert!(catalog.stable_web_api_id.is_empty());
    assert!(catalog.streaming_username.is_empty());
    assert_eq!(streaming.streaming_username, "legacy");
    assert!(streaming.legacy_web_api_id.is_empty());
    let missing = credential_tokens(Ok(
        r#"{"access_token":"access","refresh_token":"refresh","expires_at":0}"#.into(),
    ))
    .unwrap()
    .unwrap();
    assert!(
        token_identity(&missing, false)
            .unwrap()
            .stable_web_api_id
            .is_empty()
    );
    let mut future = tokens.clone();
    future.identity = Some(AccountIdentity {
        version: 2,
        ..AccountIdentity::default()
    });
    let raw = serde_json::to_string(&future).unwrap();
    let error = credential_tokens(Ok(raw)).err().unwrap();
    assert!(error.to_string().contains("Unsupported"));
    assert!(!format!("{error:#}").contains("refresh\""));
    let new = Tokens {
        identity: Some(profile("stable", "legacy")),
        ..tokens
    };
    let roundtrip = credential_tokens(Ok(serde_json::to_string(&new).unwrap()))
        .unwrap()
        .unwrap();
    assert_eq!(roundtrip.identity, new.identity);
    let malformed = credential_tokens(Ok(r#"{"access_token":"access","refresh_token":"private-refresh","expires_at":"private-secret"}"#.into())).err().unwrap();
    assert!(!format!("{malformed:#}").contains("private"));
}

fn fixture_tokens() -> Tokens {
    Tokens {
        access_token: "old".into(),
        refresh_token: "old-refresh".into(),
        expires_at: 1,
        account_id: "old-alias".into(),
        identity: None,
    }
}

#[test]
fn failed_catalog_config_save_rolls_back_credentials_and_preserves_raw_state() {
    for previous_exists in [true, false] {
        let (_dir, store, _) = snapshot_store();
        let config = Config::default();
        store.save_config(&config).unwrap();
        let config_path = store.root.join("config.json");
        let original_permissions = std::fs::metadata(&config_path).unwrap().permissions();
        let mut readonly = original_permissions.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&config_path, readonly).unwrap();
        let before = snapshot_bytes(&store);
        let previous = fixture_tokens();
        let saved = std::cell::RefCell::new(
            previous_exists.then(|| serde_json::to_string(&previous).unwrap()),
        );
        let replacement = Tokens {
            access_token: "new".into(),
            identity: Some(profile("stable", "new-alias")),
            ..previous.clone()
        };
        let result = commit_catalog_login(
            &store,
            &Config {
                volume: 71,
                ..config
            },
            &replacement,
            previous_exists.then_some(&previous),
            |tokens| {
                *saved.borrow_mut() = Some(serde_json::to_string(tokens).unwrap());
                Ok(())
            },
            || {
                *saved.borrow_mut() = None;
                Ok(())
            },
        );
        let after = snapshot_bytes(&store);
        std::fs::set_permissions(&config_path, original_permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(after, before);
        assert_eq!(
            *saved.borrow(),
            previous_exists.then(|| serde_json::to_string(&previous).unwrap())
        );
    }
}

#[test]
fn failed_initial_credential_save_never_changes_config_or_other_files() {
    let (_dir, store, _) = snapshot_store();
    store.save_config(&Config::default()).unwrap();
    let before = snapshot_bytes(&store);
    let error = commit_catalog_login(
        &store,
        &Config {
            volume: 81,
            ..Config::default()
        },
        &fixture_tokens(),
        None,
        |_| bail!("credential write denied"),
        || panic!("no deletion after failed write"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("credential write denied"));
    assert_eq!(snapshot_bytes(&store), before);
}

#[test]
fn streaming_write_failure_restores_catalog_metadata_and_success_commits_both() {
    for fail_streaming in [true, false] {
        let previous = fixture_tokens();
        let mapped = Tokens {
            identity: Some(AccountIdentity {
                streaming_username: "canonical".into(),
                ..profile("stable", "alias")
            }),
            ..previous.clone()
        };
        let streaming = Tokens {
            account_id: "canonical".into(),
            access_token: "stream".into(),
            ..mapped.clone()
        };
        let catalog_saved = std::cell::RefCell::new(serde_json::to_string(&previous).unwrap());
        let stream_saved = std::cell::RefCell::new(String::from("old-streaming-credential"));
        let result = commit_streaming_login(&previous, &mapped, &streaming, |tokens, streaming| {
            if streaming && fail_streaming {
                bail!("stream credential write denied");
            }
            let target = if streaming {
                &stream_saved
            } else {
                &catalog_saved
            };
            *target.borrow_mut() = serde_json::to_string(tokens).unwrap();
            Ok(())
        });
        if fail_streaming {
            assert!(result.is_err());
            assert_eq!(
                *catalog_saved.borrow(),
                serde_json::to_string(&previous).unwrap()
            );
            assert_eq!(*stream_saved.borrow(), "old-streaming-credential");
        } else {
            assert!(result.is_ok());
            assert_eq!(
                *catalog_saved.borrow(),
                serde_json::to_string(&mapped).unwrap()
            );
            assert_eq!(
                *stream_saved.borrow(),
                serde_json::to_string(&streaming).unwrap()
            );
        }
    }
}

#[tokio::test]
async fn identity_profile_accepts_stable_only_and_legacy_only_but_rejects_invalid_fields() {
    for (body, expected) in [
        (
            serde_json::json!({"account_id":"stable", "id":"alias"}),
            Some(profile("stable", "alias")),
        ),
        (
            serde_json::json!({"account_id":"stable"}),
            Some(profile("stable", "")),
        ),
        (
            serde_json::json!({"id":"alias"}),
            Some(profile("", "alias")),
        ),
        (serde_json::json!({"account_id":17, "id":"alias"}), None),
        (
            serde_json::json!({"display_name":"private-person", "id":null}),
            None,
        ),
        (serde_json::json!({"account_id":"bad\nsecret"}), None),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;
        let result = profile_identity_at("private-token", &server.uri()).await;
        if let Some(expected) = expected {
            assert_eq!(result.unwrap(), expected);
        } else {
            let error = result.unwrap_err();
            assert!(ServiceFailure::is(&error, FailureKind::InvalidResponse));
            assert!(!format!("{error:#}").contains("private"));
            assert!(!format!("{error:#}").contains("secret"));
        }
    }
}

#[tokio::test]
async fn stable_reauth_preserves_raw_files_and_existing_verified_username_mapping() {
    let (_dir, store, _) = snapshot_store();
    std::fs::write(
        store.root.join("recipes.json"),
        b"recipe bytes are not rewritten",
    )
    .unwrap();
    std::fs::write(
        store.root.join("config.json"),
        b"config bytes are not read by identity comparison",
    )
    .unwrap();
    let before = snapshot_bytes(&store);
    let manager = TokenManager::mock("http://127.0.0.1:1/no-network".into(), true);
    manager.state.lock().await.identity = Some(AccountIdentity {
        streaming_username: "canonical".into(),
        ..profile("stable", "old-alias")
    });
    let mapped = prepare_catalog_identity(
        &store,
        Some(&manager),
        profile("stable", "new-alias"),
        "http://127.0.0.1:1/no-network",
    )
    .await
    .unwrap();
    assert_eq!(mapped.stable_web_api_id, "stable");
    assert_eq!(mapped.legacy_web_api_id, "new-alias");
    assert_eq!(mapped.streaming_username, "canonical");
    assert_eq!(snapshot_bytes(&store), before);
}

#[tokio::test]
async fn changed_legacy_alias_migrates_using_the_previous_authenticated_profile() {
    let (_dir, store, _) = snapshot_store();
    let before = snapshot_bytes(&store);
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wiremock::matchers::header("authorization", "Bearer old"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"account_id":"stable", "id":"old-alias"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let manager = TokenManager::mock(server.uri(), false);
    manager.state.lock().await.account_id = "old-alias".into();
    let mapped = prepare_catalog_identity(
        &store,
        Some(&manager),
        profile("stable", "new-alias"),
        &server.uri(),
    )
    .await
    .unwrap();
    assert_eq!(mapped, profile("stable", "new-alias"));
    assert_eq!(snapshot_bytes(&store), before);
    assert!(manager.state.lock().await.identity.is_none());
}

#[tokio::test]
async fn legacy_and_identity_free_tokens_can_migrate_without_new_token_metadata() {
    for old_alias in ["current-alias", ""] {
        let (_dir, store, _) = snapshot_store();
        let before = snapshot_bytes(&store);
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"account_id":"stable", "id":"current-alias"}),
                ),
            )
            .expect(if old_alias.is_empty() { 1 } else { 0 })
            .mount(&server)
            .await;
        let manager = TokenManager::mock(server.uri(), false);
        manager.state.lock().await.account_id = old_alias.into();
        let result = prepare_catalog_identity(
            &store,
            Some(&manager),
            profile("stable", "current-alias"),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(result, profile("stable", "current-alias"));
        assert_eq!(snapshot_bytes(&store), before);
    }
}

#[tokio::test]
async fn replacement_rejects_conflicts_and_unverifiable_prior_accounts_without_writing() {
    for body in [
        ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({"account_id":"different", "id":"old"})),
        ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"old"})),
        ResponseTemplate::new(401),
        ResponseTemplate::new(429)
            .set_body_json(serde_json::json!({"error":{"reason":"QUOTA_EXCEEDED"}})),
        ResponseTemplate::new(503),
    ] {
        let (_dir, store, _) = snapshot_store();
        let before = snapshot_bytes(&store);
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(body)
            .expect(1)
            .mount(&server)
            .await;
        let manager = TokenManager::mock(server.uri(), false);
        manager.state.lock().await.account_id = "old".into();
        assert!(
            prepare_catalog_identity(
                &store,
                Some(&manager),
                profile("stable", "new"),
                &server.uri()
            )
            .await
            .is_err()
        );
        assert_eq!(snapshot_bytes(&store), before);
    }
    let (_dir, store, _) = snapshot_store();
    let manager = TokenManager::mock("http://127.0.0.1:1/no-network".into(), true);
    manager.state.lock().await.identity = Some(profile("different", "same-alias"));
    let before = snapshot_bytes(&store);
    assert!(
        prepare_catalog_identity(
            &store,
            Some(&manager),
            profile("stable", "same-alias"),
            "http://127.0.0.1:1/no-network"
        )
        .await
        .is_err()
    );
    assert_eq!(snapshot_bytes(&store), before);
}

#[tokio::test]
async fn initial_auth_requires_no_orphaned_account_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = Storage {
        root: dir.path().to_owned(),
    };
    assert!(
        prepare_catalog_identity(&store, None, profile("stable", "alias"), "unused")
            .await
            .is_ok()
    );
    std::fs::write(
        store.root.join("stats.json"),
        b"damaged old stats must survive",
    )
    .unwrap();
    let before = snapshot_bytes(&store);
    assert!(
        prepare_catalog_identity(&store, None, profile("stable", "alias"), "unused")
            .await
            .is_err()
    );
    assert_eq!(snapshot_bytes(&store), before);
}

#[tokio::test]
async fn streaming_migration_binds_three_distinct_namespaces_and_rejects_another_account() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"account_id":"stable", "id":"public-alias"})),
        )
        .expect(3)
        .mount(&server)
        .await;
    let manager = TokenManager::mock(server.uri(), false);
    manager.state.lock().await.account_id = "old-public-alias".into();
    let mapped = prepare_streaming_identity(&manager, "canonical", &server.uri(), || async {
        Ok(profile("stable", "different-stream-alias"))
    })
    .await
    .unwrap();
    assert_eq!(
        mapped,
        AccountIdentity {
            streaming_username: "canonical".into(),
            ..profile("stable", "public-alias")
        }
    );
    assert!(
        prepare_streaming_identity(&manager, "another-account", &server.uri(), || async {
            Ok(profile("other-stable-account", "public-alias"))
        })
        .await
        .is_err()
    );
    // A fresh legacy handle matching the authenticated AP username preserves
    // the prior verifier's path without requesting the streaming client's /me.
    assert!(
        prepare_streaming_identity(&manager, "public-alias", &server.uri(), || async {
            panic!("current authenticated legacy handle needs no extra profile request")
        })
        .await
        .is_ok()
    );
    assert!(manager.state.lock().await.identity.is_none());
}

#[tokio::test]
async fn streaming_reauth_uses_verified_mapping_only_after_fresh_stable_account_check() {
    for (actual_stable, username, success) in [
        ("stable", "canonical", true),
        ("stable", "another-account", false),
        ("different", "canonical", false),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"account_id":actual_stable, "id":"new-alias"}),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;
        let manager = TokenManager::mock(server.uri(), false);
        manager.state.lock().await.identity = Some(AccountIdentity {
            streaming_username: "canonical".into(),
            ..profile("stable", "old-alias")
        });
        let result = prepare_streaming_identity(&manager, username, &server.uri(), || async {
            assert_eq!(username, "another-account");
            Ok(profile("different", "other-alias"))
        })
        .await;
        assert_eq!(result.is_ok(), success);
    }
}

#[tokio::test]
async fn changed_streaming_username_can_rebind_only_with_matching_authenticated_profiles() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wiremock::matchers::header("authorization", "Bearer old"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"account_id":"stable", "id":"public-alias"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer new-stream-token",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"account_id":"stable", "id":"stream-alias"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let manager = TokenManager::mock(server.uri(), false);
    manager.state.lock().await.identity = Some(AccountIdentity {
        streaming_username: "old-canonical".into(),
        ..profile("stable", "old-alias")
    });
    let mapped = prepare_streaming_identity(&manager, "new-canonical", &server.uri(), || async {
        profile_identity_at("new-stream-token", &server.uri()).await
    })
    .await
    .unwrap();
    assert_eq!(mapped.streaming_username, "new-canonical");
    assert_eq!(mapped.stable_web_api_id, "stable");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    assert_eq!(
        manager
            .state
            .lock()
            .await
            .identity
            .as_ref()
            .unwrap()
            .streaming_username,
        "old-canonical"
    );
}

#[tokio::test]
async fn ambiguous_stream_mapping_preserves_actual_profile_failure_and_all_files() {
    for (response, kind) in [
        (ResponseTemplate::new(403), FailureKind::AccessRestricted),
        (
            ResponseTemplate::new(429)
                .set_body_json(serde_json::json!({"error":{"reason":"QUOTA_EXCEEDED"}})),
            FailureKind::QuotaExceeded,
        ),
        (
            ResponseTemplate::new(429).insert_header("Retry-After", "120"),
            FailureKind::RateLimited,
        ),
        (ResponseTemplate::new(503), FailureKind::Server),
    ] {
        let (_dir, store, _) = snapshot_store();
        let before = snapshot_bytes(&store);
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wiremock::matchers::header("authorization", "Bearer old"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"account_id":"stable", "id":"public-alias"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(wiremock::matchers::header(
                "authorization",
                "Bearer private-stream-token",
            ))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let manager = TokenManager::mock(server.uri(), false);
        let error = prepare_streaming_identity(&manager, "canonical", &server.uri(), || async {
            profile_identity_at("private-stream-token", &server.uri()).await
        })
        .await
        .unwrap_err();
        assert!(ServiceFailure::is(&error, kind));
        assert!(!format!("{error:#}").contains("private-stream-token"));
        assert_eq!(snapshot_bytes(&store), before);
        assert!(manager.state.lock().await.identity.is_none());
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }
}

#[tokio::test]
async fn refresh_preserves_verified_identity_and_runtime_checks_authenticated_username() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"access_token":"new", "refresh_token":"new-refresh", "expires_in":3600}))).expect(1).mount(&server).await;
    let mut manager = TokenManager::mock(server.uri(), true);
    manager.streaming = true;
    let identity = AccountIdentity {
        streaming_username: "canonical".into(),
        ..profile("stable", "alias")
    };
    manager.state.lock().await.identity = Some(identity.clone());
    assert_eq!(manager.access().await.unwrap(), "new");
    assert_eq!(
        manager.state.lock().await.identity.as_ref(),
        Some(&identity)
    );
    manager
        .verify_streaming_username("canonical")
        .await
        .unwrap();
    assert!(manager.verify_streaming_username("other").await.is_err());
    assert!(manager.verify_streaming_username("").await.is_err());
    let catalog = Tokens {
        identity: Some(identity.clone()),
        ..manager.state.lock().await.clone()
    };
    let streaming = Tokens {
        identity: Some(identity),
        ..catalog.clone()
    };
    assert_eq!(
        saved_login_state(Some(&catalog), Some(&streaming)).unwrap(),
        (true, true)
    );
}

#[test]
fn same_account_relogin_preserves_queue_and_cache() {
    let (_dir, store, id) = snapshot_store();

    let identity = AccountIdentity {
        stable_web_api_id: "account".into(),
        ..AccountIdentity::default()
    };
    check_account_state(&store, Some(&identity), &identity).unwrap();
    assert_eq!(store.queue().unwrap().ids, vec![id.clone()]);
    assert!(store.cache().unwrap().contains_key(&id));
    assert!(store.stats().unwrap().tracks.contains_key(&id));
}

#[test]
fn changed_or_unknown_account_preserves_every_snapshot() {
    for previous in [
        Some(AccountIdentity {
            stable_web_api_id: "different".into(),
            ..AccountIdentity::default()
        }),
        None,
    ] {
        let (_dir, store, id) = snapshot_store();
        let before = snapshot_bytes(&store);
        let verified = AccountIdentity {
            stable_web_api_id: "account".into(),
            ..AccountIdentity::default()
        };
        assert!(check_account_state(&store, previous.as_ref(), &verified).is_err());
        assert_eq!(before, snapshot_bytes(&store));
        assert_eq!(store.queue().unwrap().ids, vec![id.clone()]);
        assert!(store.cache().unwrap().contains_key(&id));
        assert!(store.stats().unwrap().tracks.contains_key(&id));
    }
}

#[test]
fn account_matching_rejects_unknown_ids_for_streaming_reauth() {
    let catalog = AccountIdentity {
        stable_web_api_id: "stable".into(),
        ..AccountIdentity::default()
    };
    assert!(bind_streaming(catalog.clone(), "username", "username").is_ok());
    assert!(bind_streaming(catalog.clone(), "username", "other").is_err());
    assert!(bind_streaming(catalog.clone(), "", "username").is_err());
    assert!(bind_streaming(catalog, "username", "").is_err());
}

#[test]
fn saved_login_state_reauths_mismatched_or_unknown_streaming_accounts() {
    let token = |account: &str| Tokens {
        access_token: "access".into(),
        refresh_token: "refresh".into(),
        expires_at: 1,
        account_id: account.into(),
        identity: None,
    };
    let catalog = token("catalog");
    let matching = token("catalog");
    let different = token("different");
    let unknown = token("");

    assert_eq!(
        saved_login_state(Some(&catalog), Some(&matching)).unwrap(),
        (true, true)
    );
    assert_eq!(
        saved_login_state(Some(&catalog), Some(&different)).unwrap(),
        (true, false)
    );
    assert_eq!(
        saved_login_state(Some(&catalog), Some(&unknown)).unwrap(),
        (true, false)
    );
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
        identity: None,
    };
    assert!(
        credential_tokens(Ok(serde_json::to_string(&tokens).unwrap()))
            .unwrap()
            .is_some()
    );
    assert!(
        credential_tokens(Err(keyring::Error::NoEntry))
            .unwrap()
            .is_none()
    );
    assert!(credential_tokens(Ok("broken json".into())).is_err());
    let empty = Tokens {
        refresh_token: String::new(),
        ..tokens
    };
    assert!(
        credential_tokens(Ok(serde_json::to_string(&empty).unwrap()))
            .unwrap()
            .is_none()
    );
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

#[tokio::test]
async fn profile_quota_never_uses_the_short_rate_limit_retry() {
    for header in [None, Some("0"), Some("invalid")] {
        let server = MockServer::start().await;
        let mut template = ResponseTemplate::new(429)
            .set_body_json(serde_json::json!({"error":{"reason":"QUOTA_EXCEEDED"}}));
        if let Some(header) = header {
            template = template.insert_header("Retry-After", header);
        }
        Mock::given(method("GET"))
            .respond_with(template)
            .expect(1)
            .mount(&server)
            .await;
        let error = profile_id_at("secret-token", &server.uri())
            .await
            .unwrap_err();
        assert!(ServiceFailure::is(&error, FailureKind::QuotaExceeded));
        assert!(!format!("{error:#}").contains("secret-token"));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn refresh_quota_gate_is_shared_and_has_no_reauthentication_prompt() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_json(
            serde_json::json!({"error":{"reason":"QUOTA_EXCEEDED", "message":"refresh-secret"}}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let manager = TokenManager::mock(server.uri(), true);
    for client in [manager.clone(), manager] {
        let error = client.access().await.unwrap_err();
        assert!(ServiceFailure::is(&error, FailureKind::QuotaExceeded));
        let message = format!("{error:#}");
        assert!(!message.contains("tuitify auth"));
        assert!(!message.contains("refresh-secret"));
        assert!(!message.contains("60 seconds"));
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn token_server_and_invalid_response_keep_the_failure_kind() {
    for (template, kind) in [
        (ResponseTemplate::new(404), FailureKind::RequestRejected),
        (ResponseTemplate::new(503), FailureKind::Server),
        (
            ResponseTemplate::new(200).set_body_string("private-token-response"),
            FailureKind::InvalidResponse,
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(template)
            .expect(1)
            .mount(&server)
            .await;
        let error = TokenManager::mock(server.uri(), true)
            .access()
            .await
            .unwrap_err();
        assert!(ServiceFailure::is(&error, kind));
        assert!(!format!("{error:#}").contains("private-token-response"));
    }
}
