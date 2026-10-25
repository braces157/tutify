//! Bounded PCM streaming. Pause/seek/stop interrupt resolution and decoding.
use super::{Stream, Tools, hidden_command};
use crate::playback::pcm::{CAPACITY, Pcm};
use crate::{
    playback::{Command, Event},
    visualizer::AudioVisualizer,
};
use anyhow::{Context, Result, ensure};
use std::{
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncReadExt, sync::mpsc, task::JoinHandle};

const CHANNELS: u16 = 2;

mod preload;
use preload::{Prepared, Preparer};

enum Update {
    Resolved(u64, Box<Stream>),
    Failed(u64, String),
}

struct Active {
    id: String,
    generation: u64,
    request: u64,
    base_ms: u32,
    paused: bool,
    started: bool,
    samples: Arc<AtomicU64>,
    sink: rodio::Sink,
    _output: rodio::cpal::Stream,
    rate: u32,
    job: JoinHandle<()>,
    stream: Option<Stream>,
}

impl Active {
    fn position(&self) -> u32 {
        self.base_ms.saturating_add(
            (self.samples.load(Ordering::Relaxed) * 1000
                / (u64::from(self.rate) * u64::from(CHANNELS)))
            .min(u64::from(u32::MAX)) as u32,
        )
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        self.job.abort();
        self.sink.stop();
    }
}

#[derive(Clone)]
struct Intent {
    id: String,
    generation: u64,
    base_ms: u32,
    paused: bool,
}

fn start(
    tools: &Tools,
    intent: Intent,
    request: u64,
    volume: u8,
    prepared: Option<Prepared>,
    updates: &mpsc::UnboundedSender<Update>,
    visualizer: &Arc<AudioVisualizer>,
) -> Result<Active> {
    ensure!(
        super::video_key(&intent.id).is_some(),
        "YouTube mode cannot play a Spotify track; use YouTube search to create a queue"
    );
    let initial_stream = prepared.as_ref().and_then(Prepared::ready);
    let output = crate::audio_output::open(
        initial_stream
            .as_ref()
            .and_then(|stream| stream.quality.sample_rate),
    )?;
    let rate = output.rate;
    let sink = output.sink;
    sink.set_volume(f32::from(volume.min(100)) / 100.0);
    if intent.paused {
        sink.pause();
    }
    let (chunks_tx, chunks_rx) = mpsc::channel(CAPACITY);
    let samples = Arc::new(AtomicU64::new(0));
    visualizer.set_sample_rate(rate);
    sink.append(Pcm::new(
        chunks_rx,
        samples.clone(),
        rate,
        visualizer.clone(),
    ));
    let tools = tools.clone();
    let id = intent.id.clone();
    let tx = updates.clone();
    let position = intent.base_ms;
    let job = tokio::spawn(async move {
        let result: Result<()> = async {
            let stream = if let Some(prepared) = prepared {
                prepared.resolve(&tools, &id).await?
            } else {
                tools.resolve(&id).await?
            };
            if tx
                .send(Update::Resolved(request, Box::new(stream.clone())))
                .is_err()
            {
                return Ok(());
            }
            decode(&tools, &stream, rate, position, chunks_tx).await
        }
        .await;
        if let Err(error) = result {
            let _ = tx.send(Update::Failed(request, format!("{error:#}")));
        }
    });
    Ok(Active {
        id: intent.id,
        generation: intent.generation,
        request,
        base_ms: intent.base_ms,
        paused: intent.paused,
        started: false,
        samples,
        sink,
        _output: output._stream,
        rate,
        job,
        stream: initial_stream,
    })
}

