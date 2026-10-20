//! Public music metadata only. Preview URLs are never used as playback audio.
use crate::{
    catalog::{ArtistResultSource, Browse, Page, RecommendationSource, Recommendations, Rows},
    model::Track,
    service::{FailureKind, Provider, ServiceFailure},
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[cfg(test)]
pub(crate) mod tests;

const PAGE_SIZE: usize = 20;
const MAX_RESPONSE: usize = 2 * 1024 * 1024;

pub(crate) fn key(id: &str) -> Option<&str> {
    id.strip_prefix("deezer:").filter(|key| {
        !key.is_empty()
            && key.len() <= 20
            && key.bytes().all(|b| b.is_ascii_digit())
            && key.parse::<u64>().is_ok_and(|id| id > 0)
    })
}

pub(crate) fn track_id(input: &str) -> Option<String> {
    if key(input).is_some() {
        return Some(input.into());
    }
    let url = url::Url::parse(input).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !matches!(url.host_str(), Some("deezer.com" | "www.deezer.com"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let parts: Vec<_> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    let at = usize::from(parts.first().is_some_and(|part| part.len() == 2));
    if parts.get(at) != Some(&"track") || parts.len() != at + 2 {
        return None;
    }
    let id = format!("deezer:{}", parts[at + 1]);
    key(&id).map(|_| id.clone())
}

pub(crate) fn isrc(raw: &str) -> Option<String> {
    let code = raw.trim().to_ascii_uppercase();
    let bytes = code.as_bytes();
    (bytes.len() == 12
        && bytes[..2].iter().all(u8::is_ascii_uppercase)
        && bytes[2..5].iter().all(u8::is_ascii_alphanumeric)
        && bytes[5..].iter().all(u8::is_ascii_digit))
    .then_some(code)
}

#[derive(Default)]
struct State {
    cache: HashMap<String, (Instant, Value)>,
    cooldown: Option<ServiceFailure>,
}

#[derive(Clone)]
pub(crate) struct Client {
    http: reqwest::Client,
    base: String,
    state: Arc<Mutex<State>>,
}

impl Client {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(8))
                .user_agent(concat!("Tuitify/", env!("CARGO_PKG_VERSION")))
                .build()?,
            base: "https://api.deezer.com".into(),
            state: Arc::new(Mutex::new(State::default())),
        })
    }

    #[cfg(test)]
    pub(crate) fn mock(base: &str) -> Self {
        Self {
            base: base.into(),
            ..Self::new().unwrap()
        }
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let cache_key = format!("{path}:{query:?}");
        let mut state = self.state.lock().await;
        if let Some((at, value)) = state.cache.get(&cache_key)
            && at.elapsed() < Duration::from_secs(300)
        {
            return Ok(value.clone());
        }
        if let Some(failure) = state.cooldown.filter(|failure| failure.active()) {
            return Err(failure.into());
        }
        let mut response = self
            .http
            .get(format!("{}{path}", self.base))
            .query(query)
            .send()
            .await
            .map_err(|_| ServiceFailure::new(Provider::Deezer, FailureKind::Transport))?;
        if !response.status().is_success() {
            let failure = ServiceFailure::from_response(response, Provider::Deezer).await;
            if failure.throttled() {
                state.cooldown = Some(failure);
            }
            return Err(failure.into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ServiceFailure::new(Provider::Deezer, FailureKind::Transport))?
        {
            ensure!(
                bytes.len() + chunk.len() <= MAX_RESPONSE,
                ServiceFailure::new(Provider::Deezer, FailureKind::InvalidResponse)
            );
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| ServiceFailure::new(Provider::Deezer, FailureKind::InvalidResponse))?;
        if let Some(error) = value.get("error") {
            let kind = match error["code"].as_u64() {
                Some(4) => FailureKind::RateLimited,
                Some(100 | 800) => FailureKind::MissingItem,
                _ => FailureKind::Server,
            };
            let mut failure = ServiceFailure::new(Provider::Deezer, kind);
            if kind != FailureKind::MissingItem {
                failure.retry_at = Some(Instant::now() + Duration::from_secs(60));
                state.cooldown = Some(failure);
            }
            return Err(failure.into());
        }
        if bytes.len() <= 256 * 1024 {
            if state.cache.len() >= 64 {
                state.cache.clear();
            }
            state
                .cache
                .insert(cache_key, (Instant::now(), value.clone()));
        }
        Ok(value)
    }

    pub async fn recording(&self, id: &str) -> Result<(Track, Option<String>)> {
        let id_key = key(id).context("Invalid Deezer track ID")?;
        let value = self.get(&format!("/track/{id_key}"), &[]).await?;
        let track = parse_track(&value).context("Deezer returned incomplete track metadata")?;
        ensure!(track.id == id, "Deezer returned a different recording");
        Ok((track, value["isrc"].as_str().and_then(isrc)))
    }

    pub async fn page(&self, browse: &Browse, offset: usize) -> Result<Page> {
        if let Browse::Search(query) = browse {
            if let Some(id) = track_id(query) {
                return Ok(Page {
                    rows: Rows::Tracks(if offset == 0 {
                        vec![self.recording(&id).await?.0]
                    } else {
                        vec![]
                    }),
                    offset,
                    next: None,
                    artist_source: None,
                });
            }
            if query.trim().is_empty() {
                return Ok(Page {
                    rows: Rows::Tracks(vec![]),
                    offset,
                    next: None,
                    artist_source: None,
                });
            }
        }
        let mut query = vec![
            ("index", offset.to_string()),
            ("limit", PAGE_SIZE.to_string()),
        ];
        let path = match browse {
            Browse::Search(text) => {
                query.push(("q", text.clone()));
                "/search".into()
            }
            Browse::Artist(id) => format!(
                "/artist/{}/top",
                key(id).context("Invalid Deezer artist ID")?
            ),
            Browse::Album(id) => format!("/album/{}", key(id).context("Invalid Deezer album ID")?),
            _ => anyhow::bail!("Deezer search does not change your connected library"),
        };
        let value = self.get(&path, &query).await?;
        let album = matches!(browse, Browse::Album(_));
        let list = if album { &value["tracks"] } else { &value };
        let items = list["data"]
            .as_array()
            .ok_or_else(|| ServiceFailure::new(Provider::Deezer, FailureKind::InvalidResponse))?;
        let mut tracks = Vec::new();
        for item in items
            .iter()
            .skip(if album { offset } else { 0 })
            .take(PAGE_SIZE)
        {
            let mut item = item.clone();
            if album {
                item["album"] = serde_json::json!({"id":value["id"],"title":value["title"],"cover_medium":value["cover_medium"]});
            }
            if let Some(track) = parse_track(&item) {
                tracks.push(track);
            }
        }
        let next = if album {
            (offset + PAGE_SIZE < items.len()).then_some(offset + PAGE_SIZE)
        } else {
            // Advance by the provider's raw rows, including unavailable entries.
            (list["next"].as_str().is_some() && !items.is_empty()).then_some(offset + items.len())
        };
        Ok(Page {
            rows: Rows::Tracks(tracks),
            offset,
            next,
            artist_source: matches!(browse, Browse::Artist(_))
                .then_some(ArtistResultSource::Deezer),
        })
    }

    pub async fn recommendations(
        &self,
        seed: &Track,
        excluded: &[Track],
        round: usize,
    ) -> Result<Recommendations> {
        let artist = seed
            .artist_ids
            .iter()
            .find_map(|id| key(id))
            .context("Deezer artist metadata is unavailable; refresh the track")?;
        let related = self
            .get(
                &format!("/artist/{artist}/related"),
                &[("limit", "30".into())],
            )
            .await?;
        let artists = related["data"]
            .as_array()
            .ok_or_else(|| ServiceFailure::new(Provider::Deezer, FailureKind::InvalidResponse))?;
        let mut groups = Vec::new();
        let ids = artists
            .iter()
            .filter_map(|artist| identity(&artist["id"]))
            .collect::<Vec<_>>();
        let ids = if ids.is_empty() {
            vec![format!("deezer:{artist}")]
        } else {
            ids
        };
        for step in 0..ids.len().min(5) {
            let id = &ids[(round * 5 + step) % ids.len()];
            let page = self
                .page(
                    &Browse::Artist(id.clone()),
                    (round / ids.len().div_ceil(5)) * 3,
                )
                .await?;
            let Rows::Tracks(tracks) = page.rows else {
                unreachable!()
            };
            groups.push(tracks);
        }
        let mut seen: HashSet<_> = excluded
            .iter()
            .chain(std::iter::once(seed))
            .map(|track| track.id.clone())
            .collect();
        let mut recordings: HashSet<_> = excluded
            .iter()
            .chain(std::iter::once(seed))
            .map(crate::catalog::recording_key)
            .collect();
        let mut tracks = Vec::new();
        for at in 0..PAGE_SIZE {
            for group in &groups {
                if let Some(track) = group.get(at)
                    && track.playable
                    && seen.insert(track.id.clone())
                    && recordings.insert(crate::catalog::recording_key(track))
                {
                    tracks.push(track.clone());
                    if tracks.len() == 15 {
                        break;
                    }
                }
            }
            if tracks.len() == 15 {
                break;
            }
        }
        Ok(Recommendations {
            tracks,
            source: RecommendationSource::Deezer,
        })
    }
}

