use crate::model::Track;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

pub const CAPACITY: usize = 3000;
const TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    track: Track,
    fetched_at: u64,
    sequence: u64,
}

/// Bounded, expiring metadata only. Never contains credentials or listening times.
#[derive(Clone, Serialize, Deserialize)]
pub struct MetadataCache {
    version: u32,
    entries: HashMap<String, Entry>,
    sequence: u64,
    #[serde(skip)]
    pub revision: u64,
}

impl Default for MetadataCache {
    fn default() -> Self {
        Self {
            version: 1,
            entries: HashMap::new(),
            sequence: 0,
            revision: 0,
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl MetadataCache {
    pub fn validate(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(self.version == 1, "Unsupported metadata cache version");
        let before = self.entries.len();
        let now = now();
        self.entries
            .retain(|id, e| id == &e.track.id && now.saturating_sub(e.fetched_at) < TTL_SECS);
        self.trim();
        if self.entries.len() != before {
            self.revision += 1;
        }
        Ok(())
    }
    pub fn prune_expired(&mut self) -> bool {
        let before = self.entries.len();
        let now = now();
        self.entries
            .retain(|_, e| now.saturating_sub(e.fetched_at) < TTL_SECS);
        let changed = self.entries.len() != before;
        if changed {
            self.revision += 1;
        }
        changed
    }
    fn trim(&mut self) {
        if self.entries.len() <= CAPACITY {
            return;
        }
        // Normally only one insertion exceeds the cap: avoid sorting/cloning
        // the entire cache for each newly browsed track.
        if self.entries.len() == CAPACITY + 1 {
            if let Some(id) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.sequence)
                .map(|(id, _)| id.clone())
            {
                self.entries.remove(&id);
            }
        } else {
            let mut oldest: Vec<_> = self
                .entries
                .iter()
                .map(|(id, e)| (e.sequence, id.clone()))
                .collect();
            oldest.sort_unstable();
            for (_, id) in oldest.into_iter().take(self.entries.len() - CAPACITY) {
                self.entries.remove(&id);
            }
        }
    }
    pub fn insert(&mut self, id: String, track: Track) {
        self.sequence = self.sequence.wrapping_add(1);
        self.entries.insert(
            id,
            Entry {
                track,
                fetched_at: now(),
                sequence: self.sequence,
            },
        );
        self.trim();
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn get(&self, id: &str) -> Option<&Track> {
        self.entries
            .get(id)
            .filter(|e| now().saturating_sub(e.fetched_at) < TTL_SECS)
            .map(|e| &e.track)
    }
    pub fn contains_key(&self, id: &str) -> bool {
        self.get(id).is_some()
    }
    pub fn remove(&mut self, id: &str) {
        if self.entries.remove(id).is_some() {
            self.revision = self.revision.wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_insertions_are_bounded_and_expired_entries_miss() {
        let mut cache = MetadataCache::default();
        for i in 0..CAPACITY + 10 {
            let id = format!("{i:022}");
            cache.insert(id.clone(), Track::unknown(&id));
        }
        assert_eq!(cache.entries.len(), CAPACITY);
        let id = format!("{:022}", CAPACITY);
        cache.entries.get_mut(&id).unwrap().fetched_at = 0;
        assert!(cache.get(&id).is_none());
        let bytes = serde_json::to_vec(&cache).unwrap();
        let mut restored: MetadataCache = serde_json::from_slice(&bytes).unwrap();
        restored.validate().unwrap();
        assert!(!restored.entries.contains_key(&id));
        assert_eq!(restored.revision, 1);
        let revision = cache.revision;
        assert!(cache.prune_expired());
        assert!(cache.revision > revision);
        assert!(!cache.prune_expired());
    }
}
