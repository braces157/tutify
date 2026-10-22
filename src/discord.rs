//! Discord Rich Presence integration via local Windows IPC named pipe.
use crate::model::{PlaybackState as State, Track};
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
const TUITIFY_URL: &str = "https://github.com/braces157/tutify";
mod metadata;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub track: Option<Track>,
    pub state: State,
    pub position_ms: u32,
    pub quality: Option<crate::audio_quality::AudioQuality>,
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
        self.current_at(Instant::now())
    }

    fn current_at(&self, now: Instant) -> Snapshot {
        let mut snapshot = self.snapshot.clone();
        if snapshot.state == State::Playing {
            let elapsed = now
                .saturating_duration_since(self.captured)
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
    quality: Option<crate::audio_quality::AudioQuality>,
}

impl DiscordPresence {
    pub fn spawn(config: &crate::storage::Config, catalog: &crate::catalog::Catalog) -> Self {
        let mut presence = Self {
            tx: None,
            task: None,
            last_sent: None,
        };
        if !config.discord_rpc {
            return presence;
        }
        let client_id = config
            .discord_client_id
            .clone()
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
        let metadata = metadata::Metadata::new(config, catalog);
        presence.task = Some(tokio::spawn(worker(rx, client_id, metadata)));
        presence
    }

    pub async fn close(&mut self) {
        self.tx.take();
        if let Some(mut task) = self.task.take()
            && tokio::time::timeout(Duration::from_millis(750), &mut task)
                .await
                .is_err()
        {
            task.abort();
            let _ = task.await;
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
            quality: snapshot.quality,
        };
        let should_send = self.last_sent.as_ref().is_none_or(|last| {
            last.track != current.track
                || last.quality != current.quality
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

fn quality_state(artists: &str, quality: Option<crate::audio_quality::AudioQuality>) -> String {
    let Some(quality) = quality else {
        return display_text(artists);
    };
    let quality = quality.label();
    let mut artist = display_text(artists);
    let available = 128usize.saturating_sub(quality.len() + " • ".len());
    while artist.len() > available {
        artist.pop();
    }
    if artist.trim().is_empty() {
        quality
    } else {
        format!("{artist} • {quality}")
    }
}

// Use the metadata's namespace, which may differ from the audio provider.
fn entity_url(id: &str, kind: &str) -> Option<String> {
    if crate::model::valid_id(id) {
        Some(format!("https://open.spotify.com/{kind}/{id}"))
    } else if let Some(key) = crate::deezer::key(id) {
        Some(format!("https://www.deezer.com/{kind}/{key}"))
    } else {
        let key = crate::youtube::music::entity_key(id, kind)?;
        let url = format!("https://music.youtube.com/browse/{key}");
        (url.len() <= 256).then_some(url)
    }
}

fn song_link(track: &Track) -> Option<(&'static str, String)> {
    let label = if crate::youtube::video_key(&track.id).is_some() {
        "Play on YouTube"
    } else if crate::deezer::key(&track.id).is_some() {
        "View on Deezer"
    } else if crate::volta::key(&track.id).is_some() {
        // Volta IDs have no known public song URL. Link to a song search
        // instead of sending listeners to the provider's generic home page.
        let mut query = format!(
            "{} {}",
            display_text(&track.name).trim(),
            display_text(&track.artists).trim()
        );
        let mut url = url::Url::parse("https://open.spotify.com/search/").ok()?;
        loop {
            url.path_segments_mut().ok()?.pop().push(query.trim());
            if url.as_str().len() <= 256 {
                return Some(("Find song on Spotify", url.into()));
            }
            query.pop()?;
        }
    } else {
        "Play on Spotify"
    };
    Some((label, crate::model::track_url(&track.id)?))
}

pub fn build_activity(
    snapshot: &Snapshot,
    now_sec: i64,
    override_art: Option<&str>,
) -> Option<Value> {
    let track = snapshot.track.as_ref()?;
    if !matches!(snapshot.state, State::Playing | State::Paused)
        || !crate::model::valid_track_id(&track.id)
    {
        return None;
    }

    let is_playing = snapshot.state == State::Playing;
    let (song_label, song_url) = song_link(track)?;
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
        "large_url": track.album_id.as_deref()
            .and_then(|id| entity_url(id, "album"))
            .unwrap_or_else(|| song_url.clone()),
    });

    if let (Some(si), Some(st)) = (small_image, small_text) {
        assets["small_image"] = json!(si);
        assets["small_text"] = json!(st);
        assets["small_url"] = json!(TUITIFY_URL);
    }

    let artists = if is_playing {
        track.artists.clone()
    } else {
        format!("Paused • {}", track.artists)
    };
    let mut activity = json!({
        "name": "Tuitify",
        "type": 2, // 2 = Listening ("Listening to Tuitify")
        "status_display_type": if is_playing { 2 } else { 0 },
        "details": display_text(&track.name),
        "details_url": song_url,
        "state": quality_state(&artists, snapshot.quality),
        "assets": assets,
        "buttons": [
            {
                "label": song_label,
                "url": song_url
            },
            {
                "label": "Get Tuitify",
                "url": TUITIFY_URL
            }
        ]
    });

    if let Some(url) = track
        .artist_ids
        .first()
        .and_then(|id| entity_url(id, "artist"))
    {
        activity["state_url"] = json!(url);
    }

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

async fn worker(
    mut rx: watch::Receiver<Option<Update>>,
    client_id: String,
    metadata: metadata::Metadata,
) {
    worker_with_connector(
        &mut rx,
        client_id,
        connect_pipe,
        Duration::from_secs(4),
        metadata,
    )
    .await;
}

async fn worker_with_connector<F, Fut>(
    rx: &mut watch::Receiver<Option<Update>>,
    client_id: String,
    mut connect: F,
    interval: Duration,
    mut metadata: metadata::Metadata,
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
                    if let Ok(mut candidate) = connect().await
                        && handshake(&mut candidate, &client_id).await.is_ok() { pipe = Some(candidate); }
                }
                let Some(p) = &mut pipe else { continue; };
                let snapshot = update.current();
                let resolved = if let Some(track) = &snapshot.track
                    && matches!(snapshot.state, State::Playing | State::Paused) {
                    Some((track.clone(), metadata.resolve(track).await))
                } else { None };
                if let Some((_, track)) = &resolved
                    && track.album_art_url.as_deref().is_none_or(|url| !valid_art(url))
                    && crate::model::valid_id(&track.id) && !art_cache.contains_key(&track.id) {
                        let art = if let Some(client) = &http_client { fetch_album_art(client, &track.id).await } else { None };
                        if art_cache.len() >= 256 { art_cache.clear(); }
                        art_cache.insert(track.id.clone(), art);
                    }
                // A skip/pause can arrive during a metadata/artwork request: always send
                // the newest state, never a buffered song that has already ended.
                let Some(update) = rx.borrow_and_update().clone() else { continue; };
                if rx.has_changed().is_err() { break; }
                let mut snapshot = update.current();
                if let Some((original, track)) = resolved
                    && snapshot.track.as_ref() == Some(&original) {
                        snapshot.track = Some(track);
                    }
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
    fn audio_quality_updates_presence_and_fits_with_long_unicode_artists() {
        let quality = crate::audio_quality::AudioQuality::volta(
            &json!({"format":"flac","bit_depth":24,"sample_rate":96000}),
        );
        let (tx, mut rx) = watch::channel(None);
        let mut presence = DiscordPresence {
            tx: Some(tx),
            task: None,
            last_sent: None,
        };
        let mut snapshot = Snapshot {
            track: Some(sample_track()),
            state: State::Paused,
            position_ms: 0,
            quality: None,
        };
        presence.update(snapshot.clone());
        rx.borrow_and_update();
        snapshot.quality = Some(quality);
        presence.update(snapshot.clone());
        assert!(rx.has_changed().unwrap());
        let activity = build_activity(&snapshot, 1000, None).unwrap();
        assert_eq!(
            activity["state"],
            "Paused • Rick Astley • FLAC 24-bit / 96 kHz"
        );
        assert!(activity.get("timestamps").is_none());
        snapshot.track.as_mut().unwrap().artists = "音".repeat(100);
        let activity = build_activity(&snapshot, 1000, None).unwrap();
        let state = activity["state"].as_str().unwrap();
        assert!(state.len() <= 128);
        assert!(state.ends_with("FLAC 24-bit / 96 kHz"));
        snapshot.quality = Some(crate::audio_quality::AudioQuality::youtube(
            &json!({"acodec":"opus","asr":48000,"abr":128}),
        ));
        assert!(
            build_activity(&snapshot, 1000, None).unwrap()["state"]
                .as_str()
                .unwrap()
                .ends_with("Opus 128 kbps / 48 kHz")
        );
    }

    #[test]
    fn reconnect_position_advances_only_while_playing() {
        let captured = Instant::now();
        let mut update = Update {
            snapshot: Snapshot {
                track: Some(sample_track()),
                state: State::Playing,
                position_ms: 10_000,
                quality: None,
            },
            captured,
        };
        let later = captured + Duration::from_secs(20);
        assert_eq!(update.current_at(later).position_ms, 30_000);
        update.snapshot.state = State::Paused;
        assert_eq!(update.current_at(later).position_ms, 10_000);
        update.snapshot.state = State::Playing;
        assert_eq!(
            update
                .current_at(captured + Duration::from_secs(500))
                .position_ms,
            sample_track().duration_ms
        );
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
            quality: None,
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
            quality: None,
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
        let mut disabled = DiscordPresence::spawn(
            &crate::storage::Config {
                discord_rpc: false,
                ..Default::default()
            },
            &crate::catalog::Catalog::offline().unwrap(),
        );
        disabled.update(Snapshot {
            track: Some(sample_track()),
            state: State::Playing,
            position_ms: 0,
            quality: None,
        });
        assert!(disabled.tx.is_none());
        assert!(disabled.task.is_none());
    }

    fn sample_track() -> Track {
        Track {
            music_metadata: false,
            artist_ids: Vec::new(),
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
            album_id: None,
            track_number: None,
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
                position_ms: 20_000,
                quality: Some(crate::audio_quality::AudioQuality::volta(
                    &json!({"format":"flac","bit_depth":24,"sample_rate":96000}),
                )),
            },
            captured: Instant::now(),
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
                metadata::Metadata::default(),
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
                assert_eq!(
                    payload["args"]["activity"]["details_url"],
                    "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT"
                );
                assert_eq!(
                    payload["args"]["activity"]["state"],
                    "Rick Astley • FLAC 24-bit / 96 kHz"
                );
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
            quality: None,
        };
        let now_sec = 1700000000;
        let act = build_activity(&snapshot, now_sec, None).expect("activity exists");

        assert_eq!(act["type"], 2);
        assert_eq!(act["status_display_type"], 2);
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
        assert_eq!(act["buttons"][0]["label"], "Play on Spotify");
        assert_eq!(
            act["buttons"][0]["url"],
            "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT"
        );
        assert_eq!(act["buttons"][1]["label"], "Get Tuitify");
        assert_eq!(act["buttons"][1]["url"], TUITIFY_URL);
    }

    #[test]
    fn test_build_activity_paused_omits_timestamps() {
        let track = sample_track();
        let snapshot = Snapshot {
            track: Some(track),
            state: State::Paused,
            position_ms: 30000,
            quality: None,
        };
        let now_sec = 1700000000;
        let act = build_activity(&snapshot, now_sec, None).expect("activity exists");

        assert_eq!(act["type"], 2);
        assert_eq!(act["status_display_type"], 0);
        assert_eq!(act["details"], "Never Gonna Give You Up");
        assert_eq!(act["state"], "Paused • Rick Astley");
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
            quality: None,
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
        assert_eq!(act["assets"]["large_url"], act["details_url"]);
    }

    #[test]
    fn clickable_song_artist_and_album_links_follow_catalog_namespaces() {
        for (id, artist, album, label, song_url, artist_url, album_url) in [
            (
                "4cOdK2wGLETKBW3PvgPWqT",
                "0gxyHStUsqpMadRV0Di1Qt",
                "6kZDoAmRSvGQvun5LzyZhk",
                "Play on Spotify",
                "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT",
                Some("https://open.spotify.com/artist/0gxyHStUsqpMadRV0Di1Qt"),
                Some("https://open.spotify.com/album/6kZDoAmRSvGQvun5LzyZhk"),
            ),
            (
                "deezer:3135556",
                "deezer:992",
                "deezer:302127",
                "View on Deezer",
                "https://www.deezer.com/track/3135556",
                Some("https://www.deezer.com/artist/992"),
                Some("https://www.deezer.com/album/302127"),
            ),
            (
                "youtube:dQw4w9WgXcQ",
                "youtube:artist:UC_artist123",
                "youtube:album:MPRE_album123",
                "Play on YouTube",
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                Some("https://music.youtube.com/browse/UC_artist123"),
                Some("https://music.youtube.com/browse/MPRE_album123"),
            ),
            (
                "volta:track-123",
                "volta:artist-123",
                "volta:album-123",
                "Find song on Spotify",
                "https://open.spotify.com/search/Never%20Gonna%20Give%20You%20Up%20Rick%20Astley",
                None,
                None,
            ),
        ] {
            let mut track = sample_track();
            track.id = id.into();
            track.artist_ids = vec![artist.into()];
            track.album_id = Some(album.into());
            let mut snapshot = Snapshot {
                track: Some(track),
                state: State::Playing,
                position_ms: 30_000,
                quality: None,
            };
            for state in [State::Playing, State::Paused] {
                snapshot.state = state;
                let activity = build_activity(&snapshot, 1000, None).unwrap();
                assert_eq!(activity["details_url"], song_url);
                assert_eq!(activity["buttons"][0]["label"], label);
                assert_eq!(activity["buttons"][0]["url"], song_url);
                assert_eq!(activity["state_url"].as_str(), artist_url);
                assert_eq!(
                    activity["assets"]["large_url"],
                    album_url.unwrap_or(song_url)
                );
                assert_eq!(activity["assets"]["small_url"], TUITIFY_URL);
            }
        }
    }

    #[test]
    fn links_reject_invalid_metadata_and_bound_encoded_unicode_searches() {
        let mut track = sample_track();
        track.artist_ids = vec!["deezer:1/../../private".into()];
        track.album_id = Some("youtube:album:bad?redirect=private".into());
        let mut snapshot = Snapshot {
            track: Some(track),
            state: State::Playing,
            position_ms: 0,
            quality: None,
        };
        let activity = build_activity(&snapshot, 1000, None).unwrap();
        assert!(activity.get("state_url").is_none());
        assert_eq!(activity["assets"]["large_url"], activity["details_url"]);

        let track = snapshot.track.as_mut().unwrap();
        track.id = "volta:track-123".into();
        track.name = "音 /?#%\n".repeat(100);
        track.artists = "歌手".repeat(100);
        let activity = build_activity(&snapshot, 1000, None).unwrap();
        let link = activity["details_url"].as_str().unwrap();
        assert!(link.len() <= 256);
        let url = url::Url::parse(link).unwrap();
        assert_eq!(url.host_str(), Some("open.spotify.com"));
        assert_eq!(url.path_segments().unwrap().count(), 2);
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
        assert!(!link.contains('\n'));

        snapshot.track.as_mut().unwrap().id = "https://untrusted.example/song".into();
        assert!(build_activity(&snapshot, 1000, None).is_none());
    }

    #[test]
    fn test_build_activity_stopped_or_failed() {
        let now_sec = 1700000000;
        let snapshot_none = Snapshot {
            track: None,
            state: State::Playing,
            position_ms: 0,
            quality: None,
        };
        assert!(build_activity(&snapshot_none, now_sec, None).is_none());

        let snapshot_failed = Snapshot {
            track: Some(sample_track()),
            state: State::Failed,
            position_ms: 0,
            quality: None,
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
