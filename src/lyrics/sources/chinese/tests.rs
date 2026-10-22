use super::super::{ENDPOINTS, Endpoints, Provider, fetch_from, lookup};
use super::*;
use std::time::Duration;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, path, query_param},
};

const LRC: &str = "[00:01.00] 中文測試\n[00:04.50] 第二句歌詞";

fn track() -> Track {
    Track {
        name: "紅豆".into(),
        artists: "王菲".into(),
        duration_ms: 255_000,
        artist_ids: vec!["artist".into()],
        ..Track::default()
    }
}

fn endpoints(base: &str) -> Endpoints<'_> {
    Endpoints {
        lrclib: base,
        ovh: base,
        netease: base,
        qq_search: base,
        qq_lyrics: base,
    }
}

fn netease_song(id: u64) -> Value {
    json!({"id": id, "name": "红豆", "duration": 255_000,
        "artists": [{"name": "王菲"}], "album": {"name": "唱游"}})
}

fn qq_song(id: &str) -> Value {
    json!({"mid": id, "name": "红豆", "title": "红豆", "interval": 255,
        "singer": [{"name": "王菲"}], "album": {"title": "唱游"}})
}

async fn netease_search(server: &MockServer, songs: Vec<Value>) {
    Mock::given(path("/api/search/get"))
        .and(query_param("s", "红豆 王菲"))
        .and(header("referer", "https://music.163.com/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"code": 200, "result": {"songs": songs}})),
        )
        .expect(1)
        .mount(server)
        .await;
}

async fn qq_search(server: &MockServer, songs: Vec<Value>) {
    Mock::given(path("/cgi-bin/musicu.fcg"))
        .and(header("referer", "https://y.qq.com/"))
        .respond_with(move |req: &wiremock::Request| {
            let payload: Value = serde_json::from_str(
                &req.url
                    .query_pairs()
                    .find(|(key, _)| key == "data")
                    .unwrap()
                    .1,
            )
            .unwrap();
            assert_eq!(payload["req_1"]["param"]["query"], "红豆 王菲");
            ResponseTemplate::new(200).set_body_json(json!({"code": 0, "req_1": {"code": 0,
                "data": {"body": {"song": {"list": songs}}}}}))
        })
        .expect(1)
        .mount(server)
        .await;
}

#[test]
fn simplified_matching_preserves_originals_accents_and_recording_versions() {
    assert_eq!(
        simplified("紅豆 — 陳奕迅 / 周杰倫"),
        "红豆 — 陈奕迅 / 周杰伦"
    );
    assert_eq!(chinese_key("紅豆!"), chinese_key("红豆"));
    assert_ne!(chinese_key("紅豆 (Live)"), chinese_key("红豆"));
    assert_ne!(chinese_key("Mưa"), chinese_key("Mua"));
    assert_eq!(simplified("🎵 王菲"), "🎵 王菲");
}

#[tokio::test]
async fn netease_keeps_timestamps_and_matches_traditional_metadata() {
    let server = MockServer::start().await;
    netease_search(&server, vec![netease_song(10)]).await;
    Mock::given(path("/api/song/lyric"))
        .and(query_param("id", "10"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"code": 200, "lrc": {"lyric": LRC}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let track = track();
    let (_, result) = lookup(
        &Client::new(),
        &endpoints(&server.uri()),
        &track,
        Provider::Netease,
    )
    .await;
    let lyrics = result.unwrap().unwrap();
    assert_eq!(lyrics.lines[1].position_ms, 4500);
    assert_eq!(lyrics.lines[0].text, "中文測試");
    assert_eq!(track.name, "紅豆");
}

#[tokio::test]
async fn qq_handles_plain_base64_and_html_encoded_lrc() {
    for text in [
        LRC.to_owned(),
        STANDARD.encode(LRC),
        LRC.replace('[', "&#91;")
            .replace(':', "&#58;")
            .replace(']', "&#93;"),
    ] {
        let server = MockServer::start().await;
        qq_search(&server, vec![qq_song("Song123")]).await;
        Mock::given(path("/lyric/fcgi-bin/fcg_query_lyric_new.fcg"))
            .and(query_param("songmid", "Song123"))
            .and(query_param("nobase64", "1"))
            .and(header("referer", "https://y.qq.com/"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"code": 0, "lyric": text})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let (_, result) = lookup(
            &Client::new(),
            &endpoints(&server.uri()),
            &track(),
            Provider::Qq,
        )
        .await;
        let lyrics = result.unwrap().unwrap();
        assert_eq!(lyrics.lines[1].position_ms, 4500);
        assert_eq!(lyrics.lines[1].text, "第二句歌詞");
    }
}

#[tokio::test]
async fn qq_retains_untimed_raw_or_base64_lyrics() {
    for plain in [
        "第一句歌词\n第二句歌词",
        "First line\nSecond line",
        "Kiss",
        "TWFu",
        "YWJj",
    ] {
        for text in [plain.to_owned(), STANDARD.encode(plain)] {
            let server = MockServer::start().await;
            qq_search(&server, vec![qq_song("Song123")]).await;
            Mock::given(path("/lyric/fcgi-bin/fcg_query_lyric_new.fcg"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"code":0,"lyric":text})),
                )
                .mount(&server)
                .await;
            let (_, result) = lookup(
                &Client::new(),
                &endpoints(&server.uri()),
                &track(),
                Provider::Qq,
            )
            .await;
            let lyrics = result.unwrap().unwrap();
            assert!(lyrics.lines.is_empty());
            assert_eq!(lyrics.plain.as_deref(), Some(plain));
        }
    }
}

#[test]
fn wrong_singers_guest_credits_live_versions_and_durations_are_rejected() {
    let mut cases = vec![];
    for (field, value) in [
        ("artists", json!([{"name": "翻唱歌手"}])),
        ("artists", json!([{"name": "翻唱歌手"}, {"name": "王菲"}])),
        ("name", json!("红豆 (Live)")),
        ("name", json!("红豆 DJ版")),
        ("duration", json!(275_000)),
        ("duration", Value::Null),
    ] {
        let mut song = netease_song(10);
        song[field] = value;
        cases.push(song);
    }
    assert!(
        matching(
            cases.iter().filter_map(netease_candidate).collect(),
            &track()
        )
        .is_empty()
    );
    let mut live = qq_song("Live123");
    live["title"] = json!("红豆 (Live)");
    assert!(matching(vec![qq_candidate(&live).unwrap()], &track()).is_empty());
}

#[test]
fn explicit_artist_aliases_and_verified_duets_can_match() {
    let mut song = netease_song(10);
    song["artists"] = json!([{"name": "王菲", "alias": ["Faye Wong"]}]);
    let mut wanted = track();
    wanted.artists = "Faye Wong".into();
    assert_eq!(
        matching(vec![netease_candidate(&song).unwrap()], &wanted).len(),
        1
    );
    song["artists"] = json!([{"name": "王菲"}, {"name": "陈奕迅"}]);
    wanted.artists = "王菲, 陳奕迅".into();
    wanted.artist_ids.push("guest".into());
    assert_eq!(
        matching(vec![netease_candidate(&song).unwrap()], &wanted).len(),
        1
    );
}

#[tokio::test]
async fn candidate_lookup_skips_covers_and_empty_matches_then_finds_timed_lyrics() {
    let server = MockServer::start().await;
    let mut cover = netease_song(1);
    cover["artists"] = json!([{"name": "Cover Band"}]);
    netease_search(&server, vec![cover, netease_song(10), netease_song(11)]).await;
    Mock::given(path("/api/song/lyric"))
        .and(query_param("id", "10"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"code": 200, "lrc": {"lyric": ""}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/song/lyric"))
        .and(query_param("id", "11"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"code": 200, "lrc": {"lyric": LRC}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let (_, result) = lookup(
        &Client::new(),
        &endpoints(&server.uri()),
        &track(),
        Provider::Netease,
    )
    .await;
    assert_eq!(result.unwrap().unwrap().lines.len(), 2);
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[test]
fn instrumental_metadata_empty_and_broken_timing_do_not_create_fake_sync() {
    for text in [
        "",
        "[ti:Song]\n[ar:Singer]",
        "[00:00]纯音乐，请欣赏",
        "[00:00]純音樂, 請欣賞",
        "[00:00]\u{1b}",
    ] {
        assert!(catalog_lrc(text, &track()).is_none());
    }
    for text in [
        "[00:00]First line\n[00:00]Second line",
        "[00:01]First line\n[99:01]Second line",
    ] {
        let lyrics = catalog_lrc(text, &track()).unwrap();
        assert!(lyrics.lines.is_empty());
        assert!(lyrics.plain.is_some());
    }
}

#[tokio::test]
async fn chinese_results_are_not_delayed_by_an_outage_in_another_provider() {
    let server = MockServer::start().await;
    netease_search(&server, vec![netease_song(10)]).await;
    Mock::given(path("/api/get"))
        .respond_with(ResponseTemplate::new(503).set_delay(Duration::from_secs(8)))
        .mount(&server)
        .await;
    Mock::given(path("/cgi-bin/musicu.fcg"))
        .respond_with(ResponseTemplate::new(503).set_delay(Duration::from_secs(8)))
        .mount(&server)
        .await;
    Mock::given(path("/api/song/lyric"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"code": 200, "lrc": {"lyric": LRC}})),
        )
        .mount(&server)
        .await;
    let lyrics = tokio::time::timeout(
        Duration::from_secs(2),
        fetch_from(&Client::new(), &endpoints(&server.uri()), &track()),
    )
    .await
    .expect("a timed Chinese result must not wait for other providers")
    .unwrap()
    .unwrap();
    assert_eq!(lyrics.lines.len(), 2);
}

#[tokio::test]
async fn qq_can_supply_timed_lyrics_when_netease_is_unavailable() {
    let server = MockServer::start().await;
    Mock::given(path("/api/search/get"))
        .respond_with(ResponseTemplate::new(429))
        .expect(1)
        .mount(&server)
        .await;
    qq_search(&server, vec![qq_song("Song123")]).await;
    Mock::given(path("/lyric/fcgi-bin/fcg_query_lyric_new.fcg"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"code": 0, "lyric": LRC})))
        .mount(&server)
        .await;
    let lyrics = fetch_from(&Client::new(), &endpoints(&server.uri()), &track())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lyrics.lines.len(), 2);
}

#[tokio::test]
async fn plain_results_survive_later_candidate_errors() {
    let server = MockServer::start().await;
    netease_search(&server, vec![netease_song(10), netease_song(11)]).await;
    Mock::given(path("/api/song/lyric"))
        .and(query_param("id", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"code": 200,
            "lrc": {"lyric": "[00:00]First line\n[00:00]Second line"}})))
        .mount(&server)
        .await;
    Mock::given(path("/api/song/lyric"))
        .and(query_param("id", "11"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&server)
        .await;
    let (_, result) = lookup(
        &Client::new(),
        &endpoints(&server.uri()),
        &track(),
        Provider::Netease,
    )
    .await;
    let lyrics = result.unwrap().unwrap();
    assert!(lyrics.lines.is_empty());
    assert_eq!(lyrics.plain.as_deref(), Some("First line\nSecond line"));
}