async fn decode(
    tools: &Tools,
    stream: &Stream,
    output_rate: u32,
    position_ms: u32,
    chunks: mpsc::Sender<Vec<f32>>,
) -> Result<()> {
    let mut command = hidden_command(&tools.ffmpeg);
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-rw_timeout",
        "20000000",
        "-reconnect",
        "1",
        "-reconnect_delay_max",
        "5",
        "-protocol_whitelist",
        "http,https,tcp,tls,crypto",
    ]);
    if position_ms > 0 {
        command
            .arg("-ss")
            .arg(format!("{:.3}", f64::from(position_ms) / 1000.0));
    }
    if !stream.headers.is_empty() {
        command.arg("-headers").arg(&stream.headers);
    }
    command.arg("-i").arg(&stream.url);
    crate::audio_output::pcm_args(&mut command, output_rate);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .context("Could not start FFmpeg; run 'tuitify youtube setup'")?;
    let mut stdout = child.stdout.take().context("FFmpeg stdout unavailable")?;
    let stderr = child.stderr.take().context("FFmpeg stderr unavailable")?;
    let read_audio = async {
        let mut buffer = vec![0; output_rate as usize / 10 * 8];
        loop {
            let mut filled = 0;
            while filled < buffer.len() {
                let count = tokio::time::timeout(
                    Duration::from_secs(25),
                    stdout.read(&mut buffer[filled..]),
                )
                .await
                .context("YouTube audio stalled; press Space to reconnect")??;
                if count == 0 {
                    break;
                }
                filled += count;
            }
            if filled == 0 {
                break;
            }
            ensure!(filled % 8 == 0, "FFmpeg returned incomplete stereo audio");
            let pcm = buffer[..filled]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| crate::audio_output::pcm_sample(*bytes))
                .collect();
            if chunks.send(pcm).await.is_err() {
                return Ok(());
            }
            if filled < buffer.len() {
                break;
            }
        }
        Ok::<_, anyhow::Error>(())
    };
    tokio::try_join!(read_audio, super::bounded_read(stderr, 64 * 1024))?;
    let status = child.wait().await?;
    // The upstream stderr contains signed stream URLs. Do not export it.
    ensure!(
        status.success(),
        "YouTube audio connection failed. Press Space to resolve a fresh stream, or update the tools with 'tuitify youtube setup'."
    );
    Ok(())
}

