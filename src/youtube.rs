//! Optional YouTube provider. Tools run directly with bounded output and no shell.
use crate::{
    catalog::{Browse, Page, Rows},
    model::Track,
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

pub(crate) mod music;
pub(crate) mod music_auth;
pub(crate) mod playback;
#[cfg(test)]
mod tests;

const PAGE_SIZE: usize = 20;
const MAX_SEARCH_RESULTS: usize = 200;
const MAX_OUTPUT: u64 = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);

fn valid_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Stored identity; unqualified IDs must not be mistaken for Spotify IDs.
pub fn video_key(id: &str) -> Option<&str> {
    id.strip_prefix("youtube:")
        .filter(|video| valid_video_id(video))
}

/// Accept only known YouTube hosts and video paths. Never forward arbitrary URLs.
pub fn video_id(input: &str) -> Option<String> {
    let input = input.trim();
    if let Some(id) = video_key(input) {
        return Some(format!("youtube:{id}"));
    }
    if valid_video_id(input) {
        return Some(format!("youtube:{input}"));
    }
    let normalized = if input.starts_with("https://") || input.starts_with("http://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let url = url::Url::parse(&normalized).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let id = match url.host_str()? {
        "youtu.be" | "www.youtu.be" => url.path().trim_matches('/').to_owned(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            if url.path() == "/watch" {
                url.query_pairs()
                    .find(|(key, _)| key == "v")?
                    .1
                    .into_owned()
            } else {
                let mut parts = url.path().trim_matches('/').split('/');
                if !matches!(parts.next()?, "shorts" | "embed" | "live") {
                    return None;
                }
                let id = parts.next()?.to_owned();
                if parts.next().is_some() {
                    return None;
                }
                id
            }
        }
        _ => return None,
    };
    valid_video_id(&id).then(|| format!("youtube:{id}"))
}

pub(crate) fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    command.kill_on_drop(true).stdin(Stdio::null());
    command
}

fn locate(name: &str) -> Result<PathBuf> {
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?)
        .join("Programs/Tuitify/tools");
    let local = root.join(name);
    if local.is_file() {
        return Ok(local);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            if directory.as_os_str().is_empty() {
                continue;
            }
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    bail!("{name} is missing. Run 'tuitify youtube setup' to install the YouTube tools.")
}

#[derive(Clone)]
pub struct Tools {
    pub(crate) music: Option<music::Client>,
    pub(crate) ytdlp: PathBuf,
    pub(crate) deno: PathBuf,
    pub(crate) ffmpeg: PathBuf,
    #[cfg(test)]
    prefix: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct Stream {
    pub track: Track,
    url: String,
    headers: String,
    obtained_at: std::time::Instant,
}

impl Stream {
    fn fresh(&self) -> bool {
        if self.obtained_at.elapsed() >= Duration::from_secs(300) {
            return false;
        }
        let expires = url::Url::parse(&self.url).ok().and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "expire")
                .and_then(|(_, value)| value.parse::<u64>().ok())
        });
        expires.is_none_or(|expires| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .is_ok_and(|now| now.as_secs().saturating_add(30) < expires)
        })
    }
}

impl Tools {
    #[cfg(test)]
    pub(crate) fn music_fixture(client: music::Client) -> Self {
        Self {
            music: Some(client),
            ytdlp: PathBuf::from("unused-ytdlp.exe"),
            deno: PathBuf::from("unused-deno.exe"),
            ffmpeg: PathBuf::from("unused-ffmpeg.exe"),
            prefix: Vec::new(),
        }
    }
    pub fn discover() -> Result<Self> {
        Ok(Self {
            music: music::Client::discover()?,
            ytdlp: locate("yt-dlp.exe")?,
            deno: locate("deno.exe")?,
            ffmpeg: locate("ffmpeg.exe")?,
            #[cfg(test)]
            prefix: Vec::new(),
        })
    }

    pub async fn doctor(&self) -> Result<()> {
        for (name, program, argument) in [
            ("yt-dlp", &self.ytdlp, "--version"),
            ("Deno", &self.deno, "--version"),
            ("FFmpeg", &self.ffmpeg, "-version"),
        ] {
            let output = tokio::time::timeout(
                Duration::from_secs(15),
                hidden_command(program).arg(argument).output(),
            )
            .await
            .with_context(|| format!("{name} version check timed out"))??;
            ensure!(
                output.status.success(),
                "{name} failed its version check; run 'tuitify youtube setup'"
            );
            let text = String::from_utf8_lossy(&output.stdout);
            println!(
                "{name}: {} ({})",
                text.lines().next().unwrap_or("available"),
                program.display()
            );
        }
        println!("No Spotify credentials or YouTube account required for public videos.");
        if let Some(music) = &self.music {
            println!(
                "YouTube Music: installed; Google account {}",
                if music.connected() {
                    "connected (run 'tuitify youtube playlists' to check access)"
                } else {
                    "not connected (run 'tuitify youtube login')"
                }
            );
        } else {
            println!(
                "YouTube Music library: run 'tuitify youtube music-setup' then 'tuitify youtube login'."
            );
        }
        Ok(())
    }

