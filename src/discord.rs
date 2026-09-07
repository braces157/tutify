//! Discord Rich Presence integration via local Windows IPC named pipe.
use crate::{app::State, model::Track};
use serde_json::{Value, json};
use std::{
    io,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient},
    sync::watch,
};

pub const DEFAULT_CLIENT_ID: &str = "1522058393898975252";
pub const TUITIFY_LOGO_URL: &str = "https://raw.githubusercontent.com/braces157/tutify/78c5fbb9eed0f0a6d17bb390c3fab086fe229340/docs/assets/brand/tuitify-discord-minimal-v2-512.png";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub track: Option<Track>,
    pub state: State,
    pub position_ms: u32,
}

// Watch retains only the newest state while Discord is slow or unavailable.
pub struct DiscordPresence {
    tx: Option<watch::Sender<Option<Update>>>,
    task: Option<tokio::task::JoinHandle<()>>,
    last_sent: Option<SentState>,
}

#[derive(Clone, Debug)]
struct Update {
    snapshot: Snapshot,
    captured: Instant,
}
impl Update {
    fn current(&self) -> Snapshot {
        let mut snapshot = self.snapshot.clone();
        if snapshot.state == State::Playing {
            let elapsed = self
                .captured
                .elapsed()
                .as_millis()
                .min(u128::from(u32::MAX)) as u32;
            snapshot.position_ms = snapshot.position_ms.saturating_add(elapsed);
            if let Some(track) = &snapshot.track {
                snapshot.position_ms = snapshot.position_ms.min(track.duration_ms);
            }
        }
        snapshot
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SentState {
    track: Option<Track>,
    state: State,
    estimated_start_sec: Option<i64>,
}

impl DiscordPresence {
    pub fn spawn(enabled: bool, custom_client_id: Option<String>) -> Self {
        let mut presence = Self {
            tx: None,
            task: None,
            last_sent: None,
        };
        if !enabled {
            return presence;
        }
        let client_id = custom_client_id
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("TUITIFY_DISCORD_APP_ID").ok())
            .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string());
        let client_id = client_id.trim().to_string();
        if client_id.is_empty()
            || client_id.len() > 20
            || !client_id.bytes().all(|b| b.is_ascii_digit())
        {
            log::warn!("Discord application ID is invalid; presence disabled");
            return presence;
        }
        let (tx, rx) = watch::channel(None);
        presence.tx = Some(tx);
        presence.task = Some(tokio::spawn(worker(rx, client_id)));
        presence
    }

