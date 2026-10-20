use super::*;
use crate::deezer::tests::item;
use serde_json::json;
use std::time::Duration;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

fn backend() -> (
    Playback,
    mpsc::UnboundedReceiver<Command>,
    mpsc::UnboundedSender<Event>,
) {
    let (commands, rx) = mpsc::unbounded_channel();
    let (tx, events) = mpsc::unbounded_channel();
    (
        Playback {
            commands,
            events,
            task: tokio::spawn(std::future::pending()),
        },
        rx,
        tx,
    )
}
async fn recv<T>(rx: &mut mpsc::UnboundedReceiver<T>) -> T {
    tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap()
}
async fn setup(delay: Duration) -> (MockServer, MockServer, Catalog) {
    let primary = MockServer::start().await;
    let native = MockServer::start().await;
    Mock::given(path("/track/1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(delay)
                .set_body_json(item(1, "Song")),
        )
        .expect(1)
        .mount(&primary)
        .await;
    Mock::given(path("/search")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"tracks":{"items":[{
        "id":"0000000000000000000001","name":"Song","artists":[{"name":"Artist"}],"duration_ms":200000
    }]}}))).expect(0..=1).mount(&native).await;
    let catalog =
        Catalog::mock(&native.uri()).with_deezer(crate::deezer::Client::mock(&primary.uri()));
    (primary, native, catalog)
}

#[tokio::test]
async fn loading_keeps_pause_seek_and_catalog_identity_while_resolving() {
    let (_primary, _native, catalog) = setup(Duration::from_millis(80)).await;
    let (inner, mut commands, events) = backend();
    let mut player = spawn(inner, catalog);
    player
        .commands
        .send(Command::Load {
            id: "deezer:1".into(),
            generation: 7,
            position_ms: 1000,
        })
        .unwrap();
    assert!(matches!(recv(&mut commands).await, Command::Stop));
    player.commands.send(Command::Pause).unwrap();
    player.commands.send(Command::Seek(5000)).unwrap();
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Paused { generation: 7, .. }
    ));
    assert!(matches!(
        recv(&mut player.events).await,
        Event::Position {
            position_ms: 5000,
            ..
        }
    ));
    assert!(
        matches!(recv(&mut commands).await,Command::Load {id,generation:7,position_ms:5000} if id=="0000000000000000000001")
    );
    assert!(matches!(recv(&mut commands).await, Command::Pause));
    assert!(
        matches!(recv(&mut player.events).await,Event::Metadata {track,..} if track.id=="deezer:1")
    );
    events
        .send(Event::Metadata {
            generation: 7,
            track: Track::unknown("0000000000000000000001"),
        })
        .unwrap();
    assert!(
        matches!(recv(&mut player.events).await,Event::Metadata {track,..} if track.id=="deezer:1" && track.name=="Song")
    );
}

#[tokio::test]
async fn stop_cancels_resolution_and_late_results_cannot_restart_audio() {
    let (primary, _native, catalog) = setup(Duration::from_millis(150)).await;
    let (inner, mut commands, _events) = backend();
    let player = spawn(inner, catalog);
    player
        .commands
        .send(Command::Load {
            id: "deezer:1".into(),
            generation: 1,
            position_ms: 0,
        })
        .unwrap();
    assert!(matches!(recv(&mut commands).await, Command::Stop));
    for _ in 0..100 {
        if !primary.received_requests().await.unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    player.commands.send(Command::Stop).unwrap();
    assert!(matches!(recv(&mut commands).await, Command::Stop));
    assert!(
        tokio::time::timeout(Duration::from_millis(200), commands.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn preloaded_mapping_is_reused_by_load_without_a_second_lookup() {
    let (_primary, _native, catalog) = setup(Duration::ZERO).await;
    let (inner, mut commands, _events) = backend();
    let player = spawn(inner, catalog);
    player
        .commands
        .send(Command::Preload {
            id: "deezer:1".into(),
        })
        .unwrap();
    assert!(matches!(recv(&mut commands).await, Command::CancelPreload));
    assert!(
        matches!(recv(&mut commands).await,Command::Preload {id} if id=="0000000000000000000001")
    );
    player
        .commands
        .send(Command::Load {
            id: "deezer:1".into(),
            generation: 2,
            position_ms: 0,
        })
        .unwrap();
    assert!(matches!(
        recv(&mut commands).await,
        Command::Load { generation: 2, .. }
    ));
}
