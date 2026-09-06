use crate::auth::TokenManager;
use anyhow::{Context, Result};
use librespot_core::{
    SpotifyUri, authentication::Credentials, config::SessionConfig, session::Session,
};
use librespot_playback::{
    audio_backend::{Sink, SinkError, SinkResult},
    config::PlayerConfig,
    convert::Converter,
    decoder::AudioPacket,
    mixer::{Mixer, MixerConfig, softmixer::SoftMixer},
    player::{Player, PlayerEvent},
};
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum Command {
    Load {
        id: String,
        position_ms: u32,
        generation: u64,
    },
    Pause,
    Resume,
    Seek(u32),
    Volume(u8),
    Stop,
    #[cfg(test)]
    SimulateDisconnect,
}
#[derive(Debug)]
pub enum Event {
    Ready,
    Playing { generation: u64, position_ms: u32 },
    Paused { generation: u64, position_ms: u32 },
    Position { generation: u64, position_ms: u32 },
    Completed(u64),
    Volume(u8),
    Error(String),
    TrackError { generation: u64, message: String },
}

pub struct Playback {
    pub commands: mpsc::UnboundedSender<Command>,
    pub events: mpsc::UnboundedReceiver<Event>,
    task: tokio::task::JoinHandle<()>,
}

impl Playback {
    pub fn spawn(tokens: TokenManager, client_id: String, volume: u8) -> Self {
        Self::spawn_with_visualizer(
            tokens,
            client_id,
            volume,
            crate::visualizer::AudioVisualizer::new(),
        )
    }

    pub fn spawn_with_visualizer(
        tokens: TokenManager,
        client_id: String,
        volume: u8,
        visualizer: Arc<crate::visualizer::AudioVisualizer>,
    ) -> Self {
        let (commands, rx) = mpsc::unbounded_channel();
        let (tx, events) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            if let Err(e) = worker(tokens, client_id, volume, rx, tx.clone(), visualizer).await {
                let _ = tx.send(Event::Error(format!("{e:#}")));
            }
        });
        Self {
            commands,
            events,
            task,
        }
    }
}
impl Drop for Playback {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Engine {
    session: Session,
    player: Arc<Player>,
    mixer: SoftMixer,
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.player.stop();
        self.session.shutdown();
    }
}

struct ConnectingSession(Option<Session>);
impl Drop for ConnectingSession {
    fn drop(&mut self) {
        if let Some(session) = &self.0 {
            session.shutdown();
        }
    }
}

type Connected = (Engine, mpsc::UnboundedReceiver<PlayerEvent>);
type ConnectionFuture = futures_util::future::BoxFuture<'static, Result<Connected>>;

#[derive(Debug)]
struct LoadIntent {
    id: String,
    position_ms: u32,
    generation: u64,
    paused: bool,
}
impl LoadIntent {
    fn control(&mut self, command: &Command) {
        match command {
            Command::Pause => self.paused = true,
            Command::Resume => self.paused = false,
            Command::Seek(position) => self.position_ms = *position,
            _ => (),
        }
    }
}
fn load_intent(engine: &Engine, intent: LoadIntent, pending: &mut VecDeque<u64>) -> Result<()> {
    let uri = SpotifyUri::from_uri(&format!("spotify:track:{}", intent.id))?;
    crate::diagnostics::take();
    pending.push_back(intent.generation);
    engine.player.load(uri, !intent.paused, intent.position_ms);
    Ok(())
}

async fn connect(
    tokens: &TokenManager,
    _client_id: &str,
    volume: u8,
    tx: mpsc::UnboundedSender<Event>,
    visualizer: Arc<crate::visualizer::AudioVisualizer>,
) -> Result<(Engine, mpsc::UnboundedReceiver<PlayerEvent>)> {
    let token = tokens.access().await?;
    let session = Session::new(
        SessionConfig {
            client_id: crate::auth::STREAMING_CLIENT_ID.to_owned(),
            autoplay: Some(false),
            ..SessionConfig::default()
        },
        None,
    );
    let mut guard = ConnectingSession(Some(session.clone()));
    let connection = tokio::time::timeout(
        Duration::from_secs(35),
        session.connect(Credentials::with_access_token(token), false),
    )
    .await;
    if !matches!(&connection, Ok(Ok(()))) {
        session.shutdown();
        anyhow::bail!(
            "Spotify streaming connection failed. Check network and Premium membership; retry with Space. If it persists, run tuitify auth --streaming --force. Librespot may be incompatible with Spotify's current service."
        );
    }
    let mixer = SoftMixer::open(MixerConfig::default())?;
    mixer.set_volume(volume as u16 * 655);
    let player = Player::new(
        PlayerConfig {
            position_update_interval: Some(Duration::from_millis(500)),
            ..PlayerConfig::default()
        },
        session.clone(),
        mixer.get_soft_volume(),
        move || Box::new(WindowsAudio::new(tx.clone(), visualizer.clone())),
    );
    let events = player.get_player_event_channel();
    guard.0.take();
    Ok((
        Engine {
            session,
            player,
            mixer,
        },
        events,
    ))
}