    pub async fn close(&mut self) {
        self.tx.take();
        if let Some(mut task) = self.task.take() {
            if tokio::time::timeout(Duration::from_millis(750), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
    }

    pub fn update(&mut self, snapshot: Snapshot) {
        let Some(tx) = &self.tx else { return };
        let estimated_start_sec = (snapshot.state == State::Playing && snapshot.track.is_some())
            .then(|| now_sec() - i64::from(snapshot.position_ms / 1000));
        let current = SentState {
            track: snapshot.track.clone(),
            state: snapshot.state,
            estimated_start_sec,
        };
        let should_send = self.last_sent.as_ref().is_none_or(|last| {
            last.track != current.track
                || last.state != current.state
                || match (last.estimated_start_sec, current.estimated_start_sec) {
                    (Some(a), Some(b)) => (a - b).abs() >= 3,
                    (a, b) => a != b,
                }
        });
        if should_send {
            tx.send_replace(Some(Update {
                snapshot,
                captured: Instant::now(),
            }));
            self.last_sent = Some(current);
        }
    }
}

impl Drop for DiscordPresence {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn now_sec() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub async fn fetch_album_art(client: &reqwest::Client, track_id: &str) -> Option<String> {
    if !crate::model::valid_id(track_id) {
        return None;
    }
    let url =
        format!("https://open.spotify.com/oembed?url=https://open.spotify.com/track/{track_id}");
    let mut resp = client
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        if body.len() + chunk.len() > 128 * 1024 {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    let val: Value = serde_json::from_slice(&body).ok()?;
    let mut thumb = val["thumbnail_url"].as_str()?.to_string();
    if thumb.contains("00001e02") {
        thumb = thumb.replace("00001e02", "0000b273");
    }
    valid_art(&thumb).then_some(thumb)
}

fn valid_art(value: &str) -> bool {
    value.len() <= 256
        && url::Url::parse(value).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
}

// Discord limits display fields to 128 bytes, with a minimum of two characters.
fn display_text(value: &str) -> String {
    let mut text = String::new();
    for ch in value.chars().filter(|ch| !ch.is_control()) {
        if text.len() + ch.len_utf8() > 128 {
            break;
        }
        text.push(ch);
    }
    while text.chars().count() < 2 {
        text.push(' ');
    }
    text
}

pub fn build_activity(
    snapshot: &Snapshot,
    now_sec: i64,
    override_art: Option<&str>,
) -> Option<Value> {
    let track = snapshot.track.as_ref()?;
    if !matches!(snapshot.state, State::Playing | State::Paused)
        || !crate::model::valid_id(&track.id)
    {
        return None;
    }

    let is_playing = snapshot.state == State::Playing;
    let art_url = override_art
        .filter(|url| valid_art(url))
        .or_else(|| track.album_art_url.as_deref().filter(|url| valid_art(url)));

    let (large_image, small_image, small_text) = if let Some(art) = art_url {
        (
            art,
            Some(TUITIFY_LOGO_URL),
            Some(if is_playing {
                "Playing on Tuitify"
            } else {
                "Paused on Tuitify"
            }),
        )
    } else {
        (TUITIFY_LOGO_URL, None, None)
    };

    let large_text = if let Some(album) = &track.album {
        format!("{album} • Tuitify")
    } else {
        format!("{} • Tuitify", track.name)
    };

    let mut assets = json!({
        "large_image": large_image,
        "large_text": display_text(&large_text),
    });

    if let (Some(si), Some(st)) = (small_image, small_text) {
        assets["small_image"] = json!(si);
        assets["small_text"] = json!(st);
    }

    let mut activity = json!({
        "name": "Tuitify",
        "type": 2, // 2 = Listening ("Listening to Tuitify")
        "details": display_text(&track.name),
        "state": display_text(&track.artists),
        "assets": assets,
        "buttons": [
            {
                "label": "Listen to Tuitify",
                "url": "https://github.com/braces157/tutify"
            },
            {
                "label": "Play on Spotify",
                "url": format!("https://open.spotify.com/track/{}", track.id)
            }
        ]
    });

    if is_playing && track.duration_ms > 0 {
        let start_sec = now_sec - (snapshot.position_ms.min(track.duration_ms) / 1000) as i64;
        let end_sec = start_sec + (track.duration_ms / 1000) as i64;
        if let Some(obj) = activity.as_object_mut() {
            obj.insert(
                "timestamps".to_string(),
                json!({
                    "start": start_sec,
                    "end": end_sec,
                }),
            );
        }
    }

    Some(activity)
}

const MAX_PACKET: usize = 64 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(2);

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

async fn bounded<T>(future: impl std::future::Future<Output = io::Result<T>>) -> io::Result<T> {
    tokio::time::timeout(IO_TIMEOUT, future)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Discord IPC timed out"))?
}

async fn send_packet<W: AsyncWrite + Unpin>(
    pipe: &mut W,
    opcode: u32,
    payload: &str,
) -> io::Result<()> {
    if payload.len() > MAX_PACKET {
        return Err(invalid("Discord packet too large"));
    }
    let mut buf = Vec::with_capacity(8 + payload.len());
    buf.extend_from_slice(&opcode.to_le_bytes());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload.as_bytes());
    pipe.write_all(&buf).await?;
    pipe.flush().await
}

async fn read_packet<R: AsyncRead + Unpin>(pipe: &mut R) -> io::Result<(u32, String)> {
    let mut header = [0u8; 8];
    pipe.read_exact(&mut header).await?;
    let opcode = u32::from_le_bytes(header[0..4].try_into().unwrap());
    let len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    if len > MAX_PACKET {
        return Err(invalid("Discord packet too large"));
    }
    let mut body = vec![0u8; len];
    pipe.read_exact(&mut body).await?;
    let body = String::from_utf8(body).map_err(|_| invalid("Invalid Discord UTF-8"))?;
    Ok((opcode, body))
}

async fn connect_pipe() -> io::Result<NamedPipeClient> {
    for i in 0..10 {
        let pipe_name = format!(r"\\.\pipe\discord-ipc-{i}");
        if let Ok(client) = ClientOptions::new().open(&pipe_name) {
            return Ok(client);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Discord is not running",
    ))
}

// Process ping/dispatch frames until the expected response arrives. The caller
// bounds the entire exchange, including a peer that sends only unrelated frames.
async fn response<S: AsyncRead + AsyncWrite + Unpin>(
    pipe: &mut S,
    nonce: Option<u64>,
) -> io::Result<()> {
    loop {
        let (opcode, body) = read_packet(pipe).await?;
        match opcode {
            3 => {
                send_packet(pipe, 4, &body).await?;
            }
            4 => (),
            2 => return Err(invalid("Discord closed the connection")),
            1 => {
                let value: Value =
                    serde_json::from_str(&body).map_err(|_| invalid("Invalid Discord JSON"))?;
                if value["evt"] == "ERROR" {
                    return Err(invalid("Discord rejected the activity or application"));
                }
                match nonce {
                    None if value["cmd"] == "DISPATCH" && value["evt"] == "READY" => return Ok(()),
                    Some(nonce)
                        if value["cmd"] == "SET_ACTIVITY"
                            && value["nonce"].as_str().and_then(|s| s.parse::<u64>().ok())
                                == Some(nonce) =>
                    {
                        return Ok(());
                    }
                    _ => (),
                }
            }
            _ => return Err(invalid("Unexpected Discord opcode")),
        }
    }
}

async fn handshake<S: AsyncRead + AsyncWrite + Unpin>(
    pipe: &mut S,
    client_id: &str,
) -> io::Result<()> {
    bounded(async {
        send_packet(
            pipe,
            0,
            &json!({"v": 1, "client_id": client_id}).to_string(),
        )
        .await?;
        response(pipe, None).await
    })
    .await
}

async fn send_activity<S: AsyncRead + AsyncWrite + Unpin>(
    pipe: &mut S,
    activity: Option<Value>,
    nonce: u64,
) -> io::Result<()> {
    bounded(async {
        let payload = json!({"cmd": "SET_ACTIVITY", "args": {
            "pid": std::process::id(), "activity": activity,
        }, "nonce": nonce.to_string()})
        .to_string();
        send_packet(pipe, 1, &payload).await?;
        response(pipe, Some(nonce)).await
    })
    .await
}

async fn worker(mut rx: watch::Receiver<Option<Update>>, client_id: String) {
    worker_with_connector(&mut rx, client_id, connect_pipe, Duration::from_secs(4)).await;
}

async fn worker_with_connector<F, Fut>(
    rx: &mut watch::Receiver<Option<Update>>,
    client_id: String,
    mut connect: F,
    interval: Duration,
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = io::Result<NamedPipeClient>>,
{
    let mut pipe: Option<NamedPipeClient> = None;
    let mut nonce = 0u64;
    // At most five activity writes per 20 seconds, including rapid seeks/skips.
    // Refreshes also detect Discord restarts while the song remains unchanged.
    let mut tick = tokio::time::interval(interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let http_client = reqwest::Client::builder()
        .timeout(IO_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok();
    let mut art_cache = std::collections::HashMap::<String, Option<String>>::new();
    loop {
        tokio::select! {
            changed = rx.changed() => { if changed.is_err() { break; } }
            _ = tick.tick() => {
                let Some(update) = rx.borrow_and_update().clone() else { continue; };
                if pipe.is_none() {
                    if !matches!(update.snapshot.state, State::Playing | State::Paused) || update.snapshot.track.is_none() { continue; }
                    if let Ok(mut candidate) = connect().await {
                        if handshake(&mut candidate, &client_id).await.is_ok() { pipe = Some(candidate); }
                    }
                }
                let Some(p) = &mut pipe else { continue; };
                let snapshot = update.current();
                if let Some(track) = &snapshot.track {
                    if track.album_art_url.is_none() && !art_cache.contains_key(&track.id) {
                        let art = if let Some(client) = &http_client { fetch_album_art(client, &track.id).await } else { None };
                        if art_cache.len() >= 256 { art_cache.clear(); }
                        art_cache.insert(track.id.clone(), art);
                    }
                }
                // A skip/pause can arrive during an artwork request: always send
                // the newest state, never a buffered song that has already ended.
                let Some(update) = rx.borrow_and_update().clone() else { continue; };
                if rx.has_changed().is_err() { break; }
                let snapshot = update.current();
                let art = snapshot.track.as_ref().and_then(|t| art_cache.get(&t.id)).and_then(|v| v.as_deref());
                nonce = nonce.wrapping_add(1);
                if send_activity(p, build_activity(&snapshot, now_sec(), art), nonce).await.is_err() {
                    pipe = None;
                }
                tick.reset();
            }
        }
    }
    if let Some(mut pipe) = pipe {
        let _ = tokio::time::timeout(
            Duration::from_millis(500),
            send_activity(&mut pipe, None, nonce.wrapping_add(1)),
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_position_advances_only_while_playing() {
        let mut update = Update {
            snapshot: Snapshot {
                track: Some(sample_track()),
                state: State::Playing,
                position_ms: 10_000,
            },
            captured: Instant::now() - Duration::from_secs(20),
        };
        assert!((30_000..31_000).contains(&update.current().position_ms));
        update.snapshot.state = State::Paused;
        assert_eq!(update.current().position_ms, 10_000);
        update.snapshot.state = State::Playing;
        update.captured = Instant::now() - Duration::from_secs(500);
        assert_eq!(update.current().position_ms, sample_track().duration_ms);
    }

    #[test]
    fn latest_update_includes_metadata_changes_and_clearing() {
        let (tx, rx) = watch::channel(None);
        let mut presence = DiscordPresence {
            tx: Some(tx),
            task: None,
            last_sent: None,
        };
        let mut snapshot = Snapshot {
            track: Some(sample_track()),
            state: State::Paused,
            position_ms: 0,
        };
        presence.update(snapshot.clone());
        snapshot.track.as_mut().unwrap().name = "Corrected title".into();
        presence.update(snapshot.clone());
        assert_eq!(
            rx.borrow()
                .as_ref()
                .unwrap()
                .snapshot
                .track
                .as_ref()
                .unwrap()
                .name,
            "Corrected title"
        );
        for position in 0..100 {
            snapshot.state = State::Playing;
            snapshot.position_ms = position * 5000;
            presence.update(snapshot.clone());
        }
        snapshot.track = None;
        presence.update(snapshot);
        assert!(rx.borrow().as_ref().unwrap().snapshot.track.is_none());
    }

    #[test]
    fn payload_limits_unicode_and_rejects_invalid_art_and_loading() {
        let mut track = sample_track();
        track.name = "音".repeat(100);
        track.artists = "X".into();
        track.album_art_url = Some("file:///private/image.png".into());
        let mut snapshot = Snapshot {
            track: Some(track),
            state: State::Playing,
            position_ms: u32::MAX,
        };
        let activity = build_activity(&snapshot, 1700000000, None).unwrap();
        assert!(activity["details"].as_str().unwrap().len() <= 128);
        assert_eq!(activity["state"], "X ");
        assert_eq!(activity["assets"]["large_image"], TUITIFY_LOGO_URL);
        assert_eq!(activity["timestamps"]["end"], 1700000000);
        snapshot.state = State::Loading;
        assert!(build_activity(&snapshot, 1700000000, None).is_none());
    }

    #[tokio::test]
    async fn protocol_handles_ping_unrelated_responses_and_nonce() {
        let (mut client, mut server) = tokio::io::duplex(4096);
        let server = tokio::spawn(async move {
            let (opcode, payload) = read_packet(&mut server).await.unwrap();
            assert_eq!(opcode, 0);
            assert_eq!(
                serde_json::from_str::<Value>(&payload).unwrap()["client_id"],
                "123"
            );
            send_packet(&mut server, 1, r#"{"cmd":"DISPATCH","evt":"READY"}"#)
                .await
                .unwrap();
            let (_, request) = read_packet(&mut server).await.unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&request).unwrap()["nonce"],
                "42"
            );
            send_packet(&mut server, 3, "ping").await.unwrap();
            assert_eq!(read_packet(&mut server).await.unwrap(), (4, "ping".into()));
            send_packet(&mut server, 1, r#"{"cmd":"SET_ACTIVITY","nonce":"41"}"#)
                .await
                .unwrap();
            send_packet(&mut server, 1, r#"{"cmd":"SET_ACTIVITY","nonce":"42"}"#)
                .await
                .unwrap();
        });
        handshake(&mut client, "123").await.unwrap();
        send_activity(&mut client, None, 42).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn protocol_rejects_errors_and_oversized_frames() {
        let (mut client, mut server) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move {
            read_packet(&mut server).await.unwrap();
            send_packet(
                &mut server,
                1,
                r#"{"cmd":"SET_ACTIVITY","evt":"ERROR","nonce":"1"}"#,
            )
            .await
            .unwrap();
        });
        assert_eq!(
            send_activity(&mut client, None, 1)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        server_task.await.unwrap();
        let (mut client, mut server) = tokio::io::duplex(16);
        server.write_all(&1u32.to_le_bytes()).await.unwrap();
        server.write_all(&u32::MAX.to_le_bytes()).await.unwrap();
        assert_eq!(
            read_packet(&mut client).await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn unresponsive_peer_times_out_and_shutdown_aborts_stalled_work() {
        let (mut client, _server) = tokio::io::duplex(4096);
        assert_eq!(
            handshake(&mut client, "123").await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        let (tx, _rx) = watch::channel(None);
        let task = tokio::spawn(std::future::pending());
        let abort = task.abort_handle();
        let mut presence = DiscordPresence {
            tx: Some(tx),
            task: Some(task),
            last_sent: None,
        };
        presence.close().await;
        assert!(abort.is_finished());
        let mut disabled = DiscordPresence::spawn(false, None);
        disabled.update(Snapshot {
            track: Some(sample_track()),
            state: State::Playing,
            position_ms: 0,
        });
        assert!(disabled.tx.is_none());
        assert!(disabled.task.is_none());
    }

    fn sample_track() -> Track {
        Track {
            id: "4cOdK2wGLETKBW3PvgPWqT".into(),
            name: "Never Gonna Give You Up".into(),
            artists: "Rick Astley".into(),
            duration_ms: 213000,
            playable: true,
            album: Some("Whenever You Need Somebody".into()),
            album_art_url: Some(
                "https://image-cdn-ak.spotifycdn.com/image/ab67616d0000b273baf89eb11ec7c657805d2da0"
                    .into(),
            ),
        }
    }

    #[tokio::test]
    async fn isolated_windows_pipe_reconnects_and_clears_on_shutdown() {
        use tokio::net::windows::named_pipe::ServerOptions;
        let pipe_name = format!(r"\\.\pipe\tuitify-test-discord-{}", std::process::id());
        let mut first = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)
            .unwrap();
        let mut second = ServerOptions::new().create(&pipe_name).unwrap();
        let (tx, mut rx) = watch::channel(Some(Update {
            snapshot: Snapshot {
                track: Some(sample_track()),
                state: State::Playing,
                position_ms: 10_000,
            },
            captured: Instant::now() - Duration::from_secs(10),
        }));
        let mut task = tokio::spawn(async move {
            worker_with_connector(
                &mut rx,
                "123".into(),
                || {
                    let name = pipe_name.clone();
                    async move { ClientOptions::new().open(&name) }
                },
                Duration::from_millis(30),
            )
            .await;
        });
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            for (index, server) in [&mut first, &mut second].into_iter().enumerate() {
                server.connect().await.unwrap();
                assert_eq!(read_packet(server).await.unwrap().0, 0);
                send_packet(server, 1, r#"{"cmd":"DISPATCH","evt":"READY"}"#)
                    .await
                    .unwrap();
                let (_, payload) = read_packet(server).await.unwrap();
                let payload: Value = serde_json::from_str(&payload).unwrap();
                assert_eq!(payload["args"]["activity"]["details"], sample_track().name);
                let start = payload["args"]["activity"]["timestamps"]["start"]
                    .as_i64()
                    .unwrap();
                assert!((19..=22).contains(&(now_sec() - start)));
                send_packet(
                    server,
                    1,
                    &json!({"cmd":"SET_ACTIVITY", "nonce":payload["nonce"]}).to_string(),
                )
                .await
                .unwrap();
                if index == 0 {
                    server.disconnect().unwrap();
                }
            }
            drop(tx);
            loop {
                let (_, payload) = read_packet(&mut second).await.unwrap();
                let payload: Value = serde_json::from_str(&payload).unwrap();
                send_packet(
                    &mut second,
                    1,
                    &json!({"cmd":"SET_ACTIVITY", "nonce":payload["nonce"]}).to_string(),
                )
                .await
                .unwrap();
                if payload["args"]["activity"].is_null() {
                    break;
                }
            }
            (&mut task).await.unwrap();
        })
        .await;
        if result.is_err() {
            task.abort();
        }
        result.unwrap();
    }

    #[test]
    fn test_build_activity_playing() {
        let track = sample_track();
        let snapshot = Snapshot {
            track: Some(track),
            state: State::Playing,
            position_ms: 30000,
        };
        let now_sec = 1700000000;
        let act = build_activity(&snapshot, now_sec, None).expect("activity exists");

        assert_eq!(act["type"], 2);
        assert_eq!(act["details"], "Never Gonna Give You Up");
        assert_eq!(act["state"], "Rick Astley");
        assert_eq!(
            act["assets"]["large_image"],
            "https://image-cdn-ak.spotifycdn.com/image/ab67616d0000b273baf89eb11ec7c657805d2da0"
        );
        assert_eq!(
            act["assets"]["large_text"],
            "Whenever You Need Somebody • Tuitify"
        );
        assert_eq!(act["assets"]["small_image"], TUITIFY_LOGO_URL);
        assert_eq!(act["assets"]["small_text"], "Playing on Tuitify");
        assert_eq!(act["timestamps"]["start"], now_sec - 30);
        assert_eq!(act["name"], "Tuitify");
        assert_eq!(act["buttons"][0]["label"], "Listen to Tuitify");
        assert_eq!(
            act["buttons"][0]["url"],
            "https://github.com/braces157/tutify"
        );
        assert_eq!(act["buttons"][1]["label"], "Play on Spotify");
        assert_eq!(
            act["buttons"][1]["url"],
            "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT"
        );
    }

    #[test]
    fn test_build_activity_paused_omits_timestamps() {
        let track = sample_track();
        let snapshot = Snapshot {
            track: Some(track),
            state: State::Paused,
            position_ms: 30000,
        };
        let now_sec = 1700000000;
        let act = build_activity(&snapshot, now_sec, None).expect("activity exists");

        assert_eq!(act["type"], 2);
        assert_eq!(act["details"], "Never Gonna Give You Up");
        assert_eq!(act["state"], "Rick Astley");
        assert_eq!(act["assets"]["small_text"], "Paused on Tuitify");
        assert!(act.get("timestamps").is_none());
    }

    #[test]
    fn test_build_activity_fallback_without_album_art() {
        let mut track = sample_track();
        track.album = None;
        track.album_art_url = None;
        let snapshot = Snapshot {
            track: Some(track),
            state: State::Playing,
            position_ms: 10000,
        };
        let now_sec = 1700000000;
        let act = build_activity(&snapshot, now_sec, None).expect("activity exists");

        assert_eq!(act["assets"]["large_image"], TUITIFY_LOGO_URL);
        assert_eq!(
            act["assets"]["large_text"],
            "Never Gonna Give You Up • Tuitify"
        );
        assert!(act["assets"].get("small_image").is_none());
        assert!(act["assets"].get("small_text").is_none());
    }

    #[test]
    fn test_build_activity_stopped_or_failed() {
        let now_sec = 1700000000;
        let snapshot_none = Snapshot {
            track: None,
            state: State::Playing,
            position_ms: 0,
        };
        assert!(build_activity(&snapshot_none, now_sec, None).is_none());

        let snapshot_failed = Snapshot {
            track: Some(sample_track()),
            state: State::Failed,
            position_ms: 0,
        };
        assert!(build_activity(&snapshot_failed, now_sec, None).is_none());
    }

    #[test]
    fn test_tuitify_logo_url_valid() {
        assert!(valid_art(TUITIFY_LOGO_URL));
        assert!(
            TUITIFY_LOGO_URL.starts_with("https://raw.githubusercontent.com/braces157/tutify/")
        );
    }
}
