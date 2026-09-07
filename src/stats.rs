use crate::{cache::MetadataCache, model::valid_id};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub const MAX_STATS_ENTRIES: usize = 50_000;
pub const MAX_ACCOUNTING_INTERVAL: Duration = Duration::from_secs(5);
pub const PLAY_TIME_THRESHOLD_MS: u64 = 30_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackStat {
    pub id: String,
    pub name: String,
    pub artists: String,
    #[serde(alias = "plays")]
    pub play_count: u64,
    pub listened_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SongStats {
    pub version: u32,
    #[serde(default, alias = "entries")]
    pub tracks: HashMap<String, TrackStat>,
    #[serde(skip)]
    pub revision: u64,
}

impl Default for SongStats {
    fn default() -> Self {
        Self {
            version: 1,
            tracks: HashMap::new(),
            revision: 0,
        }
    }
}

pub fn is_placeholder(name: &str, id: &str) -> bool {
    name.is_empty()
        || name == format!("Track {id}")
        || name == "Track unavailable (F5 rechecks)"
        || (name.starts_with("Track ") && name.len() == 6 + 22 && valid_id(&name[6..]))
}

pub fn format_duration(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}h {mins:02}m")
    } else {
        format!("{mins}:{secs:02}")
    }
}

impl SongStats {
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn validate(&mut self) -> Result<()> {
        if self.version != 1 {
            bail!(
                "Unsupported song statistics version {}; preserve stats.json and update Tuitify",
                self.version
            );
        }
        if self.tracks.len() > MAX_STATS_ENTRIES {
            bail!(
                "Song statistics exceed {MAX_STATS_ENTRIES} entries; preserve stats.json and move it aside to reset"
            );
        }
        for (key, stat) in &self.tracks {
            if !valid_id(key) || !valid_id(&stat.id) || key != &stat.id {
                bail!(
                    "Invalid track ID in stats: {key}; preserve stats.json and move it aside to reset"
                );
            }
        }
        Ok(())
    }

