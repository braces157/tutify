use super::*;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path, query_param},
};

fn song() -> Track {
    Track {
        id: "deezer:1".into(),
        name: "Song".into(),
        artists: "Artist".into(),
        duration_ms: 200_000,
        playable: true,
        ..Track::default()
    }
}

fn candidate(id: &str, title: &str, code: &str) -> Value {
    json!({"trackId":id,"title":title,"artistNames":["Artist"],"duration":200000,
        "isrc":code,"playable":true})
}

fn flac() -> Vec<u8> {
    let mut bytes = vec![0; 42];
    bytes[..4].copy_from_slice(b"fLaC");
    bytes[7] = 34;
    let packed = (96000u64 << 44) | (1 << 41) | (23 << 36);
    bytes[18..26].copy_from_slice(&packed.to_be_bytes());
    bytes
}

#[tokio::test]
async fn ipv4_only_local_service_remains_reachable() {
    let server = MockServer::start().await;
    Mock::given(path("/track/123"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(flac()))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::mock(&format!("http://localhost:{}", server.address().port()));
    let audio = client.stream("123").await.unwrap();
    assert_eq!(audio.quality.label(), "FLAC 24-bit / 96 kHz");
    assert_eq!(audio.connect_addr, None);
}

#[tokio::test]
async fn exact_recording_uses_string_ids_and_measured_flac_quality() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/tracks"))
        .and(query_param("q", "Artist Song"))
        .and(query_param("limit", "6"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"tracks":[
            candidate("155142501534011391", "Song (Live)", "USABC2400002"),
            candidate("155142501534011392", "Song", "USABC2400001")
        ]})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/track/155142501534011392"))
        .and(header("range", "bytes=0-41"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(flac()))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::mock(&server.uri());
    let id = client
        .recording_id(&song(), Some("USABC2400001"))
        .await
        .unwrap();
    let audio = client.stream(&id).await.unwrap();
    assert_eq!(
        audio.url,
        format!("{}/track/155142501534011392", server.uri())
    );
    assert_eq!(audio.source, "Monochrome");
    assert_eq!(audio.quality.label(), "FLAC 24-bit / 96 kHz");
    assert_eq!(
        client
            .recording_id(&song(), Some("USABC2400001"))
            .await
            .unwrap(),
        id
    );
    assert!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| !request.headers.contains_key("authorization"))
    );
}