fn identity(value: &Value) -> Option<String> {
    let id = value.as_u64()?.to_string();
    let id = format!("deezer:{id}");
    key(&id).map(|_| id.clone())
}
fn clean(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).collect()
}

pub(crate) fn parse_track(value: &Value) -> Option<Track> {
    if value["type"].as_str().is_some_and(|kind| kind != "track") {
        return None;
    }
    let contributors = value["contributors"]
        .as_array()
        .filter(|rows| !rows.is_empty());
    let artists = contributors
        .cloned()
        .unwrap_or_else(|| vec![value["artist"].clone()]);
    Some(Track {
        music_metadata: true,
        id: identity(&value["id"])?,
        name: clean(value["title"].as_str()?),
        artists: artists
            .iter()
            .filter_map(|artist| artist["name"].as_str())
            .map(clean)
            .collect::<Vec<_>>()
            .join(", "),
        artist_ids: artists
            .iter()
            .filter_map(|artist| identity(&artist["id"]))
            .collect(),
        duration_ms: value["duration"]
            .as_u64()?
            .saturating_mul(1000)
            .min(u32::MAX as u64) as u32,
        playable: value["readable"].as_bool().unwrap_or(true),
        album: value["album"]["title"].as_str().map(clean),
        album_id: identity(&value["album"]["id"]),
        album_art_url: value["album"]["cover_medium"].as_str().map(clean),
        track_number: value["track_position"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok()),
    })
}