    async fn json(&self, args: &[String]) -> Result<Vec<Value>> {
        self.json_with_timeout(args, REQUEST_TIMEOUT).await
    }

    async fn json_with_timeout(&self, args: &[String], timeout: Duration) -> Result<Vec<Value>> {
        let mut command = hidden_command(&self.ytdlp);
        #[cfg(test)]
        command.args(&self.prefix);
        command
            .args([
                "--ignore-config",
                "--no-warnings",
                "--no-progress",
                "--skip-download",
                "--socket-timeout",
                "15",
                "--retries",
                "1",
                "--extractor-retries",
                "1",
                "--no-playlist",
                "--js-runtimes",
            ])
            .arg(format!("deno:{}", self.deno.display()))
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .context("Could not start yt-dlp; run 'tuitify youtube setup'")?;
        let stdout = child.stdout.take().context("yt-dlp stdout unavailable")?;
        let stderr = child.stderr.take().context("yt-dlp stderr unavailable")?;
        let result = tokio::time::timeout(timeout, async {
            let (output, errors) = tokio::try_join!(bounded_read(stdout, MAX_OUTPUT), bounded_read(stderr, 64 * 1024))?;
            let status = child.wait().await?;
            ensure!(status.success(), "{}", tool_error(&errors));
            let text = std::str::from_utf8(&output).context("YouTube returned invalid UTF-8")?;
            text.lines().filter(|line| !line.trim().is_empty()).map(|line|
                serde_json::from_str(line).context("YouTube returned invalid metadata; run 'tuitify youtube setup' to update yt-dlp")
            ).collect::<Result<Vec<_>>>()
        }).await;
        // Explicitly reap a timeout/limit failure, and kill_on_drop covers cancellation.
        match result {
            Ok(Ok(values)) => Ok(values),
            Ok(Err(error)) => {
                let _ = child.kill().await;
                Err(error)
            }
            Err(_) => {
                let _ = child.kill().await;
                bail!("YouTube request timed out. Check your connection and retry with F5.")
            }
        }
    }

    pub async fn track(&self, id: &str) -> Result<Track> {
        if let Some(music) = &self.music
            && let Ok(track) = music.track(id).await
        {
            return Ok(track);
        }
        let video = video_key(id).context("Invalid YouTube track ID")?;
        let values = self
            .json(&[
                "--dump-json".into(),
                "--".into(),
                format!("https://www.youtube.com/watch?v={video}"),
            ])
            .await?;
        let track = values
            .first()
            .and_then(parse_track)
            .context("YouTube returned no usable video metadata")?;
        ensure!(track.id == id, "YouTube returned a different video");
        Ok(track)
    }