    pub fn trim(&mut self) {
        if self.tracks.len() <= MAX_STATS_ENTRIES {
            return;
        }
        let excess = self.tracks.len() - MAX_STATS_ENTRIES;
        let mut candidates: Vec<(u64, u64, String)> = self
            .tracks
            .iter()
            .map(|(id, stat)| (stat.play_count, stat.listened_ms, id.clone()))
            .collect();
        candidates.sort_unstable();
        for (_, _, id) in candidates.into_iter().take(excess) {
            self.tracks.remove(&id);
        }
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn add_listened_ms(
        &mut self,
        id: &str,
        delta_ms: u64,
        fallback_name: &str,
        fallback_artists: &str,
    ) {
        if !valid_id(id) || delta_ms == 0 {
            return;
        }
        let changed = if let Some(entry) = self.tracks.get_mut(id) {
            entry.listened_ms = entry.listened_ms.saturating_add(delta_ms);
            if is_placeholder(&entry.name, id) && !is_placeholder(fallback_name, id) {
                entry.name = fallback_name.to_string();
                if !fallback_artists.is_empty() {
                    entry.artists = fallback_artists.to_string();
                }
            } else if !fallback_artists.is_empty() && entry.artists.is_empty() {
                entry.artists = fallback_artists.to_string();
            }
            true
        } else {
            if self.tracks.len() >= MAX_STATS_ENTRIES {
                self.trim();
                if self.tracks.len() >= MAX_STATS_ENTRIES {
                    if let Some(id_to_remove) = self
                        .tracks
                        .iter()
                        .min_by_key(|(id, s)| (s.play_count, s.listened_ms, (*id).as_str()))
                        .map(|(k, _)| k.clone())
                    {
                        self.tracks.remove(&id_to_remove);
                    }
                }
            }
            let name = if fallback_name.is_empty() {
                format!("Track {id}")
            } else {
                fallback_name.to_string()
            };
            self.tracks.insert(
                id.to_string(),
                TrackStat {
                    id: id.to_string(),
                    name,
                    artists: fallback_artists.to_string(),
                    play_count: 0,
                    listened_ms: delta_ms,
                },
            );
            true
        };
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub fn add_play(&mut self, id: &str, fallback_name: &str, fallback_artists: &str) {
        if !valid_id(id) {
            return;
        }
        let changed = if let Some(entry) = self.tracks.get_mut(id) {
            entry.play_count = entry.play_count.saturating_add(1);
            if is_placeholder(&entry.name, id) && !is_placeholder(fallback_name, id) {
                entry.name = fallback_name.to_string();
                if !fallback_artists.is_empty() {
                    entry.artists = fallback_artists.to_string();
                }
            } else if !fallback_artists.is_empty() && entry.artists.is_empty() {
                entry.artists = fallback_artists.to_string();
            }
            true
        } else {
            if self.tracks.len() >= MAX_STATS_ENTRIES {
                self.trim();
                if self.tracks.len() >= MAX_STATS_ENTRIES {
                    if let Some(id_to_remove) = self
                        .tracks
                        .iter()
                        .min_by_key(|(id, s)| (s.play_count, s.listened_ms, (*id).as_str()))
                        .map(|(k, _)| k.clone())
                    {
                        self.tracks.remove(&id_to_remove);
                    }
                }
            }
            let name = if fallback_name.is_empty() {
                format!("Track {id}")
            } else {
                fallback_name.to_string()
            };
            self.tracks.insert(
                id.to_string(),
                TrackStat {
                    id: id.to_string(),
                    name,
                    artists: fallback_artists.to_string(),
                    play_count: 1,
                    listened_ms: 0,
                },
            );
            true
        };
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub fn refresh_metadata(&mut self, cache: &MetadataCache) {
        let mut changed = false;
        for (id, stat) in &mut self.tracks {
            if let Some(track) = cache.get(id) {
                if !is_placeholder(&track.name, id) {
                    if stat.name != track.name {
                        stat.name = track.name.clone();
                        changed = true;
                    }
                    if !track.artists.is_empty() && stat.artists != track.artists {
                        stat.artists = track.artists.clone();
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub fn sorted_tracks(&self) -> Vec<TrackStat> {
        let mut list: Vec<TrackStat> = self.tracks.values().cloned().collect();
        list.sort_by(|a, b| {
            b.play_count
                .cmp(&a.play_count)
                .then_with(|| b.listened_ms.cmp(&a.listened_ms))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.id.cmp(&b.id))
        });
        list
    }
}

#[derive(Clone, Debug, Default)]
pub struct PlaybackAccounting {
    pub generation: u64,
    pub track_id: Option<String>,
    pub duration_ms: u32,
    pub generation_listened_ms: u64,
    pub play_credited: bool,
    pub last_accounted_at: Option<Instant>,
}

impl PlaybackAccounting {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start_generation(
        &mut self,
        generation: u64,
        track_id: Option<String>,
        duration_ms: u32,
    ) {
        self.generation = generation;
        self.track_id = track_id;
        self.duration_ms = duration_ms;
        self.generation_listened_ms = 0;
        self.play_credited = false;
        self.last_accounted_at = None;
    }

    pub fn on_playing(
        &mut self,
        generation: u64,
        track_id: Option<String>,
        duration_ms: u32,
        now: Instant,
    ) {
        if generation != self.generation {
            return;
        }
        if self.track_id != track_id && track_id.is_some() {
            self.track_id = track_id;
        }
        if duration_ms > 0 {
            self.duration_ms = duration_ms;
        }
        self.last_accounted_at = Some(now);
    }

    pub fn account_time(
        &mut self,
        now: Instant,
        track_id: &str,
        fallback_name: &str,
        fallback_artists: &str,
        stats: &mut SongStats,
    ) {
        let Some(last) = self.last_accounted_at else {
            return;
        };
        let elapsed = now.saturating_duration_since(last);
        let clamped = elapsed.min(MAX_ACCOUNTING_INTERVAL);
        self.last_accounted_at = Some(now);
        let delta_ms = clamped.as_millis() as u64;
        if delta_ms > 0 {
            self.generation_listened_ms = self.generation_listened_ms.saturating_add(delta_ms);
            stats.add_listened_ms(track_id, delta_ms, fallback_name, fallback_artists);

            let threshold_ms = if self.duration_ms > 0 {
                (self.duration_ms as u64 / 2).min(PLAY_TIME_THRESHOLD_MS)
            } else {
                PLAY_TIME_THRESHOLD_MS
            };
            if !self.play_credited && self.generation_listened_ms >= threshold_ms {
                self.play_credited = true;
                stats.add_play(track_id, fallback_name, fallback_artists);
            }
        }
    }

    pub fn on_completed(
        &mut self,
        generation: u64,
        track_id: &str,
        fallback_name: &str,
        fallback_artists: &str,
        stats: &mut SongStats,
    ) {
        if generation != self.generation {
            return;
        }
        if !self.play_credited {
            self.play_credited = true;
            stats.add_play(track_id, fallback_name, fallback_artists);
        }
        self.last_accounted_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Track;

    #[test]
    fn storage_roundtrip_and_defaults() {
        let mut stats = SongStats::default();
        let id1 = "1".repeat(22);
        let id2 = "2".repeat(22);
        stats.add_listened_ms(&id1, 15000, "Song One", "Artist A");
        stats.add_play(&id1, "Song One", "Artist A");
        stats.add_play(&id2, "Song Two", "Artist B");

        assert_eq!(stats.len(), 2);
        let s1 = stats.tracks.get(&id1).unwrap();
        assert_eq!(s1.play_count, 1);
        assert_eq!(s1.listened_ms, 15000);
        assert_eq!(s1.name, "Song One");
        assert_eq!(s1.artists, "Artist A");

        let json = serde_json::to_string(&stats).unwrap();
        let mut loaded: SongStats = serde_json::from_str(&json).unwrap();
        loaded.validate().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.tracks.get(&id1), stats.tracks.get(&id1));
    }

    #[test]
    fn validation_rejects_invalid_id_version_or_bounds() {
        let mut invalid_version = SongStats {
            version: 2,
            ..Default::default()
        };
        assert!(invalid_version.validate().is_err());

        let mut stats = SongStats::default();
        stats.tracks.insert(
            "short".into(),
            TrackStat {
                id: "short".into(),
                name: "Short".into(),
                artists: "Artist".into(),
                play_count: 1,
                listened_ms: 1000,
            },
        );
        assert!(stats.validate().is_err());

        stats.tracks.clear();
        for i in 0..=MAX_STATS_ENTRIES {
            let id = format!("{:022}", i);
            stats.tracks.insert(
                id.clone(),
                TrackStat {
                    id,
                    name: "N".into(),
                    artists: "A".into(),
                    play_count: 1,
                    listened_ms: 100,
                },
            );
        }
        assert!(stats.validate().is_err());
        stats.trim();
        assert_eq!(stats.len(), MAX_STATS_ENTRIES);
        assert!(stats.validate().is_ok());
    }

    #[test]
    fn metadata_retention_and_refresh() {
        let mut stats = SongStats::default();
        let id = "0".repeat(22);
        stats.add_listened_ms(&id, 5000, "Initial Good Name", "Initial Artist");

        // Refresh with cache containing unknown placeholder should not overwrite
        let mut cache = MetadataCache::default();
        cache.insert(id.clone(), Track::unknown(&id));
        stats.refresh_metadata(&cache);
        assert_eq!(stats.tracks.get(&id).unwrap().name, "Initial Good Name");
        assert_eq!(stats.tracks.get(&id).unwrap().artists, "Initial Artist");

        // Refresh with real metadata in cache should update
        let real_track = Track {
            id: id.clone(),
            name: "Updated Real Title".into(),
            artists: "Updated Real Artist".into(),
            duration_ms: 210000,
            playable: true,
            album: None,
            album_art_url: None,
        };
        cache.insert(id.clone(), real_track);
        stats.refresh_metadata(&cache);
        assert_eq!(stats.tracks.get(&id).unwrap().name, "Updated Real Title");
        assert_eq!(
            stats.tracks.get(&id).unwrap().artists,
            "Updated Real Artist"
        );
    }

    #[test]
    fn sorting_order() {
        let mut stats = SongStats::default();
        let id1 = "1".repeat(22);
        let id2 = "2".repeat(22);
        let id3 = "3".repeat(22);
        let id4 = "4".repeat(22);

        // id1: 5 plays, 10000 ms, "Alpha"
        stats.add_play(&id1, "Alpha", "Art");
        stats.tracks.get_mut(&id1).unwrap().play_count = 5;
        stats.tracks.get_mut(&id1).unwrap().listened_ms = 10000;

        // id2: 5 plays, 20000 ms, "Zeta" (more listened time, same plays -> higher rank)
        stats.add_play(&id2, "Zeta", "Art");
        stats.tracks.get_mut(&id2).unwrap().play_count = 5;
        stats.tracks.get_mut(&id2).unwrap().listened_ms = 20000;

        // id3: 10 plays, 5000 ms, "Beta" (most plays -> rank 1)
        stats.add_play(&id3, "Beta", "Art");
        stats.tracks.get_mut(&id3).unwrap().play_count = 10;
        stats.tracks.get_mut(&id3).unwrap().listened_ms = 5000;

        // id4: 5 plays, 10000 ms, "Beta" (same plays, same time as id1, but "Beta" > "Alpha")
        stats.add_play(&id4, "Beta", "Art");
        stats.tracks.get_mut(&id4).unwrap().play_count = 5;
        stats.tracks.get_mut(&id4).unwrap().listened_ms = 10000;

        let sorted = stats.sorted_tracks();
        let ids: Vec<&str> = sorted.iter().map(|s| s.id.as_str()).collect();
        // Rank 1: id3 (10 plays)
        // Rank 2: id2 (5 plays, 20000 ms)
        // Rank 3: id1 (5 plays, 10000 ms, "Alpha" before "Beta")
        // Rank 4: id4 (5 plays, 10000 ms, "Beta")
        assert_eq!(
            ids,
            vec![id3.as_str(), id2.as_str(), id1.as_str(), id4.as_str()]
        );
    }

    #[test]
    fn accounting_pause_resume_stays_one_play() {
        let mut stats = SongStats::default();
        let mut accounting = PlaybackAccounting::new();
        let id = "0".repeat(22);
        let start = Instant::now();

        accounting.start_generation(1, Some(id.clone()), 200_000);
        accounting.on_playing(1, Some(id.clone()), 200_000, start);

        // Listen for 15 seconds in 5s increments
        for i in 1..=3 {
            accounting.account_time(
                start + Duration::from_secs(i * 5),
                &id,
                "Song",
                "Artist",
                &mut stats,
            );
        }
        assert_eq!(stats.tracks.get(&id).unwrap().play_count, 0);
        assert_eq!(stats.tracks.get(&id).unwrap().listened_ms, 15_000);

        // Pause
        accounting.last_accounted_at = None;

        // Resume after some pause gap
        let resume = start + Duration::from_secs(60);
        accounting.on_playing(1, Some(id.clone()), 200_000, resume);

        // Listen for another 15 seconds (reaching 30s) in 5s increments
        for i in 1..=3 {
            accounting.account_time(
                resume + Duration::from_secs(i * 5),
                &id,
                "Song",
                "Artist",
                &mut stats,
            );
        }
        assert_eq!(stats.tracks.get(&id).unwrap().play_count, 1);
        assert_eq!(stats.tracks.get(&id).unwrap().listened_ms, 30_000);

        // Continue playing for 20 more seconds in same generation
        for i in 4..=7 {
            accounting.account_time(
                resume + Duration::from_secs(i * 5),
                &id,
                "Song",
                "Artist",
                &mut stats,
            );
        }
        assert_eq!(stats.tracks.get(&id).unwrap().play_count, 1);
        assert_eq!(stats.tracks.get(&id).unwrap().listened_ms, 50_000);
    }

    #[test]
    fn accounting_seek_independence() {
        let mut stats = SongStats::default();
        let mut accounting = PlaybackAccounting::new();
        let id = "0".repeat(22);
        let start = Instant::now();

        accounting.start_generation(1, Some(id.clone()), 200_000);
        accounting.on_playing(1, Some(id.clone()), 200_000, start);

        // Only 5 seconds of real wall-clock pass
        accounting.account_time(
            start + Duration::from_secs(5),
            &id,
            "Song",
            "Artist",
            &mut stats,
        );
        assert_eq!(stats.tracks.get(&id).unwrap().play_count, 0);
        assert_eq!(stats.tracks.get(&id).unwrap().listened_ms, 5_000);
    }

    #[test]
    fn accounting_completion_and_repeat() {
        let mut stats = SongStats::default();
        let mut accounting = PlaybackAccounting::new();
        let id = "0".repeat(22);
        let start = Instant::now();

        // Short track: 10 seconds total duration. Threshold is 50% = 5 seconds.
        accounting.start_generation(1, Some(id.clone()), 10_000);
        accounting.on_playing(1, Some(id.clone()), 10_000, start);

        // Listen for 4 seconds (< 5s threshold)
        accounting.account_time(
            start + Duration::from_secs(4),
            &id,
            "Short Song",
            "Artist",
            &mut stats,
        );
        assert_eq!(stats.tracks.get(&id).unwrap().play_count, 0);

        // Track completes -> credits play if not already credited
        accounting.on_completed(1, &id, "Short Song", "Artist", &mut stats);
        assert_eq!(stats.tracks.get(&id).unwrap().play_count, 1);

        // Repeat track starts new generation 2
        let start2 = start + Duration::from_secs(12);
        accounting.start_generation(2, Some(id.clone()), 10_000);
        accounting.on_playing(2, Some(id.clone()), 10_000, start2);

        // Listen for 5 seconds -> earns second play
        accounting.account_time(
            start2 + Duration::from_secs(5),
            &id,
            "Short Song",
            "Artist",
            &mut stats,
        );
        assert_eq!(stats.tracks.get(&id).unwrap().play_count, 2);
    }

    #[test]
    fn accounting_stale_generation() {
        let mut stats = SongStats::default();
        let mut accounting = PlaybackAccounting::new();
        let id = "0".repeat(22);
        let start = Instant::now();

        accounting.start_generation(2, Some(id.clone()), 200_000);
        accounting.on_playing(2, Some(id.clone()), 200_000, start);

        // Stale generation 1 completion does nothing
        accounting.on_completed(1, &id, "Song", "Artist", &mut stats);
        assert_eq!(stats.tracks.get(&id), None);

        // Stale generation 1 on_playing does not change last_accounted_at
        let stale_time = start + Duration::from_secs(100);
        accounting.on_playing(1, Some(id.clone()), 200_000, stale_time);
        assert_eq!(accounting.last_accounted_at, Some(start));
    }

    #[test]
    fn accounting_clamping_interval() {
        let mut stats = SongStats::default();
        let mut accounting = PlaybackAccounting::new();
        let id = "0".repeat(22);
        let start = Instant::now();

        accounting.start_generation(1, Some(id.clone()), 200_000);
        accounting.on_playing(1, Some(id.clone()), 200_000, start);

        // Machine sleep of 3600 seconds (1 hour)
        accounting.account_time(
            start + Duration::from_secs(3600),
            &id,
            "Song",
            "Artist",
            &mut stats,
        );
        // Clamped to 5 seconds!
        assert_eq!(stats.tracks.get(&id).unwrap().listened_ms, 5_000);
        assert_eq!(accounting.generation_listened_ms, 5_000);
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(59_000), "0:59");
        assert_eq!(format_duration(65_000), "1:05");
        assert_eq!(format_duration(3_665_000), "1h 01m");
        assert_eq!(format_duration(72_000_000), "20h 00m");
    }

    #[test]
    fn eviction_ties_are_deterministic() {
        let mut stats = SongStats::default();
        let id_a = "a".repeat(22);
        let id_b = "b".repeat(22);
        let id_c = "c".repeat(22);

        stats.tracks.insert(
            id_b.clone(),
            TrackStat {
                id: id_b.clone(),
                name: "B".into(),
                artists: "Artist".into(),
                play_count: 0,
                listened_ms: 1000,
            },
        );
        stats.tracks.insert(
            id_c.clone(),
            TrackStat {
                id: id_c.clone(),
                name: "C".into(),
                artists: "Artist".into(),
                play_count: 0,
                listened_ms: 1000,
            },
        );
        stats.tracks.insert(
            id_a.clone(),
            TrackStat {
                id: id_a.clone(),
                name: "A".into(),
                artists: "Artist".into(),
                play_count: 0,
                listened_ms: 1000,
            },
        );

        let min_id = stats
            .tracks
            .iter()
            .min_by_key(|(id, s)| (s.play_count, s.listened_ms, (*id).as_str()))
            .map(|(k, _)| k.clone())
            .unwrap();
        assert_eq!(min_id, id_a);
    }
}
