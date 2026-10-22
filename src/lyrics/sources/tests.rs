use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use unicode_normalization::UnicodeNormalization;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{path, path_regex, query_param},
};

fn endpoints(base: &str) -> Endpoints<'_> {
    Endpoints {
        lrclib: base,
        ovh: base,
        netease: base,
        qq_search: base,
        qq_lyrics: base,
    }
}

fn duet() -> Track {
    Track {
        name: "Mưa".into(),
        artists: "Minh Vương M4U, Thùy Chi".into(),
        artist_ids: vec!["lead".into(), "guest".into()],
        duration_ms: 277_000,
        ..Track::default()
    }
}

async fn lookup(server: &MockServer, track: &Track) -> Result<Option<Lyrics>> {
    let mut best = None;
    fetch_lrclib(&Client::new(), &server.uri(), track, &mut best).await?;
    Ok(best)
}
fn response(track: &Track, artist: &str) -> Value {
    serde_json::json!({
        "trackName":track.name, "artistName":artist, "duration":278,
        "syncedLyrics":"[00:01.00] Test lời Việt\n[00:02.00] 日本語テスト",
        "plainLyrics":"Test lời Việt\n日本語テスト"
    })
}

#[test]
fn recording_keys_preserve_accents_and_version_words() {
    assert_eq!(key("Mưa!"), key(&"MƯA".nfd().collect::<String>()));
    assert_ne!(key("Mưa"), key("Mua"));
    assert_ne!(key("ค่า"), key("คา"));
    assert_ne!(key("Song (Live)"), key("Song"));
    assert_ne!(key("Song - Remix"), key("Song"));
}