    pub async fn page(&self, browse: &Browse, offset: usize) -> Result<Page> {
        if let Some(music) = &self.music {
            if let Browse::Search(query) = browse {
                if let Some(id) = music::playlist_id(query) {
                    return music.page(&Browse::Playlist(id), offset).await;
                }
                if let Some(id) = video_id(query) {
                    return Ok(Page {
                        rows: Rows::Tracks(if offset == 0 {
                            vec![self.track(&id).await?]
                        } else {
                            Vec::new()
                        }),
                        offset,
                        next: None,
                        artist_source: None,
                    });
                }
                if query.trim().is_empty() {
                    return Ok(Page {
                        rows: Rows::Tracks(Vec::new()),
                        offset,
                        next: None,
                        artist_source: None,
                    });
                }
            }
            return music.page(browse, offset).await;
        }
        let Browse::Search(query) = browse else {
            bail!(
                "YouTube mode supports search and the queue. Spotify library, album and artist views require Spotify mode."
            );
        };
        let query = query.trim();
        let (mut tracks, has_more) = if query.is_empty() {
            (Vec::new(), false)
        } else if let Some(id) = video_id(query) {
            (
                if offset == 0 {
                    vec![self.track(&id).await?]
                } else {
                    Vec::new()
                },
                false,
            )
        } else {
            ensure!(
                !query.contains("://")
                    && !query.starts_with("spotify:")
                    && !query.starts_with("youtube:")
                    && !query.starts_with("youtu.be/")
                    && !query.starts_with("www.youtube.com/")
                    && !query.starts_with("youtube.com/"),
                "Paste a valid YouTube video link, or search by song/artist name."
            );
            ensure!(
                query.chars().count() <= 300,
                "YouTube search is limited to 300 characters"
            );
            ensure!(
                offset < MAX_SEARCH_RESULTS,
                "YouTube search is limited to {MAX_SEARCH_RESULTS} results; refine your query"
            );
            let end = (offset + PAGE_SIZE + 1).min(MAX_SEARCH_RESULTS);
            let values = self
                .json(&[
                    "--flat-playlist".into(),
                    "--dump-json".into(),
                    "--playlist-start".into(),
                    (offset + 1).to_string(),
                    "--playlist-end".into(),
                    end.to_string(),
                    "--".into(),
                    format!("ytsearch{end}:{query}"),
                ])
                .await?;
            let has_more = values.len() > PAGE_SIZE && offset + PAGE_SIZE < MAX_SEARCH_RESULTS;
            (
                values.iter().filter_map(parse_track).collect::<Vec<_>>(),
                has_more,
            )
        };
        tracks.truncate(PAGE_SIZE);
        Ok(Page {
            rows: Rows::Tracks(tracks),
            offset,
            next: has_more.then_some(offset + PAGE_SIZE),
            artist_source: None,
        })
    }

    pub(crate) async fn resolve(&self, id: &str) -> Result<Stream> {
        let video = video_key(id).context("Invalid YouTube track ID")?;
        let values = self
            .json(&[
                "--dump-json".into(),
                "--format".into(),
                "bestaudio[protocol=https]/bestaudio".into(),
                "--".into(),
                format!("https://www.youtube.com/watch?v={video}"),
            ])
            .await?;
        let value = values.first().context("YouTube returned no audio stream")?;
        let track = parse_track(value).context("YouTube returned no usable video metadata")?;
        ensure!(
            track.id == id && track.playable,
            "This YouTube video is unavailable or is a live stream"
        );
        let stream_url = value["url"].as_str().context("YouTube returned no direct audio stream; update the tools with 'tuitify youtube setup'")?;
        validate_stream_url(stream_url)?;
        let mut headers = String::new();
        if let Some(values) = value["http_headers"].as_object() {
            for (name, value) in values {
                if let Some(value) = value.as_str()
                    && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    && !value.contains(['\r', '\n', '\0'])
                    && value.len() < 4096
                {
                    headers.push_str(&format!("{name}: {value}\r\n"));
                }
            }
        }
        Ok(Stream {
            track,
            url: stream_url.to_owned(),
            headers,
            obtained_at: std::time::Instant::now(),
        })
    }
}

async fn bounded_read(reader: impl tokio::io::AsyncRead + Unpin, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes).await?;
    ensure!(
        bytes.len() as u64 <= limit,
        "YouTube tool response exceeded the size limit"
    );
    Ok(bytes)
}

fn validate_stream_url(input: &str) -> Result<()> {
    let url = url::Url::parse(input).context("YouTube returned an invalid stream URL")?;
    ensure!(
        url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|host| host.ends_with(".googlevideo.com"))
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none(),
        "YouTube returned an unsupported audio stream host"
    );
    Ok(())
}

fn tool_error(stderr: &[u8]) -> &'static str {
    // Never show upstream URLs, signed tokens, cookies, or a raw diagnostic body.
    let message = String::from_utf8_lossy(stderr).to_lowercase();
    if message.contains("429") || message.contains("too many requests") {
        "YouTube is rate limiting this connection (HTTP 429). Wait before retrying."
    } else if message.contains("confirm") && message.contains("bot")
        || message.contains("sign in")
        || message.contains("sign-in")
    {
        "YouTube requires sign-in or blocked this connection. Try another public video or connection; Tuitify does not read browser cookies."
    } else if message.contains("private video")
        || message.contains("video unavailable")
        || message.contains("removed")
        || message.contains("not available")
    {
        "This YouTube video is private, removed, or unavailable in your region. Choose another video."
    } else {
        "YouTube request failed. Check your connection; F6 > Update playback tools can repair outdated dependencies."
    }
}