async fn worker(
    tokens: TokenManager,
    client_id: String,
    volume: u8,
    commands: mpsc::UnboundedReceiver<Command>,
    tx: mpsc::UnboundedSender<Event>,
    visualizer: Arc<crate::visualizer::AudioVisualizer>,
) -> Result<()> {
    let event_tx = tx.clone();
    let vis = visualizer.clone();
    worker_with_connector(volume, commands, tx, move |volume| {
        let tokens = tokens.clone();
        let client_id = client_id.clone();
        let tx = event_tx.clone();
        let vis = vis.clone();
        Box::pin(async move { connect(&tokens, &client_id, volume, tx, vis).await })
    })
    .await
}

async fn worker_with_connector<C>(
    mut volume: u8,
    mut commands: mpsc::UnboundedReceiver<Command>,
    tx: mpsc::UnboundedSender<Event>,
    connector: C,
) -> Result<()>
where
    C: Fn(u8) -> ConnectionFuture,
{
    let mut connecting: Option<ConnectionFuture> = None;
    let mut desired: Option<LoadIntent> = None;
    let mut engine: Option<Engine> = None;
    let mut player_events = mpsc::unbounded_channel().1;
    let mut pending = VecDeque::new();
    let mut active: Option<(u64, u64)> = None; // librespot request ID, local generation
    let mut loading_since: Option<Instant> = None;
    let mut health = tokio::time::interval(Duration::from_secs(1));
    let mut playing = false;
    let mut last_progress = Instant::now();
    loop {
        tokio::select! {
            cmd = commands.recv() => {
                let Some(cmd) = cmd else { break; };
                match cmd {
                    Command::Load { id, position_ms, generation } => {
                        if engine.as_ref().is_some_and(|e| e.session.is_invalid()) { engine = None; }
                        let intent = LoadIntent { id, position_ms, generation, paused: false };
                        if engine.is_none() {
                            active = None; playing = false; loading_since = None; pending.clear();
                            desired = Some(intent);
                            if connecting.is_none() { connecting = Some(connector(volume)); }
                            continue;
                        }
                        active = None; playing = false; loading_since = Some(Instant::now());
                        if let Err(e) = load_intent(engine.as_ref().unwrap(), intent, &mut pending) {
                            loading_since = None;
                            let _ = tx.send(Event::TrackError { generation, message: format!("Invalid track: {e}") });
                        }
                    },
                    Command::Volume(v) => { volume = v.min(100); if let Some(e) = &engine { e.mixer.set_volume((volume as u32 * 65535 / 100) as u16); } let _ = tx.send(Event::Volume(volume)); },
                    Command::Stop => { connecting = None; desired = None; active = None; loading_since = None; playing = false; pending.clear(); engine = None; },
                    #[cfg(test)]
                    Command::SimulateDisconnect => { if let Some(e) = &engine { e.session.shutdown(); } },
                    other => {
                        if let Some(intent) = &mut desired { intent.control(&other); }
                        else if let Some(e) = &engine { match other { Command::Pause => e.player.pause(), Command::Resume => e.player.play(), Command::Seek(ms) => e.player.seek(ms), _ => {} } }
                    },
                }
            },
            result = async { connecting.as_mut().unwrap().await }, if connecting.is_some() => {
                connecting = None;
                let Some(intent) = desired.take() else { continue; };
                let generation = intent.generation;
                match result {
                    Ok((e, events)) => {
                        e.mixer.set_volume((volume as u32 * 65535 / 100) as u16);
                        player_events = events; pending.clear(); active = None; playing = false;
                        loading_since = Some(Instant::now());
                        let _ = tx.send(Event::Ready);
                        if let Err(error) = load_intent(&e, intent, &mut pending) {
                            loading_since = None;
                            let _ = tx.send(Event::TrackError { generation, message: format!("Invalid track: {error}") });
                        }
                        engine = Some(e);
                    }
                    Err(error) => { let _ = tx.send(Event::TrackError { generation, message: format!("{error:#}") }); }
                }
            },
            event = player_events.recv(), if engine.is_some() => {
                let Some(event) = event else { engine = None; let _ = tx.send(Event::Error("Audio worker stopped. Check your output device and press Space to retry.".into())); continue; };
                if let PlayerEvent::PlayRequestIdChanged { play_request_id } = event {
                    active = pending.pop_front().map(|generation| (play_request_id, generation)); continue;
                }
                let Some((request, generation)) = active else { continue; };
                // librespot 0.8's helper omits PositionChanged; include it explicitly.
                let event_request = match &event { PlayerEvent::PositionChanged { play_request_id, .. } => Some(*play_request_id), _ => event.get_play_request_id() };
                if event_request.is_some_and(|id| id != request) { continue; }
                let out = match event {
                    PlayerEvent::Playing { position_ms, .. } => { loading_since = None; playing = true; last_progress = Instant::now(); Some(Event::Playing { generation, position_ms }) },
                    PlayerEvent::Paused { position_ms, .. } => { loading_since = None; playing = false; Some(Event::Paused { generation, position_ms }) },
                    PlayerEvent::PositionChanged { position_ms, .. } | PlayerEvent::PositionCorrection { position_ms, .. } | PlayerEvent::Seeked { position_ms, .. } => { last_progress = Instant::now(); Some(Event::Position { generation, position_ms }) },
                    PlayerEvent::EndOfTrack { .. } => { active = None; playing = false; Some(Event::Completed(generation)) },
                    PlayerEvent::Unavailable { .. } => { loading_since = None; active = None; playing = false; Some(Event::TrackError { generation, message: format!("{}. Choose another track or Space to retry; persistent errors may need tuitify auth --streaming --force or a librespot update.", crate::diagnostics::take().unwrap_or_else(|| "Track unavailable or network interrupted".into())) }) },
                    _ => None,
                };
                if let Some(out) = out { let _ = tx.send(out); }
            },
            _ = health.tick() => {
                if engine.as_ref().is_some_and(|e| e.session.is_invalid()) || loading_since.is_some_and(|t| t.elapsed() > Duration::from_secs(45)) || (playing && last_progress.elapsed() > Duration::from_secs(30)) {
                    engine = None; active = None; pending.clear(); loading_since = None; playing = false;
                    let _ = tx.send(Event::Error("Streaming connection timed out or was lost. Check your network, then press Space to reconnect; queue is preserved.".into()));
                }
            }
        }
    }
    Ok(())
}

