//! Small isolated ytmusicapi bridge; the Rust queue/shuffle remains the owner.
use super::{bounded_read, hidden_command, music_auth};
use crate::{
    catalog::{ArtistResultSource, Browse, Page, Rows},
    model::{Playlist, Track},
    service::{FailureKind, Provider, ServiceFailure},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

mod cache;
mod worker;
use cache::{Pages, ParsedRows, Snapshot};

const PAGE_SIZE: usize = 50;
const MAX_ROWS: usize = crate::queue::MAX_TRACKS;
const CACHE_TTL: Duration = Duration::from_secs(300);
const MAX_OUTPUT: u64 = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct Client {
    python: PathBuf,
    credentials: Option<Arc<music_auth::Credentials>>,
    cache: Arc<Mutex<Pages>>,
    cache_version: Arc<AtomicU64>,
    worker: Arc<Mutex<Option<worker::Worker>>>,
    #[cfg(test)]
    bridge: Option<String>,
}

impl Client {
    #[cfg(test)]
    pub(crate) fn mock(script: &str, connected: bool) -> Self {
        let python = std::env::var_os("TUITIFY_TEST_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("python.exe"));
        let credentials = connected.then(|| music_auth::Credentials::from_headers(&json!({"cookie":"__Secure-3PAPISID=fixture-private-cookie", "authorization":"SAPISIDHASH fixture-private-token"})).unwrap());
        Self {
            python,
            credentials: credentials.map(Arc::new),
            cache: Arc::new(Mutex::new(Pages::default())),
            cache_version: Arc::new(AtomicU64::new(0)),
            worker: Arc::new(Mutex::new(None)),
            bridge: Some(format!(
                "import io,json,sys\nscript={}\noriginal_in,original_out=sys.stdin,sys.stdout\nfor line in original_in:\n    sys.stdin=io.StringIO(line)\n    sys.stdout=io.StringIO()\n    try:\n        exec(script,{{'__name__':'fixture'}})\n    except SystemExit as error:\n        if error.code not in (None,0):\n            raise\n    output=sys.stdout.getvalue()\n    sys.stdin,sys.stdout=original_in,original_out\n    sys.stdout.write(output+'\\n')\n    sys.stdout.flush()\n",
                serde_json::to_string(script).unwrap()
            )),
        }
    }
    pub fn discover() -> Result<Option<Self>> {
        let python =
            PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?)
                .join("Programs/Tuitify/tools/ytmusic/Scripts/python.exe");
        if !python.is_file() {
            return Ok(None);
        }
        Ok(Some(Self {
            python,
            credentials: music_auth::load(&music_auth::credential_path()?)?.map(Arc::new),
            cache: Arc::new(Mutex::new(Pages::default())),
            cache_version: Arc::new(AtomicU64::new(0)),
            worker: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            bridge: None,
        }))
    }

    pub fn connected(&self) -> bool {
        self.credentials.is_some()
    }

    pub fn refresh(&self) {
        self.cache_version.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut cache) = self.cache.try_lock() {
            cache.clear();
        }
    }
    /// Release only the idle metadata helper; cached pages and audio continue.
    pub(crate) fn release_idle_helper(&self) {
        if let Ok(mut slot) = self.worker.try_lock()
            && slot
                .as_ref()
                .is_some_and(|worker| worker.last_used.elapsed() >= Duration::from_secs(30))
        {
            slot.take();
        }
    }

    async fn rpc(
        &self,
        mut request: Value,
        credentials: Option<&music_auth::Credentials>,
    ) -> Result<Value> {
        if let Some(credentials) = credentials {
            request["headers"] = serde_json::to_value(&credentials.headers)?;
        }
        let script = include_str!("music_bridge.py");
        #[cfg(test)]
        let script = self.bridge.as_deref().unwrap_or(script);
        let bytes = serde_json::to_vec(&request)?;
        ensure!(
            bytes.len() <= 128 * 1024,
            "YouTube Music request exceeded its size limit"
        );
        let mut slot = self.worker.lock().await;
        // Move the helper out of the shared slot: cancellation drops and kills it,
        // so the next request never consumes a response belonging to an old one.
        let mut worker = match slot.take() {
            Some(worker) => worker,
            None => worker::Worker::spawn(&self.python, script)?,
        };
        let value = tokio::time::timeout(Duration::from_secs(120), worker.exchange(&bytes))
            .await
            .map_err(|_| failure(FailureKind::Transport))??;
        *slot = Some(worker);
        drop(slot);
        if value["ok"].as_bool() != Some(true) {
            let kind = match value["error"].as_str() {
                Some("authentication") => FailureKind::AuthenticationRequired,
                Some("restricted") => FailureKind::AccessRestricted,
                Some("missing") => FailureKind::MissingItem,
                Some("rate_limit") => FailureKind::RateLimited,
                Some("transport") => FailureKind::Transport,
                Some("setup") => {
                    anyhow::bail!(
                        "YouTube Music dependencies are missing; run 'tuitify youtube music-setup'"
                    );
                }
                _ => FailureKind::InvalidResponse,
            };
            return Err(failure(kind).into());
        }
        ensure!(
            value["items"].is_array() && value["complete"].is_boolean(),
            "YouTube Music returned invalid catalog data; update with 'tuitify youtube music-setup'"
        );
        Ok(value)
    }

    pub async fn validate_credentials(&self, credentials: &music_auth::Credentials) -> Result<()> {
        self.rpc(json!({"operation":"account"}), Some(credentials))
            .await?;
        Ok(())
    }

    fn auth(&self) -> Option<&music_auth::Credentials> {
        self.credentials.as_deref()
    }

    pub async fn recommendations(
        &self,
        seed: &Track,
        excluded: &[Track],
        round: usize,
    ) -> Result<Vec<Track>> {
        let id = super::video_key(&seed.id).context("Invalid YouTube Music seed")?;
        let value = self
            .rpc(
                json!({"operation":"recommendations", "id":id,"limit":100}),
                self.auth(),
            )
            .await?;
        let mut seen = std::collections::HashSet::new();
        let mut tracks: Vec<_> = value["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(parse_track)
            .filter(|track| {
                track.playable
                    && track.id != seed.id
                    && !excluded.iter().any(|known| known.id == track.id)
                    && seen.insert(track.id.clone())
            })
            .collect();
        // Keep subsequent radio refills anchored to the same music seed.
        let shift = round.saturating_mul(20).min(tracks.len());
        tracks.drain(..shift);
        tracks.truncate(30);
        Ok(tracks)
    }

    pub async fn track(&self, id: &str) -> Result<Track> {
        let video = super::video_key(id).context("Invalid YouTube track ID")?;
        let value = self
            .rpc(json!({"operation":"track", "id":video,"limit":5}), None)
            .await?;
        value["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(parse_track)
            .find(|track| track.id == id)
            .ok_or_else(|| failure(FailureKind::MissingItem).into())
    }

    pub async fn page(&self, browse: &Browse, offset: usize) -> Result<Page> {
        ensure!(
            offset < MAX_ROWS,
            "YouTube Music catalog limit reached (100,000 rows)"
        );
        if matches!(browse, Browse::Playlists | Browse::Liked) && !self.connected() {
            return Err(failure(FailureKind::AuthenticationRequired).into());
        }
        let (key, mut request) = match browse {
            Browse::Playlists => ("playlists".into(), json!({"operation":"playlists"})),
            Browse::Liked => ("liked".into(), json!({"operation":"liked"})),
            Browse::Playlist(id) => {
                let id = playlist_key(id).context("Invalid YouTube Music playlist ID")?;
                (
                    format!("playlist:{id}"),
                    json!({"operation":"playlist","id":id}),
                )
            }
            Browse::Album(id) => {
                let id = entity_key(id, "album").context("Invalid YouTube Music album ID")?;
                (format!("album:{id}"), json!({"operation":"album","id":id}))
            }
            Browse::Artist(id) => {
                let id = entity_key(id, "artist").context("Invalid YouTube Music artist ID")?;
                (
                    format!("artist:{id}"),
                    json!({"operation":"artist","id":id}),
                )
            }
            Browse::Search(query) => {
                ensure!(
                    query.chars().count() <= 300
                        && !query.contains("://")
                        && !query.starts_with("spotify:")
                        && !query.starts_with("youtube:"),
                    "Enter a song/artist name or paste a valid YouTube video/playlist link"
                );
                (
                    format!("search:{query}"),
                    json!({"operation":"search","query":query}),
                )
            }
        };
        let version = self.cache_version.load(Ordering::Acquire);
        let mut cache = self.cache.lock().await;
        let page_size = if matches!(browse, Browse::Search(_)) {
            20
        } else {
            PAGE_SIZE
        };
        let end = (offset + page_size).min(MAX_ROWS);
        let previous = cache.get(&key, version);
        drop(cache);
        let fetch = previous
            .as_ref()
            .is_none_or(|snapshot| !snapshot.complete && snapshot.rows.len() < end);
        let snapshot = if fetch {
            // Search can return its first batch without fetching another batch
            // solely to prove that the next page exists.
            let minimum = end + usize::from(!matches!(browse, Browse::Search(_)));
            let limit = previous
                .as_ref()
                .map_or(minimum, |snapshot| (snapshot.limit * 2).max(minimum))
                .min(MAX_ROWS + 1);
            request["limit"] = limit.into();
            // Public catalog browsing must also work with an expired optional
            // Google connection. Only library/playlist reads carry credentials.
            let credentials = if matches!(
                browse,
                Browse::Playlists | Browse::Liked | Browse::Playlist(_)
            ) {
                self.auth()
            } else {
                None
            };
            let value = self.rpc(request, credentials).await?;
            let items = value["items"].as_array().unwrap();
            let items = &items[..items.len().min(MAX_ROWS + 1)];
            let snapshot = Arc::new(Snapshot {
                rows: ParsedRows::parse(items, matches!(browse, Browse::Playlists)),
                complete: value["complete"].as_bool().unwrap(),
                limit,
                at: Instant::now(),
                version,
            });
            let mut cache = self.cache.lock().await;
            if self.cache_version.load(Ordering::Acquire) == version {
                cache.insert(key, snapshot.clone());
            }
            snapshot
        } else {
            previous.context("YouTube Music catalog cache unavailable")?
        };
        let rows = snapshot.rows.page(offset, page_size);
        let next = (snapshot.rows.len() > end || !snapshot.complete).then_some(end);
        Ok(Page {
            rows,
            offset,
            next,
            artist_source: matches!(browse, Browse::Artist(_))
                .then_some(ArtistResultSource::YoutubeMusic),
        })
    }
}

fn failure(kind: FailureKind) -> ServiceFailure {
    ServiceFailure::new(Provider::YoutubeMusic, kind)
}

fn safe_key(input: &str) -> bool {
    (2..=200).contains(&input.len())
        && input
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn entity_key<'a>(input: &'a str, kind: &str) -> Option<&'a str> {
    input
        .strip_prefix(&format!("youtube:{kind}:"))
        .filter(|key| safe_key(key))
}

pub fn playlist_key(input: &str) -> Option<&str> {
    entity_key(input, "playlist")
}

pub fn playlist_id(input: &str) -> Option<String> {
    if let Some(key) = playlist_key(input) {
        return Some(format!("youtube:playlist:{key}"));
    }
    let input = input.trim();
    let normalized = if input.starts_with("https://") || input.starts_with("http://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let url = url::Url::parse(&normalized).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !matches!(
            url.host_str(),
            Some("music.youtube.com" | "www.youtube.com" | "youtube.com" | "m.youtube.com")
        )
        || url.path() != "/playlist"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let key = url.query_pairs().find(|(name, _)| name == "list")?.1;
    safe_key(&key).then(|| format!("youtube:playlist:{key}"))
}

fn parse_playlist(value: &Value) -> Option<Playlist> {
    let key = value["playlistId"].as_str().filter(|key| safe_key(key))?;
    let name = value["title"].as_str()?.chars().take(500).collect();
    let owner = value["author"]
        .as_str()
        .or_else(|| value["author"]["name"].as_str())
        .or_else(|| value["author"][0]["name"].as_str())
        .unwrap_or("YouTube Music")
        .chars()
        .take(500)
        .collect();
    Some(Playlist {
        id: format!("youtube:playlist:{key}"),
        name,
        owner,
    })
}

fn duration(value: &Value) -> u32 {
    if let Some(seconds) = value["duration_seconds"]
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0)
    {
        return (seconds * 1000.0).min(u32::MAX as f64) as u32;
    }
    let text = value["duration"]
        .as_str()
        .or_else(|| value["length"].as_str())
        .unwrap_or("");
    text.split(':')
        .try_fold(0u32, |seconds, part| {
            seconds
                .checked_mul(60)?
                .checked_add(part.parse::<u32>().ok()?)
        })
        .unwrap_or(0)
        .saturating_mul(1000)
}

