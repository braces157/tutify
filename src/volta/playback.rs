//! A cancellable FLAC-capable player; resolution and decoding stay off the UI thread.
use super::{Audio, Client};
use crate::playback::HttpResolver as Resolver;
use crate::playback::pcm::{CAPACITY, Pcm};
use crate::{
    audio_output::Output,
    playback::{Command, Event},
    visualizer::AudioVisualizer,
};
use anyhow::{Context, Result, ensure};
#[cfg(test)]
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
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

struct Active {
    id: String,
    ticket: u64,
    generation: u64,
    base: u32,
    paused: bool,
    started: bool,
    samples: Arc<AtomicU64>,
    audio: Option<Audio>,
    metadata: Option<Box<crate::model::Track>>,
    output: Option<Output>,
    job: JoinHandle<()>,
}
impl Active {
    fn position(&self) -> u32 {
        let rate = self.output.as_ref().map_or(44100, |output| output.rate);
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
    Resolved(
        u64,
        Audio,
        Option<Box<crate::model::Track>>,
        mpsc::Receiver<Vec<f32>>,
        oneshot::Sender<u32>,
    ),
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
            let (audio, metadata) = match cached.filter(Audio::fresh) {
                Some(a) => (a, None),
                None => resolver.audio(&next_id).await?,
            };
            let (tx, rx) = mpsc::channel(CAPACITY);
            let (format_tx, format_rx) = oneshot::channel();
            if updates
                .send(Update::Resolved(
                    ticket,
                    audio.clone(),
                    metadata.map(Box::new),
                    rx,
                    format_tx,
                ))
                .is_err()
            {
                return Ok(());
            }
            let Ok(rate) = format_rx.await else {
                return Ok(());
            };
            decode(
                &ffmpeg,
                &audio,
                rate,
                base,
                tx,
                Some((ticket, updates.clone())),
            )
            .await
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
        metadata: None,
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
) -> Result<u32> {
    let output = crate::audio_output::open(audio.quality.sample_rate)?;
    let sink = &output.sink;
    sink.set_volume(f32::from(volume.min(100)) / 100.0);
    if active.paused {
        sink.pause();
    }
    if active.generation != 0 {
        visualizer.set_sample_rate(output.rate);
    }
    sink.append(Pcm::new(
        rx,
        active.samples.clone(),
        output.rate,
        visualizer.clone(),
    ));
    let rate = output.rate;
    active.audio = Some(audio);
    active.output = Some(output);
    Ok(rate)
}
pub(crate) fn ffmpeg() -> Result<PathBuf> {
    crate::youtube::locate("ffmpeg.exe")
}
async fn decode(
    ffmpeg: &std::path::Path,
    audio: &Audio,
    output_rate: u32,
    position: u32,
    tx: mpsc::Sender<Vec<f32>>,
    report: Option<(u64, mpsc::UnboundedSender<Update>)>,
) -> Result<()> {
    let tunnel = crate::monochrome::connection::Tunnel::for_audio(audio).await?;
    let mut command = crate::youtube::hidden_command(ffmpeg);
    command.args([
        "-hide_banner",
        "-loglevel",
        "info",
        "-nostats",
        "-nostdin",
        "-rw_timeout",
        "20000000",
        "-reconnect",
        "1",
        "-reconnect_delay_max",
        "5",
        "-protocol_whitelist",
        "http,https,tcp,tls,crypto,httpproxy",
    ]);
    if let Some(tunnel) = &tunnel {
        command.arg("-http_proxy").arg(&tunnel.url);
    }
    if position > 0 && audio.source != "Monochrome" {
        command
            .arg("-ss")
            .arg(format!("{:.3}", f64::from(position) / 1000.0));
    }
    command
        .arg("-headers")
        .arg(&audio.headers)
        .arg("-i")
        .arg(&audio.url);
    // Monochrome can reject the extra byte-range requests made by input seeking.
    // Decode/discard to the requested timestamp over the verified connection.
    if position > 0 && audio.source == "Monochrome" {
        command
            .arg("-ss")
            .arg(format!("{:.3}", f64::from(position) / 1000.0));
    }
    crate::audio_output::pcm_args(&mut command, output_rate);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .context("Could not start FFmpeg; install it with 'tuitify youtube setup'")?;
    let mut stdout = child.stdout.take().context("FFmpeg stdout unavailable")?;
    let stderr = child.stderr.take().context("FFmpeg stderr unavailable")?;
    let read = async {
        let mut buffer = vec![0u8; (output_rate as usize / 10) * 8];
        loop {
            let mut filled = 0;
            while filled < buffer.len() {
                let count = tokio::time::timeout(
                    Duration::from_secs(25),
                    stdout.read(&mut buffer[filled..]),
                )
                .await
                .context("Audio stream stalled; press Space to reconnect")??;
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
                .map(|b| crate::audio_output::pcm_sample(*b))
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
                "Audio decoder diagnostics exceeded the size limit"
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
        "Audio connection failed; press Space for a fresh playback link"
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
    let mut prepared: Option<Active> = None;
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
                    Command::Load { id, position_ms, generation } => {
                        active = None;
                        if let Some(mut next) = prepared.take().filter(|next| next.id == id && position_ms == 0) {
                            next.generation = generation;
                            next.paused = false;
                            if let Some(output) = &next.output {
                                output.sink.set_volume(f32::from(volume) / 100.0);
                                visualizer.set_sample_rate(output.rate);
                                output.sink.play();
                            }
                            if let Some(audio) = &next.audio {
                                if audio.source != resolver.source() || audio.source == "Monochrome" { let _ = events.send(Event::Fallback { generation, source: audio.source }); }
                                let _ = events.send(Event::Quality { generation, quality: audio.quality });
                            }
                            if let Some(track) = &next.metadata { let _ = events.send(Event::Metadata { generation, track: track.as_ref().clone() }); }
                            active = Some(next);
                            None
                        } else { Some((id, generation, position_ms, false, None)) }
                    },
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
                    Command::Stop => { active = None; prepared = None; None },
                    Command::CancelPreload => { prepared = None; None },
                    Command::Preload { id } => {
                        if active.is_some() && resolver.accepts(&id) && prepared.as_ref().is_none_or(|next| next.id != id) {
                            drop(prepared.take());
                            ticket = ticket.wrapping_add(1);
                            prepared = Some(start(&resolver, &ffmpeg, Intent { id, generation: 0, base: 0, paused: true, cached: None }, ticket, &updates));
                        }
                        None
                    },
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
                Update::Resolved(t, audio, metadata, chunks, format) if active.as_ref().is_some_and(|a| a.ticket == t) => {
                    let quality = audio.quality;
                    let source = audio.source;
                    if let Some(a) = &mut active {
                        if let Some(track) = metadata {
                            let _ = events.send(Event::Metadata { generation: a.generation, track: track.as_ref().clone() });
                            a.metadata = Some(track);
                        }
                        match open(a, audio, chunks, volume, &visualizer) {
                            Ok(rate) => {
                                let _ = format.send(rate);
                                if source != resolver.source() || source == "Monochrome" { let _ = events.send(Event::Fallback { generation: a.generation, source }); }
                                let _ = events.send(Event::Quality { generation: a.generation, quality });
                            }
                            Err(error) => {
                                let generation = a.generation; active = None; let _ = events.send(Event::TrackError { generation, message: format!("{error:#}") });
                            }
                        }
                    }
                }
                Update::Resolved(t, audio, metadata, chunks, format) if prepared.as_ref().is_some_and(|a| a.ticket == t) => {
                    if let Some(next) = &mut prepared {
                        next.metadata = metadata;
                        match open(next, audio, chunks, volume, &visualizer) {
                            Ok(rate) => { let _ = format.send(rate); }
                            Err(_) => prepared = None,
                        }
                    }
                }
                Update::Quality(t, quality) if prepared.as_ref().is_some_and(|a| a.ticket == t) => {
                    if let Some(audio) = prepared.as_mut().and_then(|a| a.audio.as_mut()) { audio.quality = quality; }
                }
                Update::Failed(t, _) if prepared.as_ref().is_some_and(|a| a.ticket == t) => { prepared = None; }
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
                        let _ = events.send(Event::TimeToPreload { generation: a.generation });
                    }
                    if a.output.as_ref().is_some_and(|o| o.sink.empty()) && a.job.is_finished() {
                        let _ = events.send(Event::Position { generation: a.generation, position_ms: a.position() });
                        let generation = a.generation; let started = a.started; active = None;
                        let _ = events.send(if started { Event::Completed(generation) } else { Event::TrackError { generation, message: "Audio stream returned no audio; choose another track".into() } });
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
    let requested = std::time::Instant::now();
    let mut started = None;
    let mut last_position = 0;
    let mut last_progress = std::time::Instant::now();
    let mut gaps = 0;
    let mut gap_ms = 0u128;
    loop {
        let event = tokio::time::timeout_at(end, playback.events.recv())
            .await
            .context("Volta audio probe timed out")?
            .context("Volta audio worker ended")?;
        match event {
            Event::Fallback { source, .. } => println!("Audio fallback: {source}."),
            Event::Quality {
                quality: reported, ..
            } => quality = Some(reported),
            Event::Position { position_ms, .. } if position_ms >= u32::from(seconds) * 1000 => {
                playback.commands.send(Command::Stop)?;
                println!(
                    "Volta audio PASS: decoded and played at least {seconds}s (volume {volume}%). Quality: {}.",
                    quality.unwrap_or_default().label(),
                );
                println!(
                    "Continuity: {gaps} gaps over 250 ms, {:.2}s stalled; {:.2}s since playback started.",
                    gap_ms as f64 / 1000.0,
                    started.map_or(0.0, |time: std::time::Instant| time.elapsed().as_secs_f64())
                );
                return Ok(());
            }
            Event::Playing { position_ms, .. } if started.is_none() => {
                started = Some(std::time::Instant::now());
                last_progress = std::time::Instant::now();
                last_position = position_ms;
                println!(
                    "Playback started after {:.2}s.",
                    requested.elapsed().as_secs_f64()
                );
            }
            Event::Position { position_ms, .. } if position_ms > last_position => {
                let elapsed = last_progress.elapsed().as_millis();
                let advanced = u128::from(position_ms - last_position);
                let gap = elapsed.saturating_sub(advanced);
                if gap > 250 {
                    gaps += 1;
                    gap_ms += gap;
                }
                last_progress = std::time::Instant::now();
                last_position = position_ms;
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

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "Live Volta lossless stream and muted Windows output; set TUITIFY_PROBE_TRACK"]
    async fn live_lossless_continuity_comparison() -> Result<()> {
        let id = std::env::var("TUITIFY_PROBE_TRACK")
            .context("Set TUITIFY_PROBE_TRACK to a Volta track ID")?;
        let ffmpeg = ffmpeg()?;
        let requested = std::time::Instant::now();
        let audio = Client::load()?.stream(&id).await?;
        println!(
            "Direct Volta resolution: {:.2}s; {}",
            requested.elapsed().as_secs_f64(),
            audio.quality.label()
        );
        for legacy in [true, false] {
            let output = crate::audio_output::open(audio.quality.sample_rate)?;
            output.sink.set_volume(0.0);
            let (tx, rx) = mpsc::channel(if legacy { 4 } else { CAPACITY });
            let samples = Arc::new(AtomicU64::new(0));
            let source = Pcm::new(rx, samples.clone(), output.rate, AudioVisualizer::new());
            output
                .sink
                .append(if legacy { source.legacy() } else { source });
            let decoder = tokio::spawn({
                let audio = audio.clone();
                let ffmpeg = ffmpeg.clone();
                let rate = output.rate;
                async move { decode(&ffmpeg, &audio, rate, 0, tx, None).await }
            });
            let request = std::time::Instant::now();
            let mut started = None;
            let mut previous = 0;
            let mut last = std::time::Instant::now();
            let mut gaps = 0;
            let mut stalled = 0u128;
            let result: Result<()> = tokio::time::timeout(Duration::from_secs(100), async {
                loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    let position =
                        samples.load(Ordering::Relaxed) * 1000 / (u64::from(output.rate) * 2);
                    if position > 0 && started.is_none() {
                        started = Some(std::time::Instant::now());
                        println!(
                            "{} startup: {:.2}s",
                            if legacy { "Legacy" } else { "Buffered" },
                            request.elapsed().as_secs_f64()
                        );
                    } else if position > previous {
                        let gap = last
                            .elapsed()
                            .as_millis()
                            .saturating_sub(u128::from(position - previous));
                        if gap > 250 {
                            gaps += 1;
                            stalled += gap;
                        }
                    }
                    if position > previous {
                        last = std::time::Instant::now();
                        previous = position;
                    }
                    if position >= 20000 {
                        break;
                    }
                    if decoder.is_finished() && output.sink.empty() {
                        anyhow::bail!("Direct stream ended before 20 seconds");
                    }
                }
                println!(
                    "{}: 20s audio in {:.2}s; {gaps} gaps, {:.2}s stalled",
                    if legacy { "Legacy" } else { "Buffered" },
                    started.unwrap().elapsed().as_secs_f64(),
                    stalled as f64 / 1000.0
                );
                Ok(())
            })
            .await
            .context("Direct lossless probe timed out")?;
            decoder.abort();
            let _ = decoder.await;
            result?;
        }
        Ok(())
    }

    #[test]
    fn native_pcm_stalls_do_not_advance_position_and_end_drains_samples() {
        let (tx, rx) = mpsc::channel(1);
        let samples = Arc::new(AtomicU64::new(0));
        let mut pcm = Pcm::new(rx, samples.clone(), 192000, AudioVisualizer::new());
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
        for rate in [44100, 48000, 96000, 192000, 384000] {
            let directory = tempfile::tempdir()?;
            let file = directory.path().join("fixture.flac");
            let raw = directory.path().join("fixture.s24le");
            // Include the least significant 24-bit values and full-scale edges.
            let integers: Vec<i32> = (0..rate as usize * 2 / 5)
                .map(|i| match i % 8 {
                    0 => -8388608,
                    1 => 8388607,
                    2 => -1,
                    3 => 1,
                    _ => ((i as i32 * 7919) % 16777216) - 8388608,
                })
                .collect();
            let expected: Vec<f32> = integers
                .iter()
                .map(|sample| *sample as f32 / 8388608.0)
                .collect();
            let raw_bytes: Vec<u8> = integers
                .iter()
                .flat_map(|sample| sample.to_le_bytes()[..3].to_vec())
                .collect();
            std::fs::write(&raw, raw_bytes)?;
            let result = crate::youtube::hidden_command(&ffmpeg)
                .args(["-hide_banner", "-loglevel", "error", "-f", "s24le", "-ar"])
                .arg(rate.to_string())
                .args(["-ac", "2", "-i"])
                .arg(&raw)
                .args(["-c:a", "flac"])
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
                quality: crate::audio_quality::AudioQuality {
                    sample_rate: Some(rate),
                    ..Default::default()
                },
                obtained: std::time::Instant::now(),
                source: "Volta",
                connect_addr: None,
            };
            let (tx, mut rx) = mpsc::channel(CAPACITY);
            let collect = async {
                let mut samples = Vec::new();
                while let Some(chunk) = rx.recv().await {
                    samples.extend(chunk);
                }
                samples
            };
            let (updates, mut reports) = mpsc::unbounded_channel();
            let (result, samples) = tokio::join!(
                decode(&ffmpeg, &audio, rate, 0, tx, Some((1, updates))),
                collect
            );
            result?;
            assert!(
                matches!(reports.try_recv(), Ok(Update::Quality(1, quality)) if quality.codec == Some(crate::audio_quality::Codec::Flac) && quality.sample_rate == Some(rate) && quality.bit_depth == Some(24))
            );
            assert_eq!(
                samples, expected,
                "24-bit FLAC samples changed at {rate} Hz"
            );
            println!("FLAC PASS: every 24-bit sample preserved exactly at {rate} Hz");
        }
        Ok(())
    }

    #[tokio::test]
    #[ignore = "Requires installed FFmpeg; generated FLAC over loopback, no audible playback"]
    async fn monochrome_output_seek_preserves_remaining_duration() -> Result<()> {
        let ffmpeg = ffmpeg()?;
        let directory = tempfile::tempdir()?;
        let file = directory.path().join("seek.flac");
        let generated = crate::youtube::hidden_command(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=1000:sample_rate=48000:duration=2",
                "-ac",
                "2",
                "-c:a",
                "flac",
            ])
            .arg(&file)
            .output()
            .await?;
        ensure!(
            generated.status.success(),
            "Could not generate seek fixture"
        );
        let server = MockServer::start().await;
        Mock::given(path("/audio"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(std::fs::read(file)?))
            .expect(1)
            .mount(&server)
            .await;
        let audio = Audio {
            url: format!("{}/audio", server.uri()),
            headers: String::new(),
            quality: crate::audio_quality::AudioQuality::default(),
            obtained: std::time::Instant::now(),
            source: "Monochrome",
            connect_addr: None,
        };
        let (tx, mut rx) = mpsc::channel::<Vec<f32>>(CAPACITY);
        let collect = async {
            let mut samples = 0;
            while let Some(chunk) = rx.recv().await {
                samples += chunk.len();
            }
            samples
        };
        let (result, samples) =
            tokio::join!(decode(&ffmpeg, &audio, 48000, 750, tx, None), collect);
        result?;
        assert_eq!(
            samples,
            48000 * 2 * 5 / 4,
            "A 750 ms seek must leave 1.25 seconds of stereo audio"
        );
        Ok(())
    }

    #[tokio::test]
    #[ignore = "Requires installed FFmpeg; generated FLAC over loopback, no audible playback"]
    async fn high_resolution_downsampling_rejects_aliasing_and_preserves_duration() -> Result<()> {
        let ffmpeg = ffmpeg()?;
        let directory = tempfile::tempdir()?;
        let file = directory.path().join("tones.flac");
        let generated = crate::youtube::hidden_command(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "aevalsrc=0.25*sin(2*PI*1000*t)+0.25*sin(2*PI*60000*t):s=192000:d=0.2",
                "-ac",
                "2",
                "-c:a",
                "flac",
            ])
            .arg(&file)
            .output()
            .await?;
        ensure!(
            generated.status.success(),
            "Could not generate resampling fixture"
        );
        let server = MockServer::start().await;
        Mock::given(path("/audio"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(std::fs::read(file)?))
            .mount(&server)
            .await;
        let audio = Audio {
            url: format!("{}/audio", server.uri()),
            headers: String::new(),
            quality: crate::audio_quality::AudioQuality::default(),
            obtained: std::time::Instant::now(),
            source: "Volta",
            connect_addr: None,
        };
        let (tx, mut rx) = mpsc::channel(CAPACITY);
        let (updates, mut reports) = mpsc::unbounded_channel();
        let collect = async {
            let mut samples = Vec::new();
            while let Some(chunk) = rx.recv().await {
                samples.extend(chunk);
            }
            samples
        };
        let (result, samples) = tokio::join!(
            decode(&ffmpeg, &audio, 48000, 0, tx, Some((1, updates))),
            collect
        );
        result?;
        assert_eq!(samples.len(), 48000 * 2 / 5);
        // The quality label must still describe the 192 kHz source, not 48 kHz output.
        assert!(
            matches!(reports.try_recv(), Ok(Update::Quality(1, quality)) if quality.sample_rate == Some(192000))
        );
        let amplitude = |frequency: f64| {
            let (mut re, mut im) = (0.0, 0.0);
            for (frame, sample) in samples.as_chunks::<2>().0[1200..6000].iter().enumerate() {
                let phase = std::f64::consts::TAU * frequency * frame as f64 / 48000.0;
                re += f64::from(sample[0]) * phase.cos();
                im += f64::from(sample[0]) * phase.sin();
            }
            (re * re + im * im).sqrt() * 2.0 / 4800.0
        };
        // Mono-to-stereo rematrixing applies a sqrt(1/2) gain in FFmpeg.
        assert!((amplitude(1000.0) - 0.25 / 2.0_f64.sqrt()).abs() < 0.001);
        assert!(
            amplitude(12000.0) < 0.0001,
            "60 kHz content aliased into the audible band"
        );
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
                    Resolver::Volta(crate::volta::matching::Resolver::new(client, None).into()),
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

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "Requires installed FFmpeg and a Windows output device; generated audio is muted"]
    async fn preloaded_audio_is_silent_and_promoted_without_a_second_stream_request() -> Result<()>
    {
        let ffmpeg = ffmpeg()?;
        let directory = tempfile::tempdir()?;
        let file = directory.path().join("preload.flac");
        let generated = crate::youtube::hidden_command(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=12",
                "-ac",
                "2",
                "-c:a",
                "flac",
            ])
            .arg(&file)
            .output()
            .await?;
        ensure!(
            generated.status.success(),
            "Could not generate preload fixture"
        );
        let server = MockServer::start().await;
        Mock::given(path("/api/v1/oauth/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"access_token":"volta_at_test","scope":"catalog:read stream:read","expires_in":3600})))
            .mount(&server).await;
        Mock::given(path("/api/v1/stream"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"stream_url":"/api/v1/app_stream_relay","sample_rate":48000}),
            ))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(path("/api/v1/app_stream_relay"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(std::fs::read(file)?))
            .expect(2)
            .mount(&server)
            .await;
        let client = Client::new(
            super::super::Credentials {
                client_id: "volta_id_test".into(),
                client_secret: "volta_sk_test".into(),
            },
            &format!("{}/api/v1", server.uri()),
        )?;
        tokio::task::LocalSet::new()
            .run_until(async move {
                let (commands, rx) = mpsc::unbounded_channel();
                let (events, mut output) = mpsc::unbounded_channel();
                let task = tokio::task::spawn_local(worker(
                    Resolver::Volta(crate::volta::matching::Resolver::new(client, None).into()),
                    ffmpeg,
                    0,
                    rx,
                    events,
                    AudioVisualizer::new(),
                ));
                assert!(matches!(output.recv().await, Some(Event::Ready)));
                commands.send(Command::Load {
                    id: "volta:first".into(),
                    position_ms: 0,
                    generation: 1,
                })?;
                tokio::time::timeout(Duration::from_secs(10), async {
                    loop {
                        if matches!(
                            output.recv().await,
                            Some(Event::Playing { generation: 1, .. })
                        ) {
                            break;
                        }
                    }
                })
                .await?;
                commands.send(Command::Preload {
                    id: "volta:next".into(),
                })?;
                // The shared client deliberately spaces /stream calls by 6.1 seconds.
                tokio::time::timeout(Duration::from_secs(15), async {
                    loop {
                        let requests = server.received_requests().await.unwrap();
                        if requests
                            .iter()
                            .filter(|request| request.url.path() == "/api/v1/app_stream_relay")
                            .count()
                            == 2
                        {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await?;
                // Let the bounded PCM buffer fill. The next generation must stay silent.
                tokio::time::sleep(Duration::from_millis(300)).await;
                while let Ok(event) = output.try_recv() {
                    assert_ne!(
                        match event {
                            Event::Playing { generation, .. }
                            | Event::Position { generation, .. } => generation,
                            _ => 1,
                        },
                        0
                    );
                }
                commands.send(Command::Load {
                    id: "volta:next".into(),
                    position_ms: 0,
                    generation: 2,
                })?;
                tokio::time::timeout(Duration::from_secs(2), async {
                    loop {
                        if let Some(Event::Playing {
                            generation: 2,
                            position_ms,
                        }) = output.recv().await
                        {
                            assert!(
                                position_ms < 500,
                                "preloaded audio was consumed before promotion"
                            );
                            break;
                        }
                    }
                })
                .await?;
                commands.send(Command::Stop)?;
                drop(commands);
                task.await??;
                server.verify().await;
                Ok::<_, anyhow::Error>(())
            })
            .await
    }
}
