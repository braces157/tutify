use super::*;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn options() -> Options {
    Options {
        network: true,
        timeout: 1,
        ..Options::default()
    }
}

#[test]
fn inspection_preserves_corrupt_future_state_and_pending_journal() {
    let root = tempfile::tempdir().unwrap();
    let store = Storage {
        root: root.path().to_owned(),
    };
    let original = [
        ("config.json", r#"{"version":1,"volume":44}"#),
        ("queue.json", "secret track title: malformed JSON"),
        (
            "cache.json",
            r#"{"version":99,"private_path":"C:/private/person"}"#,
        ),
        ("restore-journal.json", "never recover or remove in doctor"),
    ];
    for (name, contents) in original {
        fs::write(store.root.join(name), contents).unwrap();
    }
    let mut report = Report::default();
    assert!(state_checks(&mut report, &store));
    assert!(!report.healthy());
    for (name, contents) in original {
        assert_eq!(fs::read_to_string(store.root.join(name)).unwrap(), contents);
    }
    assert!(!store.root.join("instance.lock").exists());
    let text = serde_json::to_string(&report).unwrap();
    assert!(!text.contains("secret track title"));
    assert!(!text.contains("private_path"));
    assert!(text.contains("99"));
    assert!(text.contains("restore_journal"));
    assert!(
        report
            .checks
            .iter()
            .filter(|check| check.level == Level::Failure)
            .all(|check| check.action.is_some())
    );
}

#[test]
fn inspection_does_not_create_missing_root_or_rewrite_valid_config() {
    let root = tempfile::tempdir().unwrap();
    let store = Storage {
        root: root.path().join("missing"),
    };
    let mut report = Report::default();
    assert!(state_checks(&mut report, &store));
    assert!(report.healthy());
    assert!(!store.root.exists());
    fs::create_dir(&store.root).unwrap();
    let bytes = b"{  \"version\": 1, \"volume\": 71, \"unknown_future_field\": \"keep exactly\" }";
    fs::write(store.root.join("config.json"), bytes).unwrap();
    state_checks(&mut Report::default(), &store);
    assert_eq!(fs::read(store.root.join("config.json")).unwrap(), bytes);
    assert_eq!(fs::read_dir(&store.root).unwrap().count(), 1);
}

#[test]
fn invalid_data_root_is_not_traversed() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file");
    fs::write(&file, "preserve me").unwrap();
    let mut report = Report::default();
    assert!(!state_checks(&mut report, &Storage { root: file.clone() }));
    assert!(!report.healthy());
    assert_eq!(fs::read_to_string(file).unwrap(), "preserve me");
    assert_eq!(report.checks.len(), 1);
}

#[test]
fn support_projection_drops_paths_payloads_devices_and_unrecognized_check_names() {
    let mut report = Report::default();
    let private = "private-fixture-secret C:/Users/Private Person http://127.0.0.1:8989/callback?code=secret Secret Song";
    report.add("data_directory", Level::Failure, private, Some(private));
    report.add("audio.device.1", Level::Pass, private, None);
    report.add(private, Level::Failure, private, Some(private));
    let checks = report.safe_checks();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].code, "data_directory");
    let text = serde_json::to_string(&checks).unwrap();
    for secret in [
        "private-fixture-secret",
        "Private Person",
        "http://",
        "Secret Song",
    ] {
        assert!(!text.contains(secret));
    }
}

#[test]
fn terminal_reports_redirects_size_and_errors_without_changing_console() {
    for (input, output, size, expected) in [
        (false, false, Ok((80, 24)), Level::Warning),
        (true, true, Ok((31, 10)), Level::Failure),
        (true, true, Ok((32, 9)), Level::Failure),
        (true, true, Ok((32, 10)), Level::Pass),
        (
            true,
            true,
            Err(std::io::Error::other("private console details")),
            Level::Failure,
        ),
    ] {
        let mut report = Report::default();
        terminal_check(&mut report, input, output, size);
        assert_eq!(report.checks[0].level, expected);
        if expected != Level::Pass {
            assert!(report.checks[0].action.is_some());
        }
    }
    assert!(!printable("\x1b]2;fake title\x07\ntext").contains(['\x1b', '\x07', '\n']));
}

#[test]
fn credential_reports_never_include_tokens_and_have_recovery_actions() {
    use auth::doctor::State;
    let mut report = Report::default();
    for state in [
        State::Missing,
        State::Present,
        State::Expired,
        State::Invalid,
        State::Unavailable,
    ] {
        credential_check(&mut report, "credentials.catalog", state);
    }
    assert!(
        report
            .checks
            .iter()
            .filter(|check| check.level == Level::Failure)
            .all(|check| check.action.is_some())
    );
    assert!(report.checks[2].detail.contains("does not refresh"));
}