fn parse_track(value: &Value) -> Option<Track> {
    let video = value["videoId"]
        .as_str()
        .filter(|video| super::valid_video_id(video))?;
    let name = value["title"].as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let artists = value["artists"].as_array();
    let artist_ids = artists
        .into_iter()
        .flatten()
        .filter_map(|artist| artist["id"].as_str().filter(|key| safe_key(key)))
        .map(|key| format!("youtube:artist:{key}"))
        .collect();
    let names: Vec<_> = artists
        .into_iter()
        .flatten()
        .filter_map(|artist| artist["name"].as_str())
        .take(8)
        .collect();
    let album_id = value["album"]["id"]
        .as_str()
        .filter(|key| safe_key(key))
        .map(|key| format!("youtube:album:{key}"));
    Some(Track {
        id: format!("youtube:{video}"),
        name: name.chars().take(500).collect(),
        artists: names.join(", ").chars().take(500).collect(),
        artist_ids,
        album: value["album"]["name"]
            .as_str()
            .map(|name| name.chars().take(500).collect()),
        album_id,
        duration_ms: duration(value),
        playable: value["isAvailable"].as_bool() != Some(false),
        music_metadata: true,
        album_art_url: Some(format!("https://i.ytimg.com/vi/{video}/hqdefault.jpg")),
        ..Track::default()
    })
}

pub async fn setup() -> Result<()> {
    let dir = tempfile::Builder::new()
        .prefix("tuitify-music-setup-")
        .tempdir()?;
    let script = dir.path().join("setup.ps1");
    std::fs::write(&script, include_bytes!("setup-music.ps1"))?;
    std::fs::write(
        dir.path().join("music-requirements.txt"),
        include_bytes!("music-requirements.txt"),
    )?;
    let status = hidden_command("powershell.exe")
        .env_remove("PSModulePath")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;
    ensure!(status.success(), "YouTube Music setup failed");
    Ok(())
}

#[cfg(test)]
mod tests;