/// Opens Rodio/CPAL on librespot's audio thread (WASAPI on Windows).
/// No process::exit or unwrap on device failure; buffers have a bounded drain time.
struct WindowsAudio {
    output: Option<(rodio::Sink, rodio::OutputStream)>,
    tx: mpsc::UnboundedSender<Event>,
    visualizer: Arc<crate::visualizer::AudioVisualizer>,
}
impl WindowsAudio {
    fn new(tx: mpsc::UnboundedSender<Event>, visualizer: Arc<crate::visualizer::AudioVisualizer>) -> Self {
        Self { output: None, tx, visualizer }
    }
    fn fail(&self, detail: impl std::fmt::Display) -> SinkError {
        let message = format!(
            "Windows audio failed: {detail}. Select a working default output in Windows Sound settings, then press Space to retry."
        );
        let _ = self.tx.send(Event::Error(message.clone()));
        SinkError::ConnectionRefused(message)
    }
}
impl Sink for WindowsAudio {
    fn start(&mut self) -> SinkResult<()> {
        if self.output.is_none() {
            let (stream, handle) = rodio::OutputStream::try_default().map_err(|e| self.fail(e))?;
            let sink = rodio::Sink::try_new(&handle).map_err(|e| self.fail(e))?;
            self.output = Some((sink, stream));
        }
        self.output.as_ref().unwrap().0.play();
        Ok(())
    }
    fn stop(&mut self) -> SinkResult<()> {
        if let Some((sink, _)) = &self.output {
            let until = Instant::now() + Duration::from_secs(2);
            while !sink.empty() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
            sink.clear();
            sink.pause();
        }
        Ok(())
    }
    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let samples = packet.samples().map_err(|e| self.fail(e))?;
        let samples = converter.f64_to_f32(samples);
        let (sink, _) = self
            .output
            .as_ref()
            .ok_or_else(|| self.fail("output disconnected"))?;
        let buffer = rodio::buffer::SamplesBuffer::new(2, 44_100, samples);
        let source = crate::visualizer::VisualizerSource::new(buffer, self.visualizer.clone());
        sink.append(source);
        let until = Instant::now() + Duration::from_secs(3);
        while sink.len() > 12 {
            if Instant::now() > until {
                return Err(self.fail("output stopped consuming audio"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}

/// Minimal first-stage acceptance tool, before the full UI. All controls share the real worker.
pub async fn probe(tokens: TokenManager, client_id: String, id: String) -> Result<()> {
    use crossterm::event::{Event as Input, EventStream, KeyCode, KeyEventKind};
    use futures_util::StreamExt;
    let mut player = Playback::spawn(tokens, client_id, 35);
    let mut keys = EventStream::new();
    crossterm::terminal::enable_raw_mode()
        .context("Open a Windows Terminal to run the playback probe")?;
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
    let _restore = Restore;
    println!(
        "\r\nClose Spotify desktop. Probe: Space pause/resume, arrows seek, +/- volume, q exit.\r"
    );
    player.commands.send(Command::Load {
        id: id.clone(),
        position_ms: 0,
        generation: 1,
    })?;
    let mut paused = false;
    let mut position: u32 = 0;
    let mut volume: u8 = 35;
    loop {
        tokio::select! {
            event = player.events.recv() => match event {
                Some(Event::Position { position_ms, .. }) => position = position_ms,
                Some(Event::Playing { position_ms, .. }) => { position = position_ms; println!("Playing at {position} ms\r"); },
                Some(Event::Error(e)) | Some(Event::TrackError { message: e, .. }) => { println!("{e}\r"); break; },
                Some(e) => println!("{e:?}\r"), None => break,
            },
            key = keys.next() => if let Some(Ok(Input::Key(key))) = key {
                if key.kind == KeyEventKind::Release { continue; }
                let command = match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char(' ') => { paused = !paused; if paused { Command::Pause } else { Command::Resume } },
                    KeyCode::Left => Command::Seek(position.saturating_sub(10_000)),
                    KeyCode::Right => Command::Seek(position.saturating_add(10_000)),
                    KeyCode::Char('+') | KeyCode::Char('=') => { volume = (volume + 5).min(100); Command::Volume(volume) },
                    KeyCode::Char('-') => { volume = volume.saturating_sub(5); Command::Volume(volume) },
                    _ => continue,
                }; player.commands.send(command)?;
            },
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{catalog::Catalog, storage::Storage};

    async fn expect(player: &mut Playback, predicate: impl Fn(&Event) -> bool) -> Event {
        tokio::time::timeout(Duration::from_secs(55), async {
            loop {
                let event = player.events.recv().await.expect("Playback worker ended");
                if predicate(&event) {
                    return event;
                }
                if let Event::Error(message) | Event::TrackError { message, .. } = event {
                    panic!("Live playback error: {message}");
                }
            }
        })
        .await
        .expect("Live playback event timed out")
    }

    #[tokio::test]
    async fn connecting_accepts_controls_and_stop_cancels_the_future() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let (commands, rx) = mpsc::unbounded_channel();
        let (events, mut out) = mpsc::unbounded_channel();
        let (started, mut ready) = mpsc::unbounded_channel();
        let worker = tokio::spawn(worker_with_connector(40, rx, events, move |_| {
            let guard = Dropped(flag.clone());
            let started = started.clone();
            Box::pin(async move {
                let _guard = guard;
                started.send(()).unwrap();
                std::future::pending::<Result<Connected>>().await
            })
        }));
        commands
            .send(Command::Load {
                id: "0".repeat(22),
                position_ms: 0,
                generation: 1,
            })
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), ready.recv())
            .await
            .unwrap()
            .unwrap();
        commands.send(Command::Pause).unwrap();
        commands.send(Command::Seek(10000)).unwrap();
        commands.send(Command::Volume(23)).unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), out.recv())
                .await
                .unwrap(),
            Some(Event::Volume(23))
        ));
        commands.send(Command::Stop).unwrap();
        commands.send(Command::Volume(24)).unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), out.recv())
                .await
                .unwrap(),
            Some(Event::Volume(24))
        ));
        assert!(cancelled.load(Ordering::SeqCst));
        drop(commands);
        worker.await.unwrap().unwrap();
    }
    #[tokio::test]
    async fn connecting_keeps_only_latest_load_generation() {
        let notify = Arc::new(tokio::sync::Notify::new());
        let gate = notify.clone();
        let (commands, rx) = mpsc::unbounded_channel();
        let (events, mut out) = mpsc::unbounded_channel();
        let worker = tokio::spawn(worker_with_connector(40, rx, events, move |_| {
            let gate = gate.clone();
            Box::pin(async move {
                gate.notified().await;
                anyhow::bail!("delayed connection failure")
            })
        }));
        for generation in [1, 2] {
            commands
                .send(Command::Load {
                    id: "0".repeat(22),
                    position_ms: 0,
                    generation,
                })
                .unwrap();
        }
        commands.send(Command::Volume(25)).unwrap();
        assert!(matches!(out.recv().await, Some(Event::Volume(25))));
        notify.notify_one();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), out.recv())
                .await
                .unwrap(),
            Some(Event::TrackError { generation: 2, .. })
        ));
        drop(commands);
        worker.await.unwrap().unwrap();
    }
    #[test]
    fn pending_load_preserves_pause_and_seek_intent() {
        let mut intent = LoadIntent {
            id: "0".repeat(22),
            generation: 1,
            position_ms: 0,
            paused: false,
        };
        intent.control(&Command::Pause);
        intent.control(&Command::Seek(4321));
        assert!(intent.paused);
        assert_eq!(intent.position_ms, 4321);
        intent.control(&Command::Resume);
        assert!(!intent.paused);
    }
    #[tokio::test]
    #[ignore = "Requires both Spotify browser logins, Premium, Windows audio; plays audible sound"]
    async fn live_streaming_acceptance() {
        crate::diagnostics::init();
        let store = Storage::local().unwrap();
        let _lock = store.lock().unwrap();
        let config = store.config().unwrap();
        let catalog = Catalog::new(TokenManager::load(&config).unwrap()).unwrap();
        let track = catalog.track("4uLU6hMCjMI75M1A2tKUQC").await.unwrap();
        let mut player = Playback::spawn(
            TokenManager::load_streaming().unwrap(),
            config.client_id.clone(),
            20,
        );
        player
            .commands
            .send(Command::Load {
                id: track.id.clone(),
                position_ms: 0,
                generation: 1,
            })
            .unwrap();
        expect(&mut player, |e| {
            matches!(e, Event::Playing { generation: 1, .. })
        })
        .await;
        println!("PASS: standalone audio started");
        player.commands.send(Command::Pause).unwrap();
        expect(&mut player, |e| matches!(e, Event::Paused { .. })).await;
        player.commands.send(Command::Seek(10_000)).unwrap();
        expect(
            &mut player,
            |e| matches!(e,Event::Position{position_ms,..} if *position_ms>=9_000),
        )
        .await;
        player.commands.send(Command::Resume).unwrap();
        expect(
            &mut player,
            |e| matches!(e,Event::Playing{position_ms,..} if *position_ms>=9_000),
        )
        .await;
        player.commands.send(Command::Volume(25)).unwrap();
        expect(&mut player, |e| matches!(e, Event::Volume(25))).await;
        println!("PASS: pause, seek, resume, volume command acknowledgement");
        player.commands.send(Command::SimulateDisconnect).unwrap();
        expect(&mut player, |e| matches!(e, Event::Error(_))).await;
        player
            .commands
            .send(Command::Load {
                id: track.id.clone(),
                position_ms: 12_000,
                generation: 2,
            })
            .unwrap();
        expect(&mut player, |e| {
            matches!(e, Event::Playing { generation: 2, .. })
        })
        .await;
        println!("PASS: injected streaming-session loss and reconnection without browser login");
        player
            .commands
            .send(Command::Seek(track.duration_ms.saturating_sub(1200)))
            .unwrap();
        expect(&mut player, |e| matches!(e, Event::Completed(2))).await;
        println!("PASS: real track-completion event after seeking near end");
        drop(player);
        let mut reused = Playback::spawn(
            TokenManager::load_streaming().unwrap(),
            config.client_id,
            20,
        );
        reused
            .commands
            .send(Command::Load {
                id: track.id,
                position_ms: 0,
                generation: 3,
            })
            .unwrap();
        expect(&mut reused, |e| {
            matches!(e, Event::Playing { generation: 3, .. })
        })
        .await;
        reused.commands.send(Command::Stop).unwrap();
        println!("PASS: new player reuses Windows Credential Manager login");
    }
}
