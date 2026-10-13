//! A cancellable FLAC-capable player; resolution and decoding stay off the UI thread.
use super::{Audio, Client, matching::Resolver};
use crate::{
    playback::{Command, Event},
    visualizer::AudioVisualizer,
};
use anyhow::{Context, Result, ensure};
use rodio::Source;
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    sync::mpsc,
    task::JoinHandle,
};

struct Pcm {
    chunks: mpsc::Receiver<Vec<f32>>,
    current: std::vec::IntoIter<f32>,
    samples: Arc<AtomicU64>,
    rate: u32,
    visualizer: Arc<AudioVisualizer>,
    left: Option<f32>,
}
impl Iterator for Pcm {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.current.len() == 0 {
            match self.chunks.try_recv() {
                Ok(chunk) => self.current = chunk.into_iter(),
                Err(mpsc::error::TryRecvError::Empty) => return Some(0.0),
                Err(mpsc::error::TryRecvError::Disconnected) => return None,
            }
        }
        let sample = self.current.next()?;
        self.samples.fetch_add(1, Ordering::Relaxed);
        if let Some(left) = self.left.take() {
            self.visualizer.push_sample((left + sample) * 0.5);
        } else {
            self.left = Some(sample);
        }
        Some(sample)
    }
}
impl Source for Pcm {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
struct Output {
    sink: rodio::Sink,
    _stream: rodio::OutputStream,
}
struct Active {
    id: String,
    ticket: u64,
    generation: u64,
    base: u32,
    paused: bool,
    started: bool,
    samples: Arc<AtomicU64>,
    audio: Option<Audio>,
    output: Option<Output>,
    job: JoinHandle<()>,
}
impl Active {
    fn position(&self) -> u32 {
        let rate = self.audio.as_ref().map_or(44100, |a| a.rate);
        self.base.saturating_add(
            (self.samples.load(Ordering::Relaxed) * 1000 / (u64::from(rate) * 2))
                .min(u64::from(u32::MAX)) as u32,
        )
    }
}
impl Drop for Active {
    fn drop(&mut self) {
        self.job.abort();
        if let Some(output) = &self.output {
            output.sink.stop();
        }
    }
}
enum Update {
    Quality(u64, crate::audio_quality::AudioQuality),
    Resolved(u64, Audio, mpsc::Receiver<Vec<f32>>),
    Failed(u64, String),
}
struct Intent {
    id: String,
    generation: u64,
    base: u32,
    paused: bool,
    cached: Option<Audio>,
}
fn start(
    resolver: &Resolver,
    ffmpeg: &std::path::Path,
    intent: Intent,
    ticket: u64,
    updates: &mpsc::UnboundedSender<Update>,
) -> Active {
    let Intent {
        id,
        generation,
        base,
        paused,
        cached,
    } = intent;
    let resolver = resolver.clone();
    let ffmpeg = ffmpeg.to_owned();
    let next_id = id.clone();
    let updates = updates.clone();
    let job = tokio::spawn(async move {
        let result: Result<()> = async {
            let audio = match cached.filter(Audio::fresh) {
                Some(a) => a,
                None => resolver.audio(&next_id).await?,
            };
            let (tx, rx) = mpsc::channel(4);
            if updates
                .send(Update::Resolved(ticket, audio.clone(), rx))
                .is_err()
            {
                return Ok(());
            }
            decode(&ffmpeg, &audio, base, tx, Some((ticket, updates.clone()))).await
        }
        .await;
        if let Err(error) = result {
            let _ = updates.send(Update::Failed(ticket, format!("{error:#}")));
        }
    });
    Active {
        id,
        ticket,
        generation,
        base,
        paused,
        started: false,
        samples: Arc::new(AtomicU64::new(0)),
        audio: None,
        output: None,
        job,
    }
}
fn open(
    active: &mut Active,
    audio: Audio,
    rx: mpsc::Receiver<Vec<f32>>,
    volume: u8,
    visualizer: &Arc<AudioVisualizer>,
) -> Result<()> {
    let (stream, handle) = rodio::OutputStream::try_default()
        .context("Cannot open Windows audio output; select a working default output and retry")?;
    let sink = rodio::Sink::try_new(&handle)?;
    sink.set_volume(f32::from(volume.min(100)) / 100.0);
    if active.paused {
        sink.pause();
    }
    visualizer.set_sample_rate(audio.rate);
    sink.append(Pcm {
        chunks: rx,
        current: Vec::new().into_iter(),
        samples: active.samples.clone(),
        rate: audio.rate,
        visualizer: visualizer.clone(),
        left: None,
    });
    active.audio = Some(audio);
    active.output = Some(Output {
        sink,
        _stream: stream,
    });
    Ok(())
}
pub(crate) fn ffmpeg() -> Result<PathBuf> {
    crate::youtube::locate("ffmpeg.exe")
}
async fn decode(
    ffmpeg: &std::path::Path,
    audio: &Audio,
    position: u32,
    tx: mpsc::Sender<Vec<f32>>,
    report: Option<(u64, mpsc::UnboundedSender<Update>)>,
) -> Result<()> {
    let mut command = crate::youtube::hidden_command(ffmpeg);
    command.args([
        "-hide_banner",
        "-loglevel",
        "info",
        "-nostats",
        "-nostdin",
        "-rw_timeout",
        "20000000",
        "-protocol_whitelist",
        "http,https,tcp,tls,crypto",
    ]);
    if position > 0 {
        command
            .arg("-ss")
            .arg(format!("{:.3}", f64::from(position) / 1000.0));
    }
    command
        .arg("-headers")
        .arg(&audio.headers)
        .arg("-i")
        .arg(&audio.url)
        .args(["-vn", "-sn", "-dn", "-ac", "2", "-ar"])
        .arg(audio.rate.to_string())
        .args(["-f", "f32le", "-acodec", "pcm_f32le", "pipe:1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .context("Could not start FFmpeg; install it with 'tuitify youtube setup'")?;
    let mut stdout = child.stdout.take().context("FFmpeg stdout unavailable")?;
    let stderr = child.stderr.take().context("FFmpeg stderr unavailable")?;
    let read = async {
        let mut buffer = vec![0u8; (audio.rate as usize / 10) * 8];
        loop {
            let mut filled = 0;
            while filled < buffer.len() {
                let count = tokio::time::timeout(
                    Duration::from_secs(25),
                    stdout.read(&mut buffer[filled..]),
                )
                .await
                .context("Volta audio stalled; press Space to reconnect")??;
                if count == 0 {
                    break;
                }
                filled += count;
            }
            if filled == 0 {
                break;
            }
            ensure!(filled % 8 == 0, "FFmpeg returned incomplete stereo audio");
            let samples = buffer[..filled]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| {
                    let sample = f32::from_le_bytes(*b);
                    if sample.is_finite() {
                        sample.clamp(-1.0, 1.0)
                    } else {
                        0.0
                    }
                })
                .collect();
            if tx.send(samples).await.is_err() {
                return Ok::<_, anyhow::Error>(());
            }
            if filled < buffer.len() {
                break;
            }
        }
        Ok(())
    };
    let inspect = async {
        let mut reader = BufReader::new(stderr.take(64 * 1024 + 1));
        let mut used = 0;
        let mut line = Vec::new();
        let mut reported = false;
        loop {
            line.clear();
            let count = reader.read_until(b'\n', &mut line).await?;
            if count == 0 {
                break;
            }
            used += count;
            ensure!(
                used <= 64 * 1024,
                "Volta decoder diagnostics exceeded the size limit"
            );
            if !reported
                && let Some(quality) = audio.quality.decoded(&String::from_utf8_lossy(&line))
            {
                reported = true;
                if let Some((ticket, updates)) = &report {
                    let _ = updates.send(Update::Quality(*ticket, quality));
                }
            }
        }
        Ok::<_, anyhow::Error>(())
    };
    tokio::try_join!(read, inspect)?;
    // FFmpeg stderr may contain signed URLs/cookies; retain only a safe message.
    ensure!(
        child.wait().await?.success(),
        "Volta audio connection failed; press Space for a fresh playback link"
    );
    Ok(())
}
pub(crate) async fn worker(
    resolver: Resolver,
    ffmpeg: PathBuf,
    mut volume: u8,
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<Event>,
    visualizer: Arc<AudioVisualizer>,
) -> Result<()> {
    let (updates, mut rx) = mpsc::unbounded_channel();
    let mut active: Option<Active> = None;
    let mut ticket = 0u64;
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let _ = events.send(Event::Ready);
    loop {
        tokio::select! {
            biased;
            command = commands.recv() => {
                let Some(command) = command else { break; };
                let intent = match command {
                    Command::Load { id, position_ms, generation } => Some((id, generation, position_ms, false, None)),
                    Command::Seek(position) => active.as_ref().map(|a| (a.id.clone(), a.generation, position, a.paused, a.audio.clone())),
                    Command::Pause => { if let Some(a) = &mut active { a.paused = true; if let Some(o) = &a.output { o.sink.pause(); } let _ = events.send(Event::Paused { generation: a.generation, position_ms: a.position() }); } None },
                    Command::Resume => {
                        if let Some(a) = &mut active {
                            a.paused = false;
                            if let Some(o) = &a.output { o.sink.play(); }
                            if a.started { let _ = events.send(Event::Playing { generation: a.generation, position_ms: a.position() }); }
                        }
                        None
                    },
                    Command::Volume(value) => { volume = value.min(100); if let Some(o) = active.as_ref().and_then(|a| a.output.as_ref()) { o.sink.set_volume(f32::from(volume) / 100.0); } let _ = events.send(Event::Volume(volume)); None },
                    Command::Stop => { active = None; None },
                    // Resolve only on demand: signed links expire, and /stream is limited to 10/min.
                    Command::Preload { .. } => None,
                    #[cfg(test)]
                    Command::SimulateDisconnect => { active = None; None },
                };
                if let Some((id, generation, position, paused, cached)) = intent {
                    active = None;
                    if !resolver.accepts(&id) { let _ = events.send(Event::TrackError { generation, message: "Choose a track from the connected music catalogue".into() }); continue; }
                    ticket = ticket.wrapping_add(1);
                    active = Some(start(&resolver, &ffmpeg, Intent { id, generation, base: position, paused, cached }, ticket, &updates));
                }
            }
            Some(update) = rx.recv() => match update {
                Update::Resolved(t, audio, chunks) if active.as_ref().is_some_and(|a| a.ticket == t) => {
                    let quality = audio.quality;
                    if let Some(a) = &mut active && let Err(error) = open(a, audio, chunks, volume, &visualizer) {
                        let generation = a.generation; active = None; let _ = events.send(Event::TrackError { generation, message: format!("{error:#}") });
                    } else if let Some(a) = &active {
                        let _ = events.send(Event::Quality { generation: a.generation, quality });
                    }
                }
                Update::Quality(t, quality) if active.as_ref().is_some_and(|a| a.ticket == t) => {
                    let a = active.as_mut().unwrap();
                    if let Some(audio) = &mut a.audio { audio.quality = quality; }
                    let _ = events.send(Event::Quality { generation: a.generation, quality });
                }
                Update::Failed(t, message) if active.as_ref().is_some_and(|a| a.ticket == t) => {
                    let generation = active.as_ref().unwrap().generation; active = None; let _ = events.send(Event::TrackError { generation, message });
                }
                _ => (),
            },
            _ = tick.tick(), if active.is_some() => {
                if let Some(a) = &mut active {
                    if !a.started && a.samples.load(Ordering::Relaxed) > 0 {
                        a.started = true;
                        let _ = events.send(if a.paused { Event::Paused { generation: a.generation, position_ms: a.position() } } else { Event::Playing { generation: a.generation, position_ms: a.position() } });
                    }
                    if a.output.as_ref().is_some_and(|o| o.sink.empty()) && a.job.is_finished() {
                        let generation = a.generation; let started = a.started; active = None;
                        let _ = events.send(if started { Event::Completed(generation) } else { Event::TrackError { generation, message: "Volta returned no audio; choose another track".into() } });
                    } else if a.started && !a.paused { let _ = events.send(Event::Position { generation: a.generation, position_ms: a.position() }); }
                }
            }
        }
    }
    Ok(())
}
pub(crate) async fn probe(input: &str, seconds: u8, volume: u8) -> Result<()> {
    let spotify = crate::model::track_id(input)
        .or_else(|| crate::model::valid_id(input).then(|| input.to_owned()));
    let (id, mut playback) = if let Some(id) = spotify {
        let config = crate::storage::Storage::local_read_only()?.config()?;
        let catalog = crate::catalog::Catalog::new(crate::auth::TokenManager::load(&config)?)?;
        (
            id,
            crate::playback::Playback::spawn_spotify_volta(
                Client::load()?,
                catalog,
                volume,
                AudioVisualizer::new(),
            )?,
        )
    } else {
        let id = super::track_id(input).context("Supply a Volta track ID or Spotify track link")?;
        (
            id,
            crate::playback::Playback::spawn_volta(
                Client::load()?,
                volume,
                AudioVisualizer::new(),
            )?,
        )
    };
    playback.commands.send(Command::Load {
        id,
        position_ms: 0,
        generation: 1,
    })?;
    let end = tokio::time::Instant::now() + Duration::from_secs(u64::from(seconds) + 45);
    let mut quality = None;
    loop {
        let event = tokio::time::timeout_at(end, playback.events.recv())
            .await
            .context("Volta audio probe timed out")?
            .context("Volta audio worker ended")?;
        match event {
            Event::Quality {
                quality: reported, ..
            } => quality = Some(reported),
            Event::Position { position_ms, .. } if position_ms >= u32::from(seconds) * 1000 => {
                playback.commands.send(Command::Stop)?;
                println!(
                    "Volta audio PASS: decoded and played at least {seconds}s (volume {volume}%). Quality: {}.",
                    quality.unwrap_or_default().label(),
                );
                return Ok(());
            }
            Event::Error(message) | Event::TrackError { message, .. } => anyhow::bail!("{message}"),
            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, path},
    };

    #[test]
    fn native_pcm_stalls_do_not_advance_position_and_end_drains_samples() {
        let (tx, rx) = mpsc::channel(1);
        let samples = Arc::new(AtomicU64::new(0));
        let mut pcm = Pcm {
            chunks: rx,
            current: Vec::new().into_iter(),
            samples: samples.clone(),
            rate: 192000,
            visualizer: AudioVisualizer::new(),
            left: None,
        };
        assert_eq!(pcm.sample_rate(), 192000);
        assert_eq!(pcm.next(), Some(0.0));
        assert_eq!(samples.load(Ordering::Relaxed), 0);
        tx.try_send(vec![0.25, -0.25]).unwrap();
        drop(tx);
        assert_eq!(pcm.collect::<Vec<_>>(), vec![0.25, -0.25]);
        assert_eq!(samples.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    #[ignore = "Requires the installed FFmpeg; uses only generated FLAC and a loopback HTTP relay"]
    async fn real_flac_decode_preserves_native_rate_and_sends_cookie() -> Result<()> {
        let ffmpeg = ffmpeg()?;
        for rate in [44100, 192000] {
            let directory = tempfile::tempdir()?;
            let file = directory.path().join("fixture.flac");
            let result = crate::youtube::hidden_command(&ffmpeg)
                .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i"])
                .arg(format!(
                    "sine=frequency=440:sample_rate={rate}:duration=0.2"
                ))
                .args(["-ac", "2", "-c:a", "flac"])
                .arg(&file)
                .output()
                .await?;
            ensure!(
                result.status.success(),
                "Could not generate the local FLAC fixture"
            );
            let server = MockServer::start().await;
            Mock::given(path("/api/v1/stream_relay"))
                .and(header("Cookie", "volta_stream=test-session"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_bytes(std::fs::read(&file)?)
                        .insert_header("content-type", "audio/flac"),
                )
                .expect(1)
                .mount(&server)
                .await;
            let audio = Audio {
                url: format!("{}/api/v1/stream_relay", server.uri()),
                headers: "User-Agent: Tuitify/test\r\nCookie: volta_stream=test-session\r\n".into(),
                rate,
                quality: crate::audio_quality::AudioQuality {
                    sample_rate: Some(rate),
                    ..Default::default()
                },
                obtained: std::time::Instant::now(),
            };
            let (tx, mut rx) = mpsc::channel(4);
            let collect = async {
                let mut samples = Vec::new();
                while let Some(chunk) = rx.recv().await {
                    samples.extend(chunk);
                }
                samples
            };
            let (updates, mut reports) = mpsc::unbounded_channel();
            let (result, samples) =
                tokio::join!(decode(&ffmpeg, &audio, 0, tx, Some((1, updates))), collect);
            result?;
            assert!(
                matches!(reports.try_recv(), Ok(Update::Quality(1, quality)) if quality.codec == Some(crate::audio_quality::Codec::Flac) && quality.sample_rate == Some(rate))
            );
            assert_eq!(samples.len(), rate as usize * 2 / 5);
            assert!(samples.iter().any(|sample| sample.abs() > 0.01));
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stop_during_resolution_keeps_controls_responsive() {
        let server = MockServer::start().await;
        Mock::given(path("/api/v1/oauth/token")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)).set_body_json(serde_json::json!({"access_token":"volta_at_test", "scope":"catalog:read stream:read", "expires_in":3600}))).mount(&server).await;
        let client = Client::new(
            super::super::Credentials {
                client_id: "volta_id_test".into(),
                client_secret: "volta_sk_test".into(),
            },
            &format!("{}/api/v1", server.uri()),
        )
        .unwrap();
        tokio::task::LocalSet::new()
            .run_until(async move {
                let (tx, commands) = mpsc::unbounded_channel();
                let (events, mut rx) = mpsc::unbounded_channel();
                let worker = tokio::task::spawn_local(worker(
                    Resolver::new(client, None),
                    PathBuf::from("unused.exe"),
                    0,
                    commands,
                    events,
                    AudioVisualizer::new(),
                ));
                assert!(matches!(rx.recv().await, Some(Event::Ready)));
                tx.send(Command::Load {
                    id: "volta:JPW462364925".into(),
                    position_ms: 0,
                    generation: 1,
                })
                .unwrap();
                tokio::time::timeout(Duration::from_secs(2), async {
                    while server.received_requests().await.unwrap().is_empty() {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                tx.send(Command::Stop).unwrap();
                tx.send(Command::Volume(17)).unwrap();
                assert!(matches!(
                    tokio::time::timeout(Duration::from_secs(1), rx.recv())
                        .await
                        .unwrap(),
                    Some(Event::Volume(17))
                ));
                drop(tx);
                worker.await.unwrap().unwrap();
            })
            .await;
    }
}
