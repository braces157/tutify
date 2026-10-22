use super::*;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{path, query_param},
};

const SPOTIFY: &str = "0000000000000000000001";
const ISRC: &str = "JPW462364925";

#[test]
fn youtube_bilingual_titles_match_complete_aliases_without_accepting_other_versions() {
    let artist = "Ryokuoushoku Shakai";
    let original = track(SPOTIFY, "Be a flower", artist, 200_000);
    for title in [
        "花になって - Be a flower",
        "Be a flower - 花になって",
        "花になって - Be a flower (Official Audio)",
    ] {
        let video = track("youtube:voAu0YRWUNE", title, artist, 199_333);
        assert!(youtube_recording(&original, &video), "{title}");
        assert!(
            youtube_recording(&track(SPOTIFY, "花になって", artist, 200_000), &video),
            "{title}"
        );
        if !title.contains("Official Audio") {
            assert!(youtube_recording(&video, &original), "reverse: {title}");
        }
    }
    for title in [
        "Something Else - Be a flower",
        "花になって - Be a flower (Instrumental)",
        "花になって (Live) - Be a flower",
        "花になってライブ - Be a flower",
        "花になって - Be a flower Remix",
        "花になって - Wildflower",
        "花になって - Be a flower - Cover",
    ] {
        assert!(
            !youtube_recording(
                &original,
                &track("youtube:voAu0YRWUNE", title, artist, 200_000)
            ),
            "{title}"
        );
    }
    assert!(!youtube_recording(
        &original,
        &track(
            "youtube:voAu0YRWUNE",
            "花になって - Be a flower",
            "Cover Artist",
            200_000
        )
    ));
    assert!(!youtube_recording(
        &original,
        &track(
            "youtube:voAu0YRWUNE",
            "花になって - Be a flower",
            artist,
            90_000
        )
    ));
}

#[test]
fn youtube_fallback_accepts_official_audio_but_rejects_wrong_recordings() {
    let original = track(SPOTIFY, "Song", "Artist", 200000);
    for (title, artist) in [
        ("Song", "Artist - Topic"),
        ("Artist - Song (Official Audio)", "ArtistVEVO"),
    ] {
        assert!(youtube_recording(
            &original,
            &track("youtube:dQw4w9WgXcQ", title, artist, 200001)
        ));
    }
    for (title, artist, duration) in [
        ("Song (Live)", "Artist", 200000),
        ("Song", "Cover Artist", 200000),
        ("Song", "Artist", 0),
        ("Song", "Artist", 220000),
    ] {
        assert!(!youtube_recording(
            &original,
            &track("youtube:dQw4w9WgXcQ", title, artist, duration)
        ));
    }
}

#[tokio::test]
async fn missing_volta_recording_falls_back_to_youtube_using_cached_spotify_metadata() {
    let (_spotify, volta, resolver) = setup(None).await;
    Mock::given(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results":{"tracks":[]}})))
        .expect(1)
        .mount(&volta)
        .await;
    let directory = tempfile::tempdir().unwrap();
    let tools = crate::youtube::tests::fixture(
        directory.path(),
        &format!(
            "[Console]::WriteLine('{}')",
            json!({"id":"dQw4w9WgXcQ","title":"Song","channel":"Artist - Topic","duration":200,"acodec":"opus","asr":48000,"url":"https://rr1.googlevideo.com/audio"})
        ),
    );
    let resolver = resolver.with_youtube(Some(tools));
    let audio = resolver.audio(SPOTIFY).await.unwrap();
    assert_eq!(audio.source, "YouTube");
    assert_eq!(audio.quality.codec, Some(crate::audio_quality::Codec::Opus));
    assert_eq!(audio.quality.sample_rate, Some(48000));
    assert!(!audio.headers.contains("Cookie"));
    assert!(!audio.headers.contains("Authorization"));
    assert!(audio.fresh());
}

fn track(id: &str, name: &str, artist: &str, duration: u32) -> Track {
    Track {
        id: id.into(),
        name: name.into(),
        artists: artist.into(),
        duration_ms: duration,
        playable: true,
        ..Track::default()
    }
}
async fn setup(isrc: Option<&str>) -> (MockServer, MockServer, Resolver) {
    let spotify = MockServer::start().await;
    let volta = MockServer::start().await;
    Mock::given(path(format!("/tracks/{SPOTIFY}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id":SPOTIFY,"name":"Song","artists":[{"name":"Artist"}],
            "duration_ms":200000,"external_ids":{"isrc":isrc}
        })))
        .expect(1)
        .mount(&spotify)
        .await;
    Mock::given(path("/api/v1/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token":"volta_at_test","scope":"catalog:read stream:read","expires_in":3600})))
        .expect(1).mount(&volta).await;
    let client = Client::new(
        crate::volta::Credentials {
            client_id: "volta_id_test".into(),
            client_secret: "volta_sk_test".into(),
        },
        &format!("{}/api/v1", volta.uri()),
    )
    .unwrap();
    let resolver = Resolver::new(client, Some(Catalog::mock(&spotify.uri())));
    (spotify, volta, resolver)
}