#[tokio::test]
async fn malformed_json_and_application_error_codes_remain_errors() {
    for provider in [Provider::Netease, Provider::Qq] {
        for body in ["not json", "{}", "{\"code\":301}"] {
            let server = MockServer::start().await;
            Mock::given(path(match provider {
                Provider::Netease => "/api/search/get",
                _ => "/cgi-bin/musicu.fcg",
            }))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
            let (_, result) = lookup(
                &Client::new(),
                &endpoints(&server.uri()),
                &track(),
                provider,
            )
            .await;
            let error = result.unwrap_err().to_string();
            assert!(!error.contains("http://") && !error.contains("红豆"));
        }
    }
}

#[test]
fn entities_handle_numeric_unicode_and_leave_unknown_entities_intact() {
    assert_eq!(
        entities("&#91;00&#58;01&#93; &#x4e2d; &amp; &quot;x&quot; &unknown;"),
        "[00:01] 中 & \"x\" &unknown;"
    );
}

#[tokio::test]
#[ignore = "Live NetEase metadata/lyrics only; no audio, lyric text output, or saved-state writes"]
async fn live_netease_chinese_acceptance() -> Result<()> {
    let track = Track {
        name: "十年".into(),
        artists: "陳奕迅".into(),
        duration_ms: 205_000,
        artist_ids: vec!["artist".into()],
        ..Track::default()
    };
    let (_, result) = lookup(
        &crate::auth::http_client()?,
        &ENDPOINTS,
        &track,
        Provider::Netease,
    )
    .await;
    let mut lyrics = result?.expect("Missing NetEase lyrics");
    assert!(lyrics.lines.len() > 10);
    lyrics.prepare_readings();
    assert!(lyrics.readings.iter().any(|reading| {
        reading
            .get(crate::lyrics::ReadingMode::Mandarin, false)
            .is_some()
    }));
    println!(
        "LIVE NetEase: {} synchronized lines, pinyin generated",
        lyrics.lines.len()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "Live QQ metadata/lyrics only; no audio, lyric text output, or saved-state writes"]
async fn live_qq_chinese_acceptance() -> Result<()> {
    let track = Track {
        name: "晴天".into(),
        artists: "周杰倫".into(),
        duration_ms: 269_000,
        artist_ids: vec!["artist".into()],
        ..Track::default()
    };
    let (_, result) = lookup(
        &crate::auth::http_client()?,
        &ENDPOINTS,
        &track,
        Provider::Qq,
    )
    .await;
    let mut lyrics = result?.expect("Missing QQ lyrics");
    assert!(lyrics.lines.len() > 10);
    lyrics.prepare_readings();
    assert!(lyrics.readings.iter().any(|reading| {
        reading
            .get(crate::lyrics::ReadingMode::Mandarin, false)
            .is_some()
    }));
    println!(
        "LIVE QQ Music: {} synchronized lines, pinyin generated",
        lyrics.lines.len()
    );
    Ok(())
}
