use super::{IO_TIMEOUT, Track};
use crate::catalog::{Browse, Catalog, Rows};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct Metadata {
    spotify: Option<Catalog>,
    deezer: Option<crate::deezer::Client>,
    cache: HashMap<String, Cached>,
}

struct Cached {
    original: Track,
    resolved: Track,
    at: Instant,
}

impl Metadata {
    pub(super) fn new(config: &crate::storage::Config, catalog: &Catalog) -> Self {
        Self {
            spotify: catalog.presence_catalog(config),
            deezer: catalog
                .deezer
                .clone()
                .or_else(|| crate::deezer::Client::new().ok()),
            cache: HashMap::new(),
        }
    }

    pub(super) async fn resolve(&mut self, original: &Track) -> Track {
        if crate::model::valid_id(&original.id)
            || !crate::model::valid_track_id(&original.id)
            || original.name.trim().is_empty()
            || original.artists.trim().is_empty()
        {
            return original.clone();
        }
        if let Some(cached) = self.cache.get(&original.id)
            && cached.original == *original
            && cached.at.elapsed() < Duration::from_secs(300)
        {
            return cached.resolved.clone();
        }
        let mut resolved = self
            .lookup(original)
            .await
            .unwrap_or_else(|| original.clone());
        // The displayed clock always follows the actual playing recording.
        resolved.duration_ms = original.duration_ms;
        if self.cache.len() >= 256 {
            self.cache.clear();
        }
        self.cache.insert(
            original.id.clone(),
            Cached {
                original: original.clone(),
                resolved: resolved.clone(),
                at: Instant::now(),
            },
        );
        resolved
    }

    async fn lookup(&self, original: &Track) -> Option<Track> {
        if let Some(spotify) = &self.spotify
            && let Ok(Ok(track)) =
                tokio::time::timeout(IO_TIMEOUT, spotify.presence_match(original)).await
        {
            return Some(track);
        }
        // Already-known Deezer metadata needs no second network lookup.
        if crate::deezer::key(&original.id).is_some() {
            return Some(original.clone());
        }
        let deezer = self.deezer.as_ref()?;
        let query = format!(
            "{} {}",
            super::display_text(&original.name).trim(),
            super::display_text(&original.artists).trim()
        );
        let page = tokio::time::timeout(IO_TIMEOUT, deezer.page(&Browse::Search(query), 0))
            .await
            .ok()?
            .ok()?;
        let Rows::Tracks(candidates) = page.rows else {
            return None;
        };
        let id = crate::volta::matching::select(original, &candidates).ok()?;
        candidates.into_iter().find(|track| track.id == id)
    }
}

#[cfg(test)]
mod tests;
