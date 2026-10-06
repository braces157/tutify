//! Parsed collection pages with a byte budget and least-recently-used eviction.
use super::*;
use std::collections::HashMap;

const BYTE_BUDGET: usize = 32 * 1024 * 1024;
const MAX_COLLECTIONS: usize = 16;

pub(super) enum ParsedRows {
    Tracks(Vec<Option<Track>>),
    Playlists(Vec<Option<Playlist>>),
}
impl ParsedRows {
    pub(super) fn parse(items: &[Value], playlists: bool) -> Self {
        // Keep missing rows in place: remote offsets must not shift after filtering.
        if playlists {
            Self::Playlists(items.iter().map(parse_playlist).collect())
        } else {
            Self::Tracks(items.iter().map(parse_track).collect())
        }
    }
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Tracks(rows) => rows.len(),
            Self::Playlists(rows) => rows.len(),
        }
    }
    pub(super) fn page(&self, offset: usize, size: usize) -> Rows {
        match self {
            Self::Tracks(rows) => Rows::Tracks(
                rows.iter()
                    .skip(offset)
                    .take(size)
                    .flatten()
                    .cloned()
                    .collect(),
            ),
            Self::Playlists(rows) => Rows::Playlists(
                rows.iter()
                    .skip(offset)
                    .take(size)
                    .flatten()
                    .cloned()
                    .collect(),
            ),
        }
    }
    fn bytes(&self) -> usize {
        match self {
            Self::Tracks(rows) => {
                rows.capacity() * std::mem::size_of::<Option<Track>>()
                    + rows
                        .iter()
                        .flatten()
                        .map(|t| {
                            t.id.capacity()
                                + t.name.capacity()
                                + t.artists.capacity()
                                + t.artist_ids.capacity() * std::mem::size_of::<String>()
                                + t.artist_ids.iter().map(String::capacity).sum::<usize>()
                                + t.album.as_ref().map_or(0, String::capacity)
                                + t.album_id.as_ref().map_or(0, String::capacity)
                                + t.album_art_url.as_ref().map_or(0, String::capacity)
                        })
                        .sum::<usize>()
            }
            Self::Playlists(rows) => {
                rows.capacity() * std::mem::size_of::<Option<Playlist>>()
                    + rows
                        .iter()
                        .flatten()
                        .map(|p| p.id.capacity() + p.name.capacity() + p.owner.capacity())
                        .sum::<usize>()
            }
        }
    }
}

pub(super) struct Snapshot {
    pub(super) rows: ParsedRows,
    pub(super) complete: bool,
    pub(super) limit: usize,
    pub(super) at: Instant,
    pub(super) version: u64,
}
struct Entry {
    snapshot: Arc<Snapshot>,
    used: u64,
    bytes: usize,
}
#[derive(Default)]
pub(super) struct Pages {
    entries: HashMap<String, Entry>,
    clock: u64,
}
impl Pages {
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }
    pub(super) fn get(&mut self, key: &str, version: u64) -> Option<Arc<Snapshot>> {
        self.entries.retain(|_, entry| {
            entry.snapshot.version == version && entry.snapshot.at.elapsed() < CACHE_TTL
        });
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(key)?;
        entry.used = self.clock;
        Some(entry.snapshot.clone())
    }
    pub(super) fn insert(&mut self, key: String, snapshot: Arc<Snapshot>) {
        let bytes = snapshot.rows.bytes() + key.capacity() + std::mem::size_of::<Entry>();
        self.entries.remove(&key);
        // An unusually large collection still serves its requested page, but is
        // not retained indefinitely in addition to the user's queue/metadata.
        if bytes > BYTE_BUDGET {
            return;
        }
        while !self.entries.is_empty()
            && (self.entries.len() >= MAX_COLLECTIONS
                || self
                    .entries
                    .values()
                    .map(|entry| entry.bytes)
                    .sum::<usize>()
                    + bytes
                    > BYTE_BUDGET)
        {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .unwrap()
                .0
                .clone();
            self.entries.remove(&oldest);
        }
        self.clock = self.clock.wrapping_add(1);
        self.entries.insert(
            key,
            Entry {
                snapshot,
                used: self.clock,
                bytes,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(index: usize) -> Arc<Snapshot> {
        Arc::new(Snapshot {
            rows: ParsedRows::parse(
                &[json!({"videoId":"dQw4w9WgXcQ", "title":format!("Song {index}")})],
                false,
            ),
            complete: true,
            limit: 1,
            at: Instant::now(),
            version: 0,
        })
    }
    #[test]
    fn hot_collections_survive_eviction_and_invalid_remote_rows_keep_offsets() {
        let mut pages = Pages::default();
        for i in 0..16 {
            pages.insert(i.to_string(), snapshot(i));
        }
        assert!(pages.get("0", 0).is_some());
        pages.insert("16".into(), snapshot(16));
        assert!(pages.get("0", 0).is_some());
        assert!(pages.get("1", 0).is_none());
        let rows = ParsedRows::parse(
            &[json!({}), json!({"videoId":"dQw4w9WgXcQ","title":"Song"})],
            false,
        );
        assert_eq!(rows.len(), 2);
        let Rows::Tracks(first) = rows.page(0, 1) else {
            panic!()
        };
        assert!(first.is_empty());
        let Rows::Tracks(second) = rows.page(1, 1) else {
            panic!()
        };
        assert_eq!(second.len(), 1);
        assert!(pages.get("0", 1).is_none());
    }
    #[test]
    fn byte_budget_applies_to_variable_metadata_and_oversized_collections() {
        let mut pages = Pages::default();
        let snap = Arc::new(Snapshot {
            rows: ParsedRows::Tracks(vec![Some(Track {
                id: "youtube:dQw4w9WgXcQ".into(),
                name: "x".repeat(BYTE_BUDGET + 1),
                ..Track::default()
            })]),
            complete: true,
            limit: 1,
            at: Instant::now(),
            version: 0,
        });
        pages.insert("huge".into(), snap);
        assert!(pages.entries.is_empty());
    }
}