fn parse_track(value: &Value) -> Option<Track> {
    let video = value["id"].as_str().filter(|id| valid_video_id(id))?;
    let name = value["track"]
        .as_str()
        .or_else(|| value["title"].as_str())?
        .trim();
    if name.is_empty() {
        return None;
    }
    let artists = value["artist"]
        .as_str()
        .or_else(|| value["creator"].as_str())
        .or_else(|| value["channel"].as_str())
        .or_else(|| value["uploader"].as_str())
        .unwrap_or("YouTube");
    let duration = value["duration"]
        .as_f64()
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(0.0);
    let unavailable = matches!(
        value["availability"].as_str(),
        Some("private" | "premium_only" | "subscriber_only" | "needs_auth")
    );
    let live = value["is_live"].as_bool() == Some(true)
        || matches!(
            value["live_status"].as_str(),
            Some("is_live" | "is_upcoming")
        );
    Some(Track {
        id: format!("youtube:{video}"),
        name: name.chars().take(500).collect(),
        artists: artists
            .trim()
            .trim_end_matches(" - Topic")
            .chars()
            .take(500)
            .collect(),
        duration_ms: (duration * 1000.0).min(u32::MAX as f64) as u32,
        playable: !unavailable && !live,
        album: value["album"].as_str().map(str::to_owned),
        album_art_url: Some(format!("https://i.ytimg.com/vi/{video}/hqdefault.jpg")),
        ..Track::default()
    })
}

pub async fn setup() -> Result<()> {
    let mut script = tempfile::Builder::new()
        .prefix("tuitify-youtube-")
        .suffix(".ps1")
        .tempfile()?;
    use std::io::Write;
    script.write_all(include_bytes!("youtube/setup.ps1"))?;
    script.flush()?;
    // Windows PowerShell opens scripts exclusively. Close the writer while
    // retaining automatic cleanup for the path until the child exits.
    let script_path = script.into_temp_path();
    let status = hidden_command("powershell.exe")
        // A Rust child of PowerShell 7 can inherit its incompatible module path.
        // Let Windows PowerShell discover its own built-in modules.
        .env_remove("PSModulePath")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script_path.as_os_str())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;
    ensure!(
        status.success(),
        "YouTube tool setup failed; existing tools were preserved where possible"
    );
    Tools::discover()?.doctor().await
}

/// First launch prepares public music search/radio as well as the audio tools.
/// Existing installations are reused; account login remains optional.
pub(crate) async fn prepare_music() -> Result<()> {
    if ["yt-dlp.exe", "deno.exe", "ffmpeg.exe"]
        .iter()
        .any(|name| locate(name).is_err())
    {
        println!("Preparing YouTube playback tools for this Windows user...");
        setup().await?;
    }
    if music::Client::discover()?.is_none() {
        println!("Preparing YouTube Music search and radio (Python 3.10+)...");
        music::setup().await?;
    }
    Tools::discover()?;
    Ok(())
}

pub async fn probe(input: &str, seconds: u8, volume: u8) -> Result<()> {
    let id = video_id(input).context("Supply a YouTube video link or 11-character video ID")?;
    let visualizer = crate::visualizer::AudioVisualizer::new();
    let mut playback =
        crate::playback::Playback::spawn_youtube(Tools::discover()?, volume, visualizer.clone());
    playback.commands.send(crate::playback::Command::Load {
        id,
        position_ms: 0,
        generation: 1,
    })?;
    let result = tokio::time::timeout(Duration::from_secs(120), async {
        let mut playing_at = None;
        let mut latest_position = 0;
        loop {
            let wait = playing_at.map_or(Duration::from_secs(120), |at: std::time::Instant| Duration::from_secs(u64::from(seconds)).saturating_sub(at.elapsed()));
            tokio::select! {
                event = playback.events.recv() => match event.context("YouTube playback worker exited")? {
                    crate::playback::Event::Metadata { track, .. } => println!("{} — {} ({:.1}s)", track.artists, track.name, f64::from(track.duration_ms) / 1000.0),
                    crate::playback::Event::Playing { position_ms, .. } => { playing_at = Some(std::time::Instant::now()); latest_position = position_ms; println!("Real PCM playback started (volume {volume}%)."); },
                    crate::playback::Event::Position { position_ms, .. } => latest_position = position_ms,
                    crate::playback::Event::Error(message) | crate::playback::Event::TrackError { message, .. } => bail!("{message}"),
                    crate::playback::Event::Completed(_) => break,
                    _ => (),
                },
                _ = tokio::time::sleep(wait), if playing_at.is_some() => break,
            }
        }
        ensure!(visualizer.has_audio_samples(), "No decoded audio samples reached the output");
        println!("Audio verified: decoded samples reached Windows output and the visualizer; position {latest_position}ms.");
        Ok(())
    }).await.context("YouTube audio probe timed out")?;
    let _ = playback.commands.send(crate::playback::Command::Stop);
    result
}
