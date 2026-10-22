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
    youtube: Option<crate::youtube::Tools>,
    metadata: Arc<Mutex<HashMap<String, Track>>>,
}
impl Resolver {
    pub(crate) fn new(client: Client, spotify: Option<Catalog>) -> Self {
        Self {
            client,
            spotify,
            matches: Arc::new(Mutex::new(HashMap::new())),
            youtube: None,
            metadata: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub(crate) fn with_youtube(mut self, tools: Option<crate::youtube::Tools>) -> Self {
        self.youtube = tools;
        self
    }
    pub(crate) fn accepts(&self, id: &str) -> bool {
        super::key(id).is_some()
            || (self.youtube.is_some() && crate::youtube::video_key(id).is_some())
            || (self.spotify.is_some() && crate::model::valid_id(id))
    }
    pub(crate) async fn audio(&self, id: &str) -> Result<Audio> {
        if crate::youtube::video_key(id).is_some() {
            return Ok(self
                .youtube
                .as_ref()
                .context("YouTube audio fallback is unavailable")?
                .resolve(id)
                .await?
                .into_volta_audio());
        }
        let result = async {
            let recording = self.recording(id).await?;
            self.client.stream(&recording).await
        }
        .await;
        match result {
            Ok(audio) => Ok(audio),
            Err(error) if self.youtube.is_some() => self
                .youtube_audio(id)
                .await
                .with_context(|| format!("Volta unavailable ({error}); YouTube fallback failed")),
            Err(error) => Err(error),
        }
    }
    pub(crate) async fn youtube_audio(&self, id: &str) -> Result<Audio> {
        let tools = self
            .youtube
            .as_ref()
            .context("YouTube fallback needs 'tuitify youtube setup'")?;
        let known = self.metadata.lock().await.get(id).cloned();
        let track = match known {
            Some(track) => track,
            None if super::key(id).is_some() => self.client.track(id).await?,
            None => {
                self.spotify
                    .as_ref()
                    .context("Spotify metadata is unavailable")?
                    .track(id)
                    .await?
            }
        };
        let page = tools
            .page(
                &crate::catalog::Browse::Search(format!("{} {}", track.name, track.artists)),
                0,
            )
            .await?;
        let crate::catalog::Rows::Tracks(candidates) = page.rows else {
            bail!("YouTube returned no songs");
        };
        for candidate in candidates
            .into_iter()
            .filter(|candidate| youtube_recording(&track, candidate))
            .take(3)
        {
            if let Ok(stream) = tools.resolve(&candidate.id).await
                && youtube_recording(&track, &stream.track)
            {
                return Ok(stream.into_volta_audio());
            }
        }
        bail!("No matching YouTube recording is available")
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
        let mut tracks = self.metadata.lock().await;
        if tracks.len() >= 512 {
            tracks.clear();
        }
        tracks.insert(id.into(), track.clone());
        drop(tracks);
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
pub(crate) fn compatible_duration(a: &Track, b: &Track, exact_isrc: bool) -> bool {
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

// Accept common official-video decorations, never live/remix/cover versions.
pub(crate) fn youtube_recording(original: &Track, video: &Track) -> bool {
    let mut candidate = video.clone();
    for decoration in [
        " (Official Audio)",
        " (Official Video)",
        " (Official Music Video)",
        " (Lyrics)",
        " [Official Audio]",
        " [Official Video]",
    ] {
        if candidate
            .name
            .to_lowercase()
            .ends_with(&decoration.to_lowercase())
        {
            candidate
                .name
                .truncate(candidate.name.len() - decoration.len());
            break;
        }
    }
    if let Some((artist, title)) = candidate.name.split_once(" - ")
        && original
            .artists
            .split(',')
            .any(|known| folded(known) == folded(artist))
    {
        candidate.artists = artist.into();
        candidate.name = title.into();
    } else {
        candidate.artists = candidate
            .artists
            .trim_end_matches(" - Topic")
            .trim_end_matches("VEVO")
            .into();
    }
    // Music catalogs sometimes publish both the original-script and translated
    // title, e.g. "花になって - Be a flower". Match complete aliases while
    // retaining the same artist, duration and recording-version checks.
    let original_titles = youtube_title_aliases(&original.name);
    let candidate_titles = youtube_title_aliases(&candidate.name);
    if original_titles
        .iter()
        .filter(|title| !title.is_empty())
        .any(|title| {
            candidate_titles
                .iter()
                .filter(|other| !other.is_empty())
                .any(|other| folded(title) == folded(other))
        })
    {
        candidate.name = original.name.clone();
    }
    same_recording(original, &candidate)
}

fn youtube_title_aliases(title: &str) -> [&str; 3] {
    let Some((left, right)) = title.split_once(" - ") else {
        return [title, "", ""];
    };
    let cjk = |text: &str| {
        text.chars().any(|c| matches!(c,
        '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}'))
    };
    let lower = title.to_lowercase();
    let version = lower.split(|c: char| !c.is_alphanumeric()).any(|word| {
        matches!(
            word,
            "live"
                | "remix"
                | "cover"
                | "instrumental"
                | "karaoke"
                | "acoustic"
                | "nightcore"
                | "sped"
                | "slowed"
                | "reverb"
                | "edit"
                | "version"
                | "tv"
                | "short"
                | "extended"
        )
    }) || [
        "ライブ",
        "ライヴ",
        "カバー",
        "インスト",
        "カラオケ",
        "リミックス",
        "アコースティック",
    ]
    .iter()
    .any(|tag| lower.contains(tag));
    if left.trim().is_empty()
        || right.trim().is_empty()
        || right.contains(" - ")
        || cjk(left) == cjk(right)
        || version
        || title.contains(['(', ')', '[', ']', '（', '）'])
    {
        return [title, "", ""];
    }
    [title, left.trim(), right.trim()]
}
pub(crate) fn select(track: &Track, candidates: &[Track]) -> Result<String> {
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
    bail!("No unambiguous recording matches this track")
}

#[cfg(test)]
mod tests;
