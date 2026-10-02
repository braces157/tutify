use crate::model::Track;
use anyhow::{Result, bail};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LyricLine {
    pub position_ms: u32,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    pub plain: Option<String>,
}

impl Lyrics {
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.plain.is_none()
    }

    pub fn current_line_index(&self, position_ms: u32) -> Option<usize> {
        self.lines
            .partition_point(|line| line.position_ms <= position_ms)
            .checked_sub(1)
    }
}

fn timestamp(value: &str) -> Option<u32> {
    let (minutes, seconds) = value.split_once(':')?;
    let minutes = minutes.parse::<u32>().ok()?;
    let (seconds, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let seconds = seconds.parse::<u32>().ok()?;
    if seconds >= 60 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let millis = format!("{fraction:0<3}").get(..3)?.parse::<u32>().ok()?;
    minutes
        .checked_mul(60_000)?
        .checked_add(seconds * 1000)?
        .checked_add(millis)
}

pub fn parse_lrc(lrc: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for line in lrc.lines() {
        let mut rest = line.trim();
        let mut times = Vec::new();
        while let Some(content) = rest.strip_prefix('[') {
            let Some((time, tail)) = content.split_once(']') else {
                break;
            };
            if let Some(position) = timestamp(time) {
                times.push(position);
            }
            rest = tail;
        }
        let text = rest
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>();
        for position_ms in times {
            lines.push(LyricLine {
                position_ms,
                text: text.clone(),
            });
        }
    }
    lines.sort_by_key(|l| l.position_ms);
    lines
}

pub async fn fetch(client: &Client, track: &Track) -> Result<Option<Lyrics>> {
    fetch_from(client, "https://lrclib.net", track).await
}

async fn fetch_from(client: &Client, base: &str, track: &Track) -> Result<Option<Lyrics>> {
    if let Some(json) =
        request(client, base, &track.name, &track.artists, track.duration_ms).await?
    {
        return Ok(Some(parse_lyrics(&json)));
    }
    // Spotify joins credits with commas, which LRCLIB may store under the lead
    // artist alone. Stable ID count prevents splitting a single artist's name
    // such as "Earth, Wind & Fire" or guessing from legacy metadata.
    let artists: Vec<_> = track.artists.split(',').map(str::trim).collect();
    if artists.len() > 1
        && artists.len() == track.artist_ids.len()
        && !artists[0].is_empty()
        && let Some(json) =
            request(client, base, &track.name, artists[0], track.duration_ms).await?
        && same_recording(&json, track, artists[0])
    {
        return Ok(Some(parse_lyrics(&json)));
    }
    Ok(None)
}

fn same_recording(json: &Value, track: &Track, artist: &str) -> bool {
    let key = |value: &str| value.trim().to_lowercase().nfc().collect::<String>();
    json["trackName"]
        .as_str()
        .is_some_and(|name| key(name) == key(&track.name))
        && json["artistName"]
            .as_str()
            .is_some_and(|name| key(name) == key(artist))
        && json["duration"].as_f64().is_some_and(|seconds| {
            seconds.is_finite() && (seconds - f64::from(track.duration_ms) / 1000.0).abs() <= 2.0
        })
}

async fn request(
    client: &Client,
    base: &str,
    track_name: &str,
    artist_name: &str,
    duration_ms: u32,
) -> Result<Option<Value>> {
    let track_name: String = track_name.nfc().collect();
    let artist_name: String = artist_name.nfc().collect();
    let duration_s = (duration_ms / 1000).to_string();
    let query = [
        ("track_name", track_name.as_str()),
        ("artist_name", artist_name.as_str()),
        ("duration", &duration_s),
    ];

    for attempt in 0..2 {
        let resp = client
            .get(format!("{base}/api/get"))
            .query(&query)
            .header(
                "User-Agent",
                concat!(
                    "Tuitify/",
                    env!("CARGO_PKG_VERSION"),
                    " (terminal spotify player)"
                ),
            )
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Could not reach lyrics service: {}", e.without_url()))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let status = resp.status();
        if matches!(status.as_u16(), 502..=504) {
            let delay = resp
                .headers()
                .get("retry-after")
                .map(|value| crate::auth::retry_delay(value.to_str().ok()))
                .unwrap_or(Duration::from_secs(1));
            if attempt == 0 && delay <= Duration::from_secs(2) {
                drop(resp);
                tokio::time::sleep(delay).await;
                continue;
            }
            bail!(
                "Lyrics service temporarily unavailable (HTTP {}). Try again shortly",
                status.as_u16()
            );
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            bail!("Lyrics service is busy (HTTP 429). Wait before retrying");
        }
        let resp = resp
            .error_for_status()
            .map_err(|e| anyhow::anyhow!("Lyrics service request failed: {}", e.without_url()))?;
        return Ok(Some(resp.json().await.map_err(|e| {
            anyhow::anyhow!("Invalid lyrics service response: {}", e.without_url())
        })?));
    }
    unreachable!("each request returns or retries once")
}

fn parse_lyrics(json: &Value) -> Lyrics {
    let synced = json["syncedLyrics"].as_str();
    let plain = json["plainLyrics"].as_str().map(|s| {
        s.chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .collect::<String>()
    });

    let lines = if let Some(synced_text) = synced {
        parse_lrc(synced_text)
    } else {
        vec![]
    };

    Lyrics { lines, plain }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{path, query_param},
    };

    fn duet() -> Track {
        Track {
            name: "Mưa".into(),
            artists: "Minh Vương M4U, Thùy Chi".into(),
            artist_ids: vec!["lead".into(), "guest".into()],
            duration_ms: 277_000,
            ..Track::default()
        }
    }

    fn response(track: &Track, artist: &str) -> Value {
        serde_json::json!({
            "trackName":track.name, "artistName":artist, "duration":278,
            "syncedLyrics":"[00:01.00] Test lời Việt\n[00:02.00] 日本語テスト",
            "plainLyrics":"Test lời Việt\n日本語テスト"
        })
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
            let lyrics = fetch_from(&Client::new(), &server.uri(), &track)
                .await
                .unwrap()
                .unwrap();
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
            .respond_with(
                ResponseTemplate::new(200).set_body_json(response(&track, &track.artists)),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            fetch_from(&Client::new(), &server.uri(), &track)
                .await
                .unwrap()
                .is_some()
        );
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
        assert!(
            fetch_from(&Client::new(), &server.uri(), &track)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
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
            Mock::given(query_param("artist_name", &track.artists))
                .respond_with(ResponseTemplate::new(404))
                .mount(&server)
                .await;
            Mock::given(query_param("artist_name", "Minh Vương M4U"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json))
                .mount(&server)
                .await;
            assert!(
                fetch_from(&Client::new(), &server.uri(), &track)
                    .await
                    .unwrap()
                    .is_none()
            );
            assert_eq!(server.received_requests().await.unwrap().len(), 2);
        }
    }

    #[tokio::test]
    async fn temporary_server_error_retries_same_query_once() {
        let server = MockServer::start().await;
        let track = duet();
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let json = response(&track, &track.artists);
        Mock::given(query_param("artist_name", &track.artists))
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
        assert!(
            fetch_from(&Client::new(), &server.uri(), &track)
                .await
                .unwrap()
                .is_some()
        );
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
            let error = fetch_from(&Client::new(), &server.uri(), &duet())
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains(&format!("HTTP {status}")));
            assert!(!error.contains("http://") && !error.contains("track_name"));
            assert_eq!(
                server.received_requests().await.unwrap().len() as u64,
                expected
            );
        }
    }

    #[tokio::test]
    #[ignore = "Live LRCLIB only; no audio, lyrics text output, or saved-state writes"]
    async fn live_vietnamese_and_english_lyrics_acceptance() -> Result<()> {
        let client = crate::auth::http_client()?;
        let mut castle = duet();
        castle.name = "Castle on the Hill".into();
        castle.artists = "Ed Sheeran".into();
        castle.artist_ids.truncate(1);
        castle.duration_ms = 261_000;
        for track in [duet(), castle] {
            let lyrics = fetch(&client, &track).await?.expect("Missing live lyrics");
            assert!(!lyrics.lines.is_empty());
            println!(
                "LIVE {} / {}: {} synced lines",
                track.name,
                track.artists,
                lyrics.lines.len()
            );
        }
        Ok(())
    }

    #[test]
    fn multiple_timestamps_and_invalid_times_are_handled_safely() {
        let lines = parse_lrc(
            "[00:01.5][00:05.050] Chorus\n[9999999999:00] overflow\n[00:60] bad seconds\n[00:NaN] bad float\n[00:-1] negative\n[00:03.日] unicode fraction",
        );
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].position_ms, 1500);
        assert_eq!(lines[1].position_ms, 5050);
        assert_eq!(lines[1].text, "Chorus");
    }
    #[test]
    fn parse_lrc_lines() {
        let sample = "[00:01.50] Line one\n[00:04.25] Line two\n[01:10.00] Line three";
        let lines = parse_lrc(sample);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].position_ms, 1500);
        assert_eq!(lines[0].text, "Line one");
        assert_eq!(lines[1].position_ms, 4250);
        assert_eq!(lines[2].position_ms, 70000);
    }

    #[test]
    fn current_line_lookup() {
        let sample = "[00:01.00] A\n[00:05.00] B\n[00:10.00] C";
        let lyrics = Lyrics {
            lines: parse_lrc(sample),
            plain: None,
        };
        assert_eq!(lyrics.current_line_index(500), None);
        assert_eq!(lyrics.current_line_index(2000), Some(0));
        assert_eq!(lyrics.current_line_index(6000), Some(1));
        assert_eq!(lyrics.current_line_index(15000), Some(2));
    }
}
