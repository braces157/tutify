use super::*;
use crate::catalog::Catalog;
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

async fn load_command(commands: &mut mpsc::UnboundedReceiver<Command>) -> (String, u32, u64) {
    loop {
        if let Command::Load {
            id,
            position_ms,
            generation,
        } = recv(commands).await
        {
            return (id, position_ms, generation);
        }
    }
}

pub(super) async fn fallback_event(events: &mut mpsc::UnboundedReceiver<Event>, expected: &str) {
    loop {
        match recv(events).await {
            Event::Fallback { generation, source } => {
                assert_eq!(generation, 7);
                assert_eq!(source, expected);
                return;
            }
            Event::Metadata { track, .. } => assert_eq!(track.id, "deezer:1"),
            Event::TrackError { message, .. } | Event::Error(message) => panic!("{message}"),
            _ => (),
        }
    }
}

#[tokio::test]
async fn deezer_retries_youtube_then_spotify_after_volta_lookup_or_stream_outage() {
    for lookup_outage in [true, false] {
        let deezer = MockServer::start().await;
        let volta = MockServer::start().await;
        let spotify = MockServer::start().await;
        let mut song = crate::deezer::tests::item(1, "Song");
        song["isrc"] = json!("USABC2400001");
        Mock::given(path("/track/1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(song))
            .expect(1) // Provider retries share the cached original metadata.
            .mount(&deezer)
            .await;
        Mock::given(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token":"volta_at_test","expires_in":3600,
                "scope":"catalog:read stream:read"
            })))
            .mount(&volta)
            .await;
        Mock::given(path("/track/USABC2400001"))
            .respond_with(if lookup_outage {
                ResponseTemplate::new(503)
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "id":"USABC2400001","name":"Song","artist":"Artist",
                    "duration_ms":200000
                }))
            })
            .expect(1)
            .mount(&volta)
            .await;
        Mock::given(path("/search"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"tracks":{"items":[{
                    "id":"0000000000000000000001","name":"Song",
                    "artists":[{"name":"Artist"}],"duration_ms":200000,
                    "external_ids":{"isrc":"USABC2400001"}
                }]}})),
            )
            .expect(1)
            .mount(&spotify)
            .await;

        let metadata = crate::deezer::Client::mock(&deezer.uri());
        let volta_catalog = Catalog::volta(crate::volta::Client::mock(&volta.uri()))
            .unwrap()
            .with_deezer(metadata.clone());
        let music = crate::youtube::music::Client::mock(
            r#"import json,sys
r=json.load(sys.stdin)
assert r['operation']=='search'
json.dump({'ok':True,'complete':True,'items':[
{'videoId':'aaaaaaaaaaa','title':'Song (Live)','artists':[{'name':'Artist'}],'duration':'3:20'},
{'videoId':'bbbbbbbbbbb','title':'Song','artists':[{'name':'Cover Artist'}],'duration':'3:20'},
{'videoId':'ccccccccccc','title':'Song','artists':[{'name':'Artist'}],'duration':'3:20'}]},sys.stdout)
"#,
            false,
        );
        let youtube_catalog = Catalog::youtube(crate::youtube::Tools::music_fixture(music))
            .unwrap()
            .with_deezer(metadata.clone());
        let spotify_catalog = Catalog::mock(&spotify.uri()).with_deezer(metadata);
        let (primary, mut primary_commands, primary_events) = backend();
        let (youtube, mut youtube_commands, youtube_events) = backend();
        let (spotify, mut spotify_commands, spotify_events) = backend();
        let mut player = primary
            .with_catalog(volta_catalog)
            .with_deezer_fallback(youtube.with_catalog(youtube_catalog), "YouTube")
            .with_deezer_fallback(spotify.with_catalog(spotify_catalog), "Spotify");
        player
            .commands
            .send(Command::Load {
                id: "deezer:1".into(),
                position_ms: 1234,
                generation: 7,
            })
            .unwrap();
        player.commands.send(Command::Pause).unwrap();
        player.commands.send(Command::Seek(5000)).unwrap();
        if !lookup_outage {
            assert_eq!(
                load_command(&mut primary_commands).await,
                ("volta:USABC2400001".into(), 5000, 7)
            );
            assert!(matches!(recv(&mut primary_commands).await, Command::Pause));
            primary_events
                .send(Event::TrackError {
                    generation: 7,
                    message: "Volta returned HTTP 503".into(),
                })
                .unwrap();
        }

        fallback_event(&mut player.events, "YouTube").await;
        assert_eq!(
            load_command(&mut youtube_commands).await,
            ("youtube:ccccccccccc".into(), 5000, 7)
        );
        assert!(matches!(recv(&mut youtube_commands).await, Command::Pause));
        youtube_events
            .send(Event::TrackError {
                generation: 7,
                message: "YouTube audio unavailable".into(),
            })
            .unwrap();
        fallback_event(&mut player.events, "Spotify").await;
        assert_eq!(
            load_command(&mut spotify_commands).await,
            ("0000000000000000000001".into(), 5000, 7)
        );
        assert!(matches!(recv(&mut spotify_commands).await, Command::Pause));
        primary_events.send(Event::Completed(7)).unwrap();
        youtube_events.send(Event::Completed(7)).unwrap();
        spotify_events
            .send(Event::Metadata {
                generation: 7,
                track: crate::model::Track::unknown("0000000000000000000001"),
            })
            .unwrap();
        spotify_events
            .send(Event::Paused {
                generation: 7,
                position_ms: 5000,
            })
            .unwrap();
        loop {
            match recv(&mut player.events).await {
                Event::Metadata { track, .. } => {
                    assert_eq!(track.id, "deezer:1");
                    assert_eq!(track.name, "Song");
                }
                Event::Paused {
                    generation: 7,
                    position_ms: 5000,
                } => break,
                other => panic!("Unexpected event: {other:?}"),
            }
        }
        assert_eq!(volta.received_requests().await.unwrap().len(), 2);
    }
}