#[tokio::test]
async fn isrc_mapping_is_shared_without_changing_spotify_track_identity() {
    let (spotify, volta, resolver) = setup(Some(ISRC)).await;
    Mock::given(path(format!("/api/v1/track/{ISRC}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":ISRC,"title":"Translated title","artist":"Translated artist","duration_ms":200010})))
        .expect(1).mount(&volta).await;
    for shared in [resolver.clone(), resolver.clone()] {
        assert_eq!(
            shared.recording(SPOTIFY).await.unwrap(),
            format!("volta:{ISRC}")
        );
    }
    assert_eq!(spotify.received_requests().await.unwrap().len(), 1);
    assert!(resolver.accepts(SPOTIFY));
    assert!(!resolver.accepts("youtube:dQw4w9WgXcQ"));
}

#[tokio::test]
async fn spotify_identity_resolves_to_lossless_audio_without_spotify_streaming() {
    let (_spotify, volta, resolver) = setup(Some(ISRC)).await;
    Mock::given(path(format!("/api/v1/track/{ISRC}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"id":ISRC,"title":"Song","artist":"Artist","duration_ms":200000}),
        ))
        .expect(1)
        .mount(&volta)
        .await;
    Mock::given(path("/api/v1/stream"))
        .and(query_param("track_id",ISRC)).and(query_param("quality","lossless_hq"))
        .respond_with(ResponseTemplate::new(200).insert_header("set-cookie","session=bound; HttpOnly")
            .set_body_json(json!({"stream_url":"/api/v1/app_stream_relay?sig=temporary","sample_rate":96000})))
        .expect(1).mount(&volta).await;
    let audio = resolver.audio(SPOTIFY).await.unwrap();
    assert_eq!(audio.quality.sample_rate, Some(96000));
    assert!(audio.headers.contains("Cookie: session=bound"));
    assert!(!audio.headers.contains("Authorization"));
}

#[tokio::test]
async fn missing_isrc_searches_metadata_and_rejects_live_recordings() {
    let (_spotify, volta, resolver) = setup(None).await;
    Mock::given(path("/api/v1/search"))
        .and(query_param("q", "Song Artist"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"results":{"tracks":[
                {"id":"live","title":"Song (Live)","artist":"Artist","duration_ms":200000},
                {"id":ISRC,"title":"Song","artist":"Artist","duration_ms":200010}
            ]}})),
        )
        .expect(1)
        .mount(&volta)
        .await;
    assert_eq!(
        resolver.recording(SPOTIFY).await.unwrap(),
        format!("volta:{ISRC}")
    );
}

#[tokio::test]
async fn denied_volta_lookup_does_not_trigger_search_or_an_audio_request() {
    let (_spotify, volta, resolver) = setup(Some(ISRC)).await;
    Mock::given(path(format!("/api/v1/track/{ISRC}")))
        .respond_with(ResponseTemplate::new(403).set_body_string("secret-upstream-body"))
        .expect(1)
        .mount(&volta)
        .await;
    let error = resolver.audio(SPOTIFY).await.err().unwrap();
    assert!(ServiceFailure::is(&error, FailureKind::AccessRestricted));
    assert!(!error.to_string().contains("secret-upstream-body"));
    assert_eq!(volta.received_requests().await.unwrap().len(), 2);
}

#[test]
fn metadata_fallback_rejects_wrong_artists_versions_and_ambiguous_matches() {
    let original = track(SPOTIFY, "Song", "Artist", 200000);
    for candidate in [
        track("volta:other", "Song", "Cover Artist", 200000),
        track("volta:live", "Song (Live)", "Artist", 200000),
        track("volta:remix", "Song - Remix", "Artist", 200000),
        track("volta:edit", "Song", "Artist", 180000),
        track("volta:unknown", "Song", "Artist", 0),
    ] {
        assert!(select(&original, &[candidate]).is_err());
    }
    assert!(
        select(
            &original,
            &[
                track("volta:one", "Song", "Artist", 200000),
                track("volta:two", "Song", "Artist", 200000)
            ]
        )
        .is_err()
    );
    let mut same_album = track("volta:one", "Song", "Artist", 200000);
    same_album.album = Some("Original album".into());
    let mut original = original;
    original.album = Some("Original album".into());
    assert_eq!(
        select(
            &original,
            &[same_album, track("volta:two", "Song", "Artist", 200000)]
        )
        .unwrap(),
        "volta:one"
    );
}

#[test]
fn recording_codes_cannot_inject_urls_or_query_parameters() {
    assert_eq!(isrc("jpw462364925").as_deref(), Some(ISRC));
    for value in [
        "../oauth",
        "JPW462364925?x=y",
        "ABCDEFGHIJKL",
        "123456789012",
        "",
    ] {
        assert!(isrc(value).is_none());
    }
}