#[tokio::test]
async fn unicode_duets_retry_primary_credit_without_removing_accents() {
    for (name, credits) in [
        ("Mưa", "Minh Vương M4U, Thùy Chi"),
        ("夜の歌", "歌手一, 歌手二"),
    ] {
        let server = MockServer::start().await;
        let mut track = duet();
        track.name = name.nfd().collect();
        track.artists = credits.nfd().collect();
        let primary = credits.split(',').next().unwrap();
        Mock::given(path("/api/get"))
            .and(query_param("track_name", name))
            .and(query_param("artist_name", credits))
            .and(query_param("duration", "277"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/api/get"))
            .and(query_param("track_name", name))
            .and(query_param("artist_name", primary))
            .respond_with(ResponseTemplate::new(200).set_body_json(response(&track, primary)))
            .expect(1)
            .mount(&server)
            .await;
        let lyrics = lookup(&server, &track).await.unwrap().unwrap();
        assert_eq!(lyrics.lines[0].text, "Test lời Việt");
        assert_eq!(lyrics.lines[1].text, "日本語テスト");
    }
}

#[tokio::test]
async fn exact_full_credit_result_does_not_fall_back() {
    let server = MockServer::start().await;
    let track = duet();
    Mock::given(path("/api/get"))
        .and(query_param("artist_name", &track.artists))
        .respond_with(ResponseTemplate::new(200).set_body_json(response(&track, &track.artists)))
        .expect(1)
        .mount(&server)
        .await;
    assert!(lookup(&server, &track).await.unwrap().is_some());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn comma_in_single_artist_name_is_not_split() {
    let server = MockServer::start().await;
    let mut track = duet();
    track.artists = "Earth, Wind & Fire".into();
    track.artist_ids.truncate(1);
    Mock::given(path("/api/get"))
        .and(query_param("artist_name", &track.artists))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    assert!(lookup(&server, &track).await.unwrap().is_none());
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn primary_credit_fallback_rejects_other_recordings() {
    let track = duet();
    let mut cases = Vec::new();
    for (field, value) in [
        ("trackName", serde_json::json!("Other song")),
        ("artistName", serde_json::json!("Other artist")),
        ("duration", serde_json::json!(300)),
        ("duration", Value::Null),
    ] {
        let mut json = response(&track, "Minh Vương M4U");
        json[field] = value;
        cases.push(json);
    }
    for json in cases {
        let server = MockServer::start().await;
        Mock::given(path("/api/get"))
            .and(query_param("artist_name", &track.artists))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(path("/api/get"))
            .and(query_param("artist_name", "Minh Vương M4U"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json))
            .mount(&server)
            .await;
        assert!(lookup(&server, &track).await.unwrap().is_none());
        assert_eq!(server.received_requests().await.unwrap().len(), 4);
    }
}

#[tokio::test]
async fn temporary_server_error_retries_same_query_once() {
    let server = MockServer::start().await;
    let track = duet();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let json = response(&track, &track.artists);
    Mock::given(path("/api/get"))
        .and(query_param("artist_name", &track.artists))
        .respond_with(move |_: &wiremock::Request| {
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(503).insert_header("Retry-After", "0")
            } else {
                ResponseTemplate::new(200).set_body_json(json.clone())
            }
        })
        .expect(2)
        .mount(&server)
        .await;
    assert!(lookup(&server, &track).await.unwrap().is_some());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn persistent_outage_and_rate_limits_keep_errors_clear_and_requests_bounded() {
    for (status, retry, expected) in [(503, "0", 2), (503, "3600", 1), (429, "0", 1)] {
        let server = MockServer::start().await;
        Mock::given(path("/api/get"))
            .respond_with(ResponseTemplate::new(status).insert_header("Retry-After", retry))
            .expect(expected)
            .mount(&server)
            .await;
        let error = lookup(&server, &duet()).await.unwrap_err().to_string();
        assert!(error.contains(&format!("HTTP {status}")));
        assert!(!error.contains("http://") && !error.contains("track_name"));
        assert_eq!(
            server.received_requests().await.unwrap().len() as u64,
            expected
        );
    }
}

#[tokio::test]
async fn search_prefers_synced_lyrics_and_rejects_wrong_recordings() {
    let server = MockServer::start().await;
    let track = duet();
    let mut plain = response(&track, &track.artists);
    plain["syncedLyrics"] = Value::Null;
    let mut timed = response(&track, "Minh Vương M4U");
    timed["trackName"] = serde_json::json!("MƯA!");
    timed["duration"] = serde_json::json!(281);
    let mut live = timed.clone();
    live["trackName"] = serde_json::json!("Mưa (Live)");
    let mut cover = timed.clone();
    cover["artistName"] = serde_json::json!("Cover Band");
    let mut long = timed.clone();
    long["duration"] = serde_json::json!(290);
    let mut unaccented = timed.clone();
    unaccented["trackName"] = serde_json::json!("Mua");
    for bad in [&mut live, &mut cover, &mut long, &mut unaccented] {
        bad["syncedLyrics"] = serde_json::json!("[00:01] Wrong recording");
    }
    Mock::given(path("/api/search"))
        .and(query_param("artist_name", &track.artists))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            live, cover, long, unaccented, plain, timed
        ])))
        .expect(1)
        .mount(&server)
        .await;
    let lyrics = fetch_from(&Client::new(), &endpoints(&server.uri()), &track)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lyrics.lines[0].text, "Test lời Việt");
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn primary_artist_search_recovers_a_duet_missing_from_exact_lookup() {
    let server = MockServer::start().await;
    let track = duet();
    Mock::given(path("/api/search"))
        .and(query_param("track_name", "Mưa"))
        .and(query_param("artist_name", "Minh Vương M4U"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(vec![response(&track, "Minh Vương M4U")]),
        )
        .expect(1)
        .mount(&server)
        .await;
    assert!(
        fetch_from(&Client::new(), &endpoints(&server.uri()), &track)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test]
async fn plain_exact_match_is_upgraded_by_search_but_survives_search_failure() {
    for status in [200, 429] {
        let server = MockServer::start().await;
        let track = duet();
        let mut plain = response(&track, &track.artists);
        plain["syncedLyrics"] = Value::Null;
        Mock::given(path("/api/get"))
            .and(query_param("artist_name", &track.artists))
            .respond_with(ResponseTemplate::new(200).set_body_json(plain))
            .mount(&server)
            .await;
        Mock::given(path("/api/search"))
            .respond_with(if status == 200 {
                ResponseTemplate::new(200).set_body_json(vec![response(&track, &track.artists)])
            } else {
                ResponseTemplate::new(status)
            })
            .expect(1)
            .mount(&server)
            .await;
        let lyrics = fetch_from(&Client::new(), &endpoints(&server.uri()), &track)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(!lyrics.lines.is_empty(), status == 200);
        assert!(lyrics.plain.is_some());
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            if status == 200 { 3 } else { 5 }
        );
    }
}

#[tokio::test]
async fn empty_lrclib_responses_fall_back_to_ovh_with_encoded_unicode_segments() {
    let server = MockServer::start().await;
    let mut track = duet();
    track.name = "Mưa / 夜 ?#".into();
    track.artists = "AC/DC".into();
    track.artist_ids.truncate(1);
    let mut blank = response(&track, &track.artists);
    blank["syncedLyrics"] = serde_json::json!("[00:01]\n[00:02] ");
    blank["plainLyrics"] = serde_json::json!("\r\n\t\u{1b}");
    Mock::given(path("/api/get"))
        .respond_with(ResponseTemplate::new(200).set_body_json(blank))
        .mount(&server)
        .await;
    Mock::given(path_regex("^/v1/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"lyrics": " Test lời Việt\r\n日本語\u{1b}\rLast line "}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let lyrics = fetch_from(&Client::new(), &endpoints(&server.uri()), &track)
        .await
        .unwrap()
        .unwrap();
    assert!(lyrics.lines.is_empty());
    assert_eq!(
        lyrics.plain.as_deref(),
        Some("Test lời Việt\n日本語\nLast line")
    );
    let requests = server.received_requests().await.unwrap();
    let url = &requests.last().unwrap().url;
    assert_eq!(
        url.path(),
        "/v1/AC%2FDC/M%C6%B0a%20%2F%20%E5%A4%9C%20%3F%23"
    );
    assert!(url.query().is_none() && url.fragment().is_none());
}

#[tokio::test]
async fn lrclib_outage_rate_limit_and_invalid_json_do_not_block_ovh() {
    for status in [503, 429, 200] {
        let server = MockServer::start().await;
        Mock::given(path("/api/get"))
            .respond_with(
                ResponseTemplate::new(status)
                    .insert_header("Retry-After", "3600")
                    .set_body_string("invalid json"),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path_regex("^/v1/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"lyrics": "Fallback first line\nFallback second line"}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let lyrics = fetch_from(&Client::new(), &endpoints(&server.uri()), &duet())
            .await
            .unwrap()
            .unwrap();
        assert!(lyrics.lines.is_empty() && lyrics.plain.is_some());
        assert_eq!(server.received_requests().await.unwrap().len(), 4);
    }
}

#[tokio::test]
async fn ovh_retries_verified_primary_credit_after_missing_full_credit() {
    let server = MockServer::start().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    Mock::given(path_regex("^/v1/"))
        .respond_with(move |request: &wiremock::Request| {
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                assert!(request.url.path().contains(',') || request.url.path().contains("%2C"));
                ResponseTemplate::new(404)
            } else {
                assert!(!request.url.path().contains(',') && !request.url.path().contains("%2C"));
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"lyrics": "Primary credit lyric line"}))
            }
        })
        .expect(2)
        .mount(&server)
        .await;
    assert!(
        fetch_from(&Client::new(), &endpoints(&server.uri()), &duet())
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn all_missing_is_distinct_from_a_failed_provider() {
    for status in [404, 429] {
        let server = MockServer::start().await;
        Mock::given(path("/api/get"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;
        let result = fetch_from(&Client::new(), &endpoints(&server.uri()), &duet()).await;
        if status == 404 {
            assert!(result.unwrap().is_none());
            assert_eq!(server.received_requests().await.unwrap().len(), 8);
        } else {
            let error = result.unwrap_err().to_string();
            assert!(error.contains("HTTP 429"));
            assert!(!error.contains("http://") && !error.contains("track_name"));
        }
    }
}

#[tokio::test]
async fn a_slow_primary_service_yields_to_the_secondary_service() {
    let server = MockServer::start().await;
    Mock::given(path("/api/get"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(8)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path_regex("^/v1/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"lyrics": "Fallback after timeout"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let lyrics = tokio::time::timeout(
        Duration::from_secs(7),
        fetch_from(&Client::new(), &endpoints(&server.uri()), &duet()),
    )
    .await
    .expect("secondary service should run after the 6s request deadline")
    .unwrap()
    .unwrap();
    assert_eq!(lyrics.plain.as_deref(), Some("Fallback after timeout"));
}

#[tokio::test]
#[ignore = "Live lyrics.ovh only; no audio, lyric text output, or saved-state writes"]
async fn live_ovh_english_lyrics_acceptance() -> Result<()> {
    let track = Track {
        name: "Castle on the Hill".into(),
        artists: "Ed Sheeran".into(),
        duration_ms: 261_000,
        ..Track::default()
    };
    let lyrics = fetch_ovh(
        &crate::auth::http_client()?,
        "https://api.lyrics.ovh",
        &track,
    )
    .await?
    .expect("Missing lyrics.ovh lyrics");
    assert!(lyrics.lines.is_empty() && lyrics.plain.is_some());
    println!(
        "LIVE lyrics.ovh: {} plain lines",
        lyrics.plain.unwrap().lines().count()
    );
    Ok(())
}
