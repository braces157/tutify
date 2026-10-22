//! Local ranking of provider-grounded candidates. No listening history is sent
//! to discovery services and preferences never introduce unrelated candidates.
use crate::{
    model::Track,
    stats::{FeedbackKind, SongStats},
};
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;

const DAY: u64 = 86_400;

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn artist_name(artists: &str) -> String {
    artists
        .split(',')
        .next()
        .unwrap_or("")
        .nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn recording(name: &str, artists: &str) -> (String, String) {
    (crate::catalog::normalize_title(name), artist_name(artists))
}

fn identity(artists: &str, ids: &[String]) -> String {
    ids.first().map_or_else(
        || format!("name:{}", artist_name(artists)),
        |id| format!("id:{id}"),
    )
}

#[derive(Default)]
struct Preference {
    plays: u64,
    enjoyed: f64,
    skips: f64,
    skip_count: usize,
    latest: Option<(u64, FeedbackKind)>,
}

#[derive(Default)]
pub(crate) struct Profile {
    recordings: HashMap<(String, String), Preference>,
    recent: HashMap<(String, String), Preference>,
    artists: HashMap<String, f64>,
    recent_artists: HashMap<String, f64>,
    at: u64,
}

impl Profile {
    pub fn new(stats: &SongStats, at: u64) -> Self {
        let mut profile = Self {
            at,
            ..Self::default()
        };
        for stat in stats.tracks.values().filter(|stat| stat.play_count > 0) {
            let key = recording(&stat.name, &stat.artists);
            if key.0.is_empty() || key.1.is_empty() {
                continue;
            }
            // Alternate catalog editions must not multiply a song's evidence.
            let entry = profile.recordings.entry(key).or_default();
            entry.plays = entry.plays.max(stat.play_count);
        }
        let mut plays = HashMap::<String, u64>::new();
        for ((_, artist), entry) in &profile.recordings {
            let total = plays.entry(artist.clone()).or_default();
            *total = total.saturating_add(entry.plays);
        }
        for (artist, plays) in plays {
            profile
                .artists
                .insert(artist, (plays as f64).ln_1p().min(3.0));
        }
        let mut recent = HashMap::<String, (f64, f64)>::new();
        let mut events: Vec<_> = stats.feedback.iter().collect();
        events.sort_by_key(|event| event.at);
        for event in events {
            let age = at.saturating_sub(event.at);
            if age > 90 * DAY {
                continue;
            }
            let weight = 2f64.powf(-(age as f64) / (14 * DAY) as f64);
            let key = (
                crate::catalog::normalize_title(&event.name),
                identity(&event.artists, &event.artist_ids),
            );
            let artist = recent.entry(key.1.clone()).or_default();
            let entry = profile.recent.entry(key).or_default();
            match event.kind {
                FeedbackKind::Enjoyed => {
                    entry.enjoyed += weight;
                    entry.skip_count = 0;
                    artist.0 += weight;
                }
                FeedbackKind::EarlySkip => {
                    entry.skips += weight;
                    artist.1 += weight;
                    if age <= 30 * DAY {
                        entry.skip_count += 1;
                    }
                }
            }
            // Vec order resolves events within the same second deterministically.
            if entry.latest.is_none_or(|(time, _)| event.at >= time) {
                entry.latest = Some((event.at, event.kind));
            }
        }
        for (artist, (enjoyed, skips)) in recent {
            // Shrink sparse evidence: one skip cannot condemn an artist.
            profile
                .recent_artists
                .insert(artist, 2.0 * (enjoyed - skips) / (3.0 + enjoyed + skips));
        }
        profile
    }

    fn preference(&self, track: &Track) -> Option<&Preference> {
        self.recent.get(&(
            crate::catalog::normalize_title(&track.name),
            identity(&track.artists, &track.artist_ids),
        ))
    }

    pub fn accepts(&self, track: &Track) -> bool {
        !self.preference(track).is_some_and(|entry| {
            entry.skip_count >= 2
                && entry.latest.is_some_and(|(at, kind)| {
                    kind == FeedbackKind::EarlySkip && self.at.saturating_sub(at) < 7 * DAY
                })
        })
    }

    pub fn score(&self, track: &Track) -> i32 {
        let mut score = self
            .artists
            .get(&artist_name(&track.artists))
            .copied()
            .unwrap_or(0.0);
        score += self
            .recent_artists
            .get(&identity(&track.artists, &track.artist_ids))
            .copied()
            .unwrap_or(0.0);
        if let Some(entry) = self.recordings.get(&recording(&track.name, &track.artists)) {
            score += (entry.plays as f64).ln_1p().min(2.0) * 0.35;
        }
        if let Some(entry) = self.preference(track) {
            score += entry.enjoyed.min(3.0) * 0.5 - entry.skips.min(3.0) * 2.5;
            if let Some((at, FeedbackKind::Enjoyed)) = entry.latest {
                // Fresh discoveries should beat a song heard earlier today,
                // without excluding familiar music from sparse catalogs.
                let age = self.at.saturating_sub(at);
                if age < DAY {
                    score -= 5.0 * (1.0 - age as f64 / DAY as f64);
                }
            }
        }
        (score * 1000.0).round() as i32
    }

    pub fn sort_pool(&self, tracks: &mut [Track]) {
        // Stable sorting preserves provider relevance when evidence is tied.
        tracks.sort_by_cached_key(|track| std::cmp::Reverse(self.score(track)));
    }

    pub fn rank(&self, mut tracks: Vec<Track>, tail: &[Track]) -> Vec<Track> {
        tracks.retain(|track| self.accepts(track));
        if self.recordings.is_empty() && self.recent.is_empty() {
            return tracks;
        }
        let primary = |track: &Track| {
            track
                .artist_ids
                .first()
                .cloned()
                .unwrap_or_else(|| artist_name(&track.artists))
        };
        let shares = |a: &Track, b: &Track| {
            if !a.artist_ids.is_empty() && !b.artist_ids.is_empty() {
                a.artist_ids.iter().any(|id| b.artist_ids.contains(id))
            } else {
                artist_name(&a.artists) == artist_name(&b.artists)
            }
        };
        let mut selected: Vec<Track> = Vec::with_capacity(tracks.len());
        while !tracks.is_empty() {
            let mut remaining = HashMap::<String, usize>::new();
            for track in &tracks {
                *remaining.entry(primary(track)).or_default() += 1;
            }
            let index = tracks
                .iter()
                .enumerate()
                .min_by_key(|(index, track)| {
                    let recent = selected.iter().rev().chain(tail.iter().rev());
                    let adjacent = recent
                        .clone()
                        .next()
                        .is_some_and(|last| shares(track, last));
                    let repeated = recent.take(3).filter(|last| shares(track, last)).count();
                    let album_repeat = selected.last().or(tail.last()).is_some_and(|last| {
                        track.album_id.is_some() && track.album_id == last.album_id
                    });
                    (
                        adjacent,
                        std::cmp::Reverse(remaining[&primary(track)]),
                        repeated,
                        std::cmp::Reverse(self.score(track) - i32::from(album_repeat) * 600),
                        *index,
                    )
                })
                .unwrap()
                .0;
            selected.push(tracks.remove(index));
        }
        selected
    }
}

#[cfg(test)]
mod tests;
