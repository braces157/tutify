//! Preserve Spotify identities and discovery while resolving only audio in Volta.
use super::{Audio, Client};
use crate::{
    catalog::Catalog,
    model::Track,
    service::{FailureKind, ServiceFailure},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

type Matches = HashMap<String, (String, Instant)>;

#[derive(Clone)]
pub(crate) struct Resolver {
    client: Client,
    spotify: Option<Catalog>,
    matches: Arc<Mutex<Matches>>,
}
impl Resolver {
    pub(crate) fn new(client: Client, spotify: Option<Catalog>) -> Self {
        Self {
            client,
            spotify,
            matches: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub(crate) fn accepts(&self, id: &str) -> bool {
        super::key(id).is_some() || (self.spotify.is_some() && crate::model::valid_id(id))
    }
    pub(crate) async fn audio(&self, id: &str) -> Result<Audio> {
        let recording = self.recording(id).await?;
        self.client.stream(&recording).await
    }
    async fn recording(&self, id: &str) -> Result<String> {
        if super::key(id).is_some() {
            return Ok(id.into());
        }
        ensure!(
            crate::model::valid_id(id),
            "Choose a Spotify or Volta track"
        );
        let spotify = self
            .spotify
            .as_ref()
            .context("Choose a track from Volta search")?;
        if let Some((recording, at)) = self.matches.lock().await.get(id)
            && at.elapsed() < Duration::from_secs(6 * 60 * 60)
        {
            return Ok(recording.clone());
        }
        let metadata = spotify.get(&format!("/tracks/{id}"), &[]).await?;
        let track = crate::catalog::parse_track(&metadata)
            .context("Spotify returned incomplete track metadata")?;
        let isrc = metadata["external_ids"]["isrc"].as_str().and_then(isrc);
        if let Some(isrc) = &isrc {
            match self.client.get(&format!("/track/{isrc}"), &[]).await {
                Ok(value) => {
                    if let Some(candidate) = super::catalog::parse_track(&value)
                        && candidate.playable
                        && (super::key(&candidate.id) == Some(isrc.as_str())
                            || value["isrc"].as_str().and_then(self::isrc).as_ref() == Some(isrc))
                        && compatible_duration(&track, &candidate, true)
                    {
                        return Ok(self.remember(id, candidate.id).await);
                    }
                }
                Err(error) if ServiceFailure::is(&error, FailureKind::MissingItem) => (),
                Err(error) => return Err(error),
            }
        }
        let page = self
            .client
            .page(
                &crate::catalog::Browse::Search(format!("{} {}", track.name, track.artists)),
                0,
            )
            .await?;
        let crate::catalog::Rows::Tracks(mut candidates) = page.rows else {
            bail!("Volta returned no tracks");
        };
        if let Some(isrc) = &isrc {
            candidates.retain(|candidate| super::key(&candidate.id) == Some(isrc.as_str()));
        }
        let recording = select(&track, &candidates)?;
        Ok(self.remember(id, recording).await)
    }
    async fn remember(&self, id: &str, recording: String) -> String {
        let mut matches = self.matches.lock().await;
        if matches.len() >= 512 {
            matches.clear();
        }
        matches.insert(id.into(), (recording.clone(), Instant::now()));
        recording
    }
}

fn isrc(raw: &str) -> Option<String> {
    let value = raw.trim().to_ascii_uppercase();
    let bytes = value.as_bytes();
    (bytes.len() == 12
        && bytes[..2].iter().all(u8::is_ascii_uppercase)
        && bytes[2..5].iter().all(u8::is_ascii_alphanumeric)
        && bytes[5..].iter().all(u8::is_ascii_digit))
    .then_some(value)
}
fn folded(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}
fn compatible_duration(a: &Track, b: &Track, exact_isrc: bool) -> bool {
    if a.duration_ms == 0 || b.duration_ms == 0 {
        return exact_isrc;
    }
    a.duration_ms.abs_diff(b.duration_ms) <= (a.duration_ms / 50).clamp(3000, 6000)
}
fn same_recording(a: &Track, b: &Track) -> bool {
    let title = folded(&a.name);
    !title.is_empty()
        && title == folded(&b.name)
        && b.playable
        && compatible_duration(a, b, false)
        && a.artists
            .split(',')
            .map(folded)
            .filter(|a| !a.is_empty())
            .any(|artist| {
                b.artists
                    .split(',')
                    .map(folded)
                    .any(|other| artist == other)
            })
}
fn select(track: &Track, candidates: &[Track]) -> Result<String> {
    let mut ids = HashSet::new();
    let matched: Vec<_> = candidates
        .iter()
        .filter(|candidate| same_recording(track, candidate) && ids.insert(candidate.id.clone()))
        .collect();
    if matched.len() == 1 {
        return Ok(matched[0].id.clone());
    }
    if let Some(album) = track
        .album
        .as_ref()
        .map(|album| folded(album))
        .filter(|album| !album.is_empty())
    {
        let albums: Vec<_> = matched
            .iter()
            .filter(|candidate| {
                candidate
                    .album
                    .as_ref()
                    .is_some_and(|other| folded(other) == album)
            })
            .collect();
        if albums.len() == 1 {
            return Ok(albums[0].id.clone());
        }
    }
    bail!(
        "No unambiguous Volta recording matches this Spotify track. Your Spotify queue is preserved; use --source spotify for Spotify audio."
    )
}

#[cfg(test)]
mod tests;