#[tokio::test]
async fn offline_checks_make_no_requests_and_capabilities_start_unknown() {
    let server = MockServer::start().await;
    let mut report = Report::default();
    unknown_capabilities(&mut report, &Options::default());
    assert!(
        report
            .checks
            .iter()
            .all(|check| check.level == Level::Unknown)
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn successful_network_probes_are_get_only_scoped_and_payload_free() {
    let server = MockServer::start().await;
    for (endpoint, value) in [
        (
            "/me",
            serde_json::json!({"account_id":"private-user-id", "display_name":"private person"}),
        ),
        (
            "/me/tracks",
            serde_json::json!({"items":[{"track":{"name":"private song"}}]}),
        ),
        ("/me/playlists", serde_json::json!({"items":[]})),
        (
            "/artists/1111111111111111111111/top-tracks",
            serde_json::json!({"tracks":[]}),
        ),
        (
            "/playlists/2222222222222222222222/items",
            serde_json::json!({"items":[]}),
        ),
        ("/recommendations", serde_json::json!({"tracks":[]})),
    ] {
        Mock::given(method("GET"))
            .and(path(endpoint))
            .respond_with(ResponseTemplate::new(200).set_body_json(value))
            .expect(1)
            .mount(&server)
            .await;
    }
    let options = Options {
        artist: Some("1".repeat(22)),
        playlist: Some("2".repeat(22)),
        seed_track: Some("3".repeat(22)),
        ..options()
    };
    let mut report = Report::default();
    network_checks(&mut report, &Catalog::mock(&server.uri()), &options).await;
    assert_eq!(report.checks.len(), 6);
    assert!(report.checks.iter().all(|check| check.level == Level::Pass));
    let text = serde_json::to_string(&report).unwrap();
    for secret in [
        "private-user-id",
        "private person",
        "private song",
        "1111111111111111111111",
        "2222222222222222222222",
        "3333333333333333333333",
    ] {
        assert!(!text.contains(secret));
    }
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "GET")
    );
    assert!(
        requests
            .iter()
            .filter(
                |request| request.url.path() != "/me" && !request.url.path().contains("top-tracks")
            )
            .all(|request| request
                .url
                .query_pairs()
                .any(|(key, value)| key == "limit" && value == "1"))
    );
    server.verify().await;
}

#[tokio::test]
async fn quota_failure_stops_requests_preserves_classification_and_hides_payload() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/me")).respond_with(ResponseTemplate::new(429).set_body_json(serde_json::json!({"error":{"reason":"QUOTA_EXCEEDED", "message":"secret token and private history"}}))).expect(1).mount(&server).await;
    let mut report = Report::default();
    network_checks(&mut report, &Catalog::mock(&server.uri()), &options()).await;
    assert_eq!(report.checks[0].level, Level::Failure);
    assert!(report.checks[0].detail.contains("QUOTA_EXCEEDED"));
    assert!(
        report.checks[0]
            .detail
            .contains("Logging in again will not replenish")
    );
    assert!(
        report.checks[1..]
            .iter()
            .all(|check| check.level == Level::Unknown)
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("secret token")
    );
}

#[tokio::test]
async fn scoped_artist_denial_does_not_prevent_unrelated_playlist_probe() {
    let server = MockServer::start().await;
    for endpoint in [
        "/me/tracks",
        "/me/playlists",
        "/playlists/2222222222222222222222/items",
    ] {
        Mock::given(method("GET"))
            .and(path(endpoint))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"items":[]})))
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"user"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/artists/1111111111111111111111/top-tracks"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    let mut report = Report::default();
    network_checks(
        &mut report,
        &Catalog::mock(&server.uri()),
        &Options {
            artist: Some("1".repeat(22)),
            playlist: Some("2".repeat(22)),
            ..options()
        },
    )
    .await;
    assert_eq!(report.checks[3].level, Level::Failure);
    assert_eq!(report.checks[4].level, Level::Pass);
    assert!(
        report.checks[3]
            .action
            .as_ref()
            .unwrap()
            .contains("account/resource only")
    );
    server.verify().await;
}

#[tokio::test]
async fn malformed_success_and_timeout_stop_further_requests() {
    for response in [
        ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({"private_wrong_shape":"secret"})),
        ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({"id":"user"}))
            .set_delay(Duration::from_secs(2)),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/me"))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let mut report = Report::default();
        network_checks(&mut report, &Catalog::mock(&server.uri()), &options()).await;
        assert_eq!(report.checks[0].level, Level::Failure);
        assert!(
            report.checks[1..]
                .iter()
                .all(|check| check.level == Level::Unknown)
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("private_wrong_shape")
        );
        server.verify().await;
    }
}