#[tokio::test]
async fn rejects_wrong_versions_wrong_isrc_ambiguous_matches_and_malformed_ids() {
    for (rows, isrc) in [
        (vec![candidate("123", "Song (Live)", "")], None),
        (
            vec![candidate("123", "Song", "USABC2400002")],
            Some("USABC2400001"),
        ),
        (
            vec![candidate("123", "Song", ""), candidate("124", "Song", "")],
            None,
        ),
        (
            vec![candidate("../other", "Song", "USABC2400001")],
            Some("USABC2400001"),
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(path("/search/tracks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"tracks":rows})))
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            Client::mock(&server.uri())
                .recording_id(&song(), isrc)
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn search_and_audio_rate_limits_stop_further_requests_without_leaking_response_bodies() {
    for endpoint in ["/search/tracks", "/track/123"] {
        let server = MockServer::start().await;
        Mock::given(path(endpoint))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("Retry-After", "120")
                    .set_body_string("private upstream error"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let client = Client::mock(&server.uri());
        let error = if endpoint == "/search/tracks" {
            client.recording_id(&song(), None).await.unwrap_err()
        } else {
            client.stream("123").await.err().unwrap()
        };
        assert!(ServiceFailure::is(&error, FailureKind::RateLimited));
        assert!(!format!("{error:#}").contains("private"));
        assert!(client.recording_id(&song(), None).await.is_err());
        assert!(client.stream("124").await.is_err());
    }
}

#[tokio::test]
async fn resolver_keeps_original_deezer_metadata_and_cancels_slow_searches() {
    let deezer = MockServer::start().await;
    let server = MockServer::start().await;
    let mut raw = crate::deezer::tests::item(1, "Song");
    raw["isrc"] = json!("USABC2400001");
    Mock::given(path("/track/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(raw))
        .mount(&deezer)
        .await;
    Mock::given(path("/search/tracks"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"tracks":[candidate("123", "Song", "USABC2400001")]})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/track/123"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(flac()))
        .mount(&server)
        .await;
    let resolver = Resolver {
        client: Client::mock(&server.uri()),
        catalog: Catalog::offline()
            .unwrap()
            .with_deezer(crate::deezer::Client::mock(&deezer.uri())),
    };
    let (audio, track) = resolver.audio("deezer:1").await.unwrap();
    assert_eq!(track.id, "deezer:1");
    assert_eq!(track.name, "Song");
    assert!(audio.url.ends_with("/track/123"));
    server.reset().await;
    Mock::given(path("/search/tracks"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(5))
                .set_body_json(json!({"tracks":[]})),
        )
        .mount(&server)
        .await;
    let client = Client::mock(&server.uri());
    assert!(
        tokio::time::timeout(
            Duration::from_millis(30),
            client.recording_id(&song(), None)
        )
        .await
        .is_err()
    );
}

#[test]
fn only_valid_flac_streaminfo_supplies_quality() {
    assert_eq!(flac_quality(&flac()).unwrap().sample_rate, Some(96000));
    assert!(flac_quality(b"{\"error\":\"no audio\"}").is_none());
    let mut invalid = flac();
    invalid[7] = 33;
    assert!(flac_quality(&invalid).is_none());
}

#[tokio::test]
#[ignore = "Live Monochrome-only provider, muted playback and seek, no saved state changes"]
async fn live_monochrome_audio_probe() -> Result<()> {
    use crate::playback::{Command, Event, Playback};

    let catalog = Catalog::offline()?.with_deezer(crate::deezer::Client::new()?);
    let mut playback =
        Playback::spawn_monochrome(catalog, 0, crate::visualizer::AudioVisualizer::new())?;
    let id = std::env::var("TUITIFY_LIVE_TRACK").unwrap_or_else(|_| "deezer:1746860357".into());
    playback.commands.send(Command::Load {
        id: id.clone(),
        position_ms: 0,
        generation: 101,
    })?;
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        let mut measured_flac = false;
        let mut monochrome_source = false;
        let mut sought = false;
        loop {
            match playback
                .events
                .recv()
                .await
                .context("Audio worker stopped")?
            {
                Event::Fallback { source, .. } => {
                    ensure!(source == "Monochrome", "Unexpected audio source: {source}");
                    monochrome_source = true;
                    println!("AUDIO SOURCE {source}");
                }
                Event::Metadata { track, .. } => {
                    ensure!(track.id == id, "Original queue identity changed");
                    println!("METADATA {} / {}", track.name, track.artists);
                }
                Event::Quality { quality, .. } => {
                    measured_flac = quality.codec == Some(Codec::Flac)
                        && quality.sample_rate.is_some()
                        && quality.bit_depth.is_some();
                    println!("QUALITY {}", quality.label());
                }
                Event::Position { position_ms, .. } if !sought && position_ms >= 5_000 => {
                    playback.commands.send(Command::Seek(60_000))?;
                    sought = true;
                    println!("SEEK requested 60 seconds");
                }
                Event::Position { position_ms, .. } if sought && position_ms >= 65_000 => {
                    ensure!(
                        monochrome_source && measured_flac,
                        "Monochrome FLAC was not verified"
                    );
                    break;
                }
                Event::TrackError { message, .. } | Event::Error(message) => {
                    anyhow::bail!("{message}")
                }
                Event::Completed(_) | Event::PremiumRequired => {
                    anyhow::bail!("Audio stopped before playback and seek were verified")
                }
                _ => (),
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await;
    let _ = playback.commands.send(Command::Stop);
    result.context("Monochrome audio probe timed out")??;
    println!(
        "PASS: Monochrome alone played five seconds, sought to 60 seconds and played five more, muted, with the original queue identity"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "Live automatic provider chain, five seconds of muted Windows playback, no saved state changes"]
async fn live_automatic_audio_probe() -> Result<()> {
    use crate::{
        playback::{Command, Event},
        source::Choice,
    };
    let selection = crate::source::resolve(Choice::Auto, false).await?;
    let mut config = crate::storage::Storage::local_read_only()?.config()?;
    config.automatic_audio = true;
    config.volume = 0;
    config.discord_rpc = false;
    let mut services = crate::providers::open(
        selection.source,
        &config,
        crate::visualizer::AudioVisualizer::new(),
    )?;
    let id = std::env::var("TUITIFY_LIVE_TRACK").unwrap_or_else(|_| "deezer:2493747201".into());
    services.playback.commands.send(Command::Load {
        id: id.clone(),
        position_ms: 0,
        generation: 101,
    })?;
    tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            match services
                .playback
                .events
                .recv()
                .await
                .context("Audio worker stopped")?
            {
                Event::Fallback { source, .. } => println!("AUDIO SOURCE {source}"),
                Event::Metadata { track, .. } => {
                    assert_eq!(track.id, id);
                    println!("METADATA {} / {}", track.name, track.artists);
                }
                Event::Quality { quality, .. } => println!("QUALITY {}", quality.label()),
                Event::Position { position_ms, .. } if position_ms >= 5000 => break,
                Event::TrackError { message, .. } | Event::Error(message) => {
                    anyhow::bail!("{message}")
                }
                Event::Completed(_) | Event::PremiumRequired => {
                    anyhow::bail!("Audio stopped before five seconds")
                }
                _ => (),
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("Automatic audio probe timed out")??;
    services.playback.commands.send(Command::Stop)?;
    println!("PASS: automatic audio played five seconds, muted, with the original queue identity");
    Ok(())
}
