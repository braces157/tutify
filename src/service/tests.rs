use super::*;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

async fn failure(template: ResponseTemplate) -> ServiceFailure {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(template)
        .expect(1)
        .mount(&server)
        .await;
    let response = crate::auth::http_client()
        .unwrap()
        .get(server.uri())
        .send()
        .await
        .unwrap();
    ServiceFailure::from_response(response, Provider::Spotify).await
}

#[tokio::test]
async fn response_classes_and_context_are_semantic() {
    for (status, kind) in [
        (401, FailureKind::AuthenticationRequired),
        (403, FailureKind::AccessRestricted),
        (404, FailureKind::MissingItem),
        (429, FailureKind::RateLimited),
        (503, FailureKind::Server),
        (422, FailureKind::RequestRejected),
    ] {
        let failure = failure(ResponseTemplate::new(status)).await;
        assert_eq!(failure.kind, kind);
        assert_eq!(failure.status, Some(status));
        assert_eq!(failure.provider, Provider::Spotify);
        let error = anyhow::Error::new(failure).context("Browse failed");
        assert!(ServiceFailure::is(&error, kind));
    }
}

#[tokio::test]
async fn quota_has_no_invented_reset_and_never_retains_secrets() {
    let secret = "bearer-secret refresh-secret http://localhost/callback?code=secret";
    let template = ResponseTemplate::new(429)
        .set_body_json(serde_json::json!({"error":{"reason":"QUOTA_EXCEEDED", "message":secret}}));
    let failure = failure(template).await;
    assert_eq!(failure.kind, FailureKind::QuotaExceeded);
    assert_eq!(failure.retry_at, None);
    assert!(failure.active());
    let displayed = format!("{failure} {failure:?}");
    assert!(displayed.contains("Recovery time is unknown"));
    assert!(!displayed.contains("60 seconds"));
    for private in ["bearer-secret", "refresh-secret", "callback", "code="] {
        assert!(!displayed.contains(private));
    }
}

#[tokio::test]
async fn retry_deadlines_preserve_quota_reason_and_header() {
    for (reason, kind) in [
        ("QUOTA_EXCEEDED", FailureKind::QuotaExceeded),
        ("OTHER", FailureKind::RateLimited),
    ] {
        let before = Instant::now();
        let failure = failure(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "120")
                .set_body_json(serde_json::json!({"error":{"reason":reason}})),
        )
        .await;
        assert_eq!(failure.kind, kind);
        let deadline = failure.retry_at.unwrap();
        assert!(deadline >= before + Duration::from_secs(120));
        assert!(deadline <= Instant::now() + Duration::from_secs(120));
        assert!(failure.to_string().contains("120 seconds"));
        assert!(failure.active());
    }
    let expired = failure(ResponseTemplate::new(429).insert_header("Retry-After", "0")).await;
    assert!(!expired.active());
}

#[tokio::test]
async fn malformed_and_oversized_errors_are_bounded_and_safe() {
    for body in [
        "not json".to_owned(),
        format!(
            "{}{{\"error\":{{\"reason\":\"QUOTA_EXCEEDED\"}}}}",
            " ".repeat(20 * 1024)
        ),
    ] {
        let failure = failure(ResponseTemplate::new(429).set_body_string(body)).await;
        assert_eq!(failure.kind, FailureKind::RateLimited);
        assert!(failure.retry_at.is_some());
    }
}

#[test]
fn retry_headers_support_dates_and_do_not_treat_invalid_headers_as_quota_resets() {
    assert_eq!(retry_after(Some("invalid")), None);
    assert_eq!(retry_after(None), None);
    let date = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(90));
    assert!((88..=90).contains(&retry_after(Some(&date)).unwrap().as_secs()));
    let past = httpdate::fmt_http_date(SystemTime::now() - Duration::from_secs(90));
    assert_eq!(retry_after(Some(&past)), Some(Duration::ZERO));
}