pub(crate) async fn worker(
    tools: Tools,
    mut volume: u8,
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<Event>,
    visualizer: Arc<AudioVisualizer>,
) -> Result<()> {
    let (updates, mut update_rx) = mpsc::unbounded_channel();
    let mut active: Option<Active> = None;
    let mut preparer = Preparer::default();
    let mut request = 0u64;
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
                        let prepared = preparer.take(&id);
                        Some((Intent { id, base_ms: position_ms, generation, paused: false }, prepared))
                    },
                    Command::Seek(position) => active.as_ref().map(|current| (Intent { id: current.id.clone(), base_ms: position, generation: current.generation, paused: current.paused }, current.stream.clone().filter(Stream::fresh).map(|stream| Prepared::Ready(Box::new(stream))))),
                    Command::Pause => {
                        if let Some(current) = &mut active {
                            current.paused = true;
                            current.sink.pause();
                            let _ = events.send(Event::Paused { generation: current.generation, position_ms: current.position() });
                        }
                        None
                    }
                    Command::Resume => {
                        if let Some(current) = &mut active {
                            current.paused = false;
                            current.sink.play();
                            if current.started { let _ = events.send(Event::Playing { generation: current.generation, position_ms: current.position() }); }
                        }
                        None
                    }
                    Command::Volume(value) => {
                        volume = value.min(100);
                        if let Some(current) = &active { current.sink.set_volume(f32::from(volume) / 100.0); }
                        let _ = events.send(Event::Volume(volume));
                        None
                    }
                    Command::Stop => { active = None; preparer.cancel(); None }
                    Command::CancelPreload => { preparer.cancel(); None }
                    Command::Preload { id } => {
                        if active.is_some() { preparer.request(&tools, id); }
                        None
                    },
                    #[cfg(test)]
                    Command::SimulateDisconnect => { active = None; None }
                };
                if let Some((intent, cached)) = intent {
                    active = None;
                    request = request.wrapping_add(1);
                    let generation = intent.generation;
                    match start(&tools, intent, request, volume, cached, &updates, &visualizer) {
                        Ok(current) => active = Some(current),
                        Err(error) => { let _ = events.send(Event::TrackError { generation, message: format!("{error:#}") }); }
                    }
                }
            }
            Some(update) = update_rx.recv() => {
                match update {
                    Update::Resolved(ticket, stream) if active.as_ref().is_some_and(|current| current.request == ticket) => {
                        if let Some(current) = &mut active {
                            let _ = events.send(Event::Metadata { generation: current.generation, track: stream.track.clone() });
                            let _ = events.send(Event::Quality { generation: current.generation, quality: stream.quality });
                            preparer.remember((*stream).clone());
                            current.stream = Some(*stream);
                        }
                    }
                    Update::Failed(ticket, message) if active.as_ref().is_some_and(|current| current.request == ticket) => {
                        let generation = active.as_ref().map_or(0, |current| current.generation);
                        if let Some(current) = &active { preparer.forget(&current.id); }
                        active = None;
                        let _ = events.send(Event::TrackError { generation, message });
                    }
                    _ => (),
                }
            }
            _ = tick.tick(), if active.is_some() || preparer.is_pending() => {
                preparer.collect().await;
                if let Some(current) = &mut active {
                    let position_ms = current.position();
                    if !current.started && current.samples.load(Ordering::Relaxed) > 0 {
                        current.started = true;
                        let _ = events.send(if current.paused {
                            Event::Paused { generation: current.generation, position_ms }
                        } else {
                            Event::Playing { generation: current.generation, position_ms }
                        });
                        let _ = events.send(Event::TimeToPreload { generation: current.generation });
                    }
                    if current.started && current.sink.empty() && current.job.is_finished() {
                        let generation = current.generation;
                        preparer.forget(&current.id);
                        let _ = events.send(Event::Position { generation, position_ms });
                        active = None;
                        let _ = events.send(Event::Completed(generation));
                    } else if current.started && !current.paused {
                        let _ = events.send(Event::Position { generation: current.generation, position_ms });
                    } else if !current.started && current.sink.empty() && current.job.is_finished() {
                        let generation = current.generation;
                        active = None;
                        let _ = events.send(Event::TrackError { generation, message: "YouTube returned an empty audio stream; choose another video".into() });
                    }
                }
            }
        }
    }
    drop(active);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "Installed FFmpeg; generated audio and loopback HTTP only, no Windows audio or saved state"]
    async fn interrupted_http_body_resumes_at_byte_offset_and_ends_once() -> Result<()> {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        let tools = Tools::discover()?;
        let directory = tempfile::tempdir()?;
        let file = directory.path().join("complete.flac");
        let generated = hidden_command(&tools.ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=3",
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
            "Could not generate FLAC fixture"
        );
        let body = std::fs::read(file)?;
        let cut = body.len() / 2;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (offset_tx, mut offsets) = mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            for attempt in 0..2 {
                let (mut socket, _) = listener.accept().await?;
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    if socket.read(&mut byte).await? == 0 {
                        break;
                    }
                    request.push(byte[0]);
                    ensure!(request.len() < 16_384, "Oversized fixture request");
                }
                let request = String::from_utf8(request)?.to_ascii_lowercase();
                let offset = request
                    .lines()
                    .find_map(|line| line.strip_prefix("range: bytes="))
                    .and_then(|range| range.split_once('-'))
                    .and_then(|(start, _)| start.parse::<usize>().ok())
                    .unwrap_or(0);
                let _ = offset_tx.send(offset);
                ensure!(
                    offset < body.len(),
                    "Unexpected request past the natural end"
                );
                let status = if offset == 0 {
                    "200 OK"
                } else {
                    "206 Partial Content"
                };
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: audio/flac\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nContent-Range: bytes {offset}-{}/{}\r\nConnection: close\r\n\r\n", body.len() - offset, body.len() - 1, body.len()).as_bytes()).await?;
                // Advertise the complete body, then drop the first connection.
                // A decoder-level reconnect must fetch precisely the remainder.
                let end = if attempt == 0 { cut } else { body.len() };
                socket.write_all(&body[offset..end]).await?;
                socket.shutdown().await?;
            }
            Ok::<_, anyhow::Error>(())
        });
        let stream = Stream {
            track: crate::model::Track {
                duration_ms: 3_000,
                ..Default::default()
            },
            quality: Default::default(),
            url: format!("http://{address}/audio"),
            headers: String::new(),
            obtained_at: std::time::Instant::now(),
        };
        let (tx, mut rx) = mpsc::channel::<Vec<f32>>(CAPACITY);
        let collect = async {
            let mut count = 0;
            while let Some(chunk) = rx.recv().await {
                count += chunk.len();
            }
            count
        };
        let result = tokio::time::timeout(Duration::from_secs(15), async {
            tokio::join!(decode(&tools, &stream, 48_000, 0, tx), collect)
        })
        .await;
        if result.is_err() {
            server.abort();
        }
        let (decoded, samples) = result.context("Interrupted HTTP fixture timed out")?;
        server.await??;
        decoded?;
        assert_eq!(samples, 48_000 * 2 * 3, "Audio was truncated or replayed");
        let mut received = Vec::new();
        while let Ok(offset) = offsets.try_recv() {
            received.push(offset);
        }
        assert_eq!(
            received,
            vec![0, cut],
            "Decoder did not resume at the interrupted byte"
        );
        Ok(())
    }

    #[test]
    fn pcm_stalls_do_not_advance_position_and_close_drains_audio() {
        let (tx, rx) = mpsc::channel(1);
        let samples = Arc::new(AtomicU64::new(0));
        let visualizer = AudioVisualizer::new();
        let mut source = Pcm::new(rx, samples.clone(), 48000, visualizer.clone());
        assert_eq!(source.next(), Some(0.0));
        assert_eq!(samples.load(Ordering::Relaxed), 0);
        tx.try_send(vec![0.2, 0.4]).unwrap();
        drop(tx);
        assert_eq!(source.next(), Some(0.2));
        assert_eq!(source.next(), Some(0.4));
        assert_eq!(source.next(), None);
        assert_eq!(samples.load(Ordering::Relaxed), 2);
        assert!(visualizer.has_audio_samples());
    }
}
