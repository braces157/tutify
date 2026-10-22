//! Match catalog recordings to Monochrome's public track streamer.
//! Queue identities and discovery metadata always stay with the original catalog.
use crate::{
    audio_quality::{AudioQuality, Codec},
    catalog::Catalog,
    model::Track,
    service::{FailureKind, Provider, ServiceFailure},
    volta::Audio,
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const BASE: &str = "https://tracks.monochrome.st";
const AGENT: &str = concat!("Tuitify/", env!("CARGO_PKG_VERSION"));
type Matches = HashMap<String, (String, Instant)>;

#[derive(Clone)]
pub(crate) struct Client {
    http: reqwest::Client,
    base: String,
    cooldown: Arc<Mutex<Option<ServiceFailure>>>,
    matches: Arc<Mutex<Matches>>,
}

impl Client {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(8))
                .redirect(reqwest::redirect::Policy::limited(3))
                .user_agent(AGENT)
                .build()?,
            base: BASE.into(),
            cooldown: Arc::new(Mutex::new(None)),
            matches: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    #[cfg(test)]
    fn mock(base: &str) -> Self {
        Self {
            base: base.into(),
            ..Self::new().unwrap()
        }
    }

    fn failure(kind: FailureKind) -> anyhow::Error {
        ServiceFailure::new(Provider::Monochrome, kind).into()
    }

    async fn checked(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        if let Some(failure) = self
            .cooldown
            .lock()
            .await
            .filter(|failure| failure.active())
        {
            return Err(failure.into());
        }
        let response = request
            .send()
            .await
            .map_err(|_| Self::failure(FailureKind::Transport))?;
        if response.status().is_success() {
            return Ok(response);
        }
        let failure = ServiceFailure::from_response(response, Provider::Monochrome).await;
        if failure.throttled() {
            *self.cooldown.lock().await = Some(failure);
        }
        Err(failure.into())
    }

    async fn recording_id(&self, track: &Track, isrc: Option<&str>) -> Result<String> {
        if let Some((id, at)) = self.matches.lock().await.get(&track.id)
            && at.elapsed() < Duration::from_secs(6 * 60 * 60)
        {
            return Ok(id.clone());
        }
        let mut response = self
            .checked(
                self.http
                    .get(format!("{}/search/tracks", self.base))
                    .query(&[
                        ("q", format!("{} {}", track.artists, track.name)),
                        ("limit", "6".into()),
                    ]),
            )
            .await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Self::failure(FailureKind::Transport))?
        {
            ensure!(
                bytes.len() + chunk.len() <= 2 * 1024 * 1024,
                "Monochrome search response exceeded the size limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Self::failure(FailureKind::InvalidResponse))?;
        let rows = value["tracks"]
            .as_array()
            .or_else(|| value["tracks"]["items"].as_array())
            .context("Monochrome returned no track list")?;
        let candidates: Vec<_> = rows.iter().filter_map(parse_track).collect();
        let exact = isrc.and_then(|code| {
            rows.iter()
                .filter_map(|raw| Some((raw, parse_track(raw)?)))
                .find(|(raw, candidate)| {
                    raw["isrc"]
                        .as_str()
                        .and_then(crate::deezer::isrc)
                        .as_deref()
                        == Some(code)
                        && candidate.playable
                        && crate::volta::matching::compatible_duration(track, candidate, true)
                })
                .map(|(_, candidate)| candidate.id)
        });
        let id = if let Some(id) = exact {
            id
        } else {
            // A known recording code must not silently select a different master.
            ensure!(
                isrc.is_none(),
                "No matching Monochrome recording is available"
            );
            crate::volta::matching::select(track, &candidates)
                .context("No unambiguous Monochrome recording matches this song")?
        };
        let mut matches = self.matches.lock().await;
        if matches.len() >= 512 {
            matches.clear();
        }
        matches.insert(track.id.clone(), (id.clone(), Instant::now()));
        Ok(id)
    }

    async fn stream(&self, id: &str) -> Result<Audio> {
        ensure!(valid_id(id), "Invalid Monochrome track ID");
        let url = format!("{}/track/{id}", self.base);
        // Read only STREAMINFO to catch HTTP failures and discover the real input
        // format before opening the output device. FFmpeg then owns playback.
        let mut response = self
            .checked(
                self.http
                    .get(&url)
                    .header(reqwest::header::RANGE, "bytes=0-41"),
            )
            .await?;
        let mut header = Vec::with_capacity(42);
        while header.len() < 42 {
            let chunk = response
                .chunk()
                .await
                .map_err(|_| Self::failure(FailureKind::Transport))?
                .context("Monochrome returned an incomplete audio header")?;
            header.extend_from_slice(&chunk[..chunk.len().min(42 - header.len())]);
        }
        let quality =
            flac_quality(&header).context("Monochrome did not return a valid FLAC stream")?;
        Ok(Audio {
            url,
            headers: format!("User-Agent: {AGENT}\r\n"),
            quality,
            obtained: Instant::now(),
            source: "Monochrome",
        })
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 20
        && id.bytes().all(|b| b.is_ascii_digit())
        && id.parse::<u64>().is_ok_and(|id| id > 0)
}

fn parse_track(value: &Value) -> Option<Track> {
    let id = value["trackId"].as_str().or_else(|| value["id"].as_str())?;
    if !valid_id(id) {
        return None;
    }
    let duration = value["duration"].as_u64().unwrap_or(0);
    let artists = value["artistNames"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    Some(Track {
        id: id.into(),
        name: value["title"].as_str()?.into(),
        artists,
        duration_ms: if duration > 1000 {
            duration
        } else {
            duration.saturating_mul(1000)
        }
        .min(u64::from(u32::MAX)) as u32,
        playable: value["playable"].as_bool().unwrap_or(true),
        album: value["albumTitle"]
            .as_str()
            .or_else(|| value["releaseTitle"].as_str())
            .map(str::to_owned),
        ..Track::default()
    })
}

fn flac_quality(header: &[u8]) -> Option<AudioQuality> {
    if header.len() < 42
        || &header[..4] != b"fLaC"
        || header[4] & 0x7f != 0
        || header[5..8] != [0, 0, 34]
    {
        return None;
    }
    let packed = u64::from_be_bytes(header[18..26].try_into().ok()?);
    let rate = (packed >> 44) as u32;
    let bits = ((packed >> 36) & 31) as u8 + 1;
    if !(8000..=768000).contains(&rate) || !matches!(bits, 8 | 16 | 24 | 32) {
        return None;
    }
    Some(AudioQuality {
        codec: Some(Codec::Flac),
        sample_rate: Some(rate),
        bit_depth: Some(bits),
        ..AudioQuality::default()
    })
}

#[derive(Clone)]
pub(crate) struct Resolver {
    client: Client,
    catalog: Catalog,
}

impl Resolver {
    pub(crate) fn new(catalog: Catalog) -> Result<Self> {
        Ok(Self {
            client: Client::new()?,
            catalog,
        })
    }

    pub(crate) async fn audio(&self, id: &str) -> Result<(Audio, Track)> {
        let (track, isrc) = self.catalog.recording(id).await?;
        let recording = self.client.recording_id(&track, isrc.as_deref()).await?;
        Ok((self.client.stream(&recording).await?, track))
    }
}

#[cfg(test)]
mod tests;
