use crate::{catalog::RecommendationSource, model::Track};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

const MAX_PREVIEW_TRACKS: usize = 500;
pub const MAX_SOURCE_CANDIDATES: usize = 25_000;
pub const MAX_PLAYLIST_PAGES: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MixSource {
    Queue,
    Playlist { id: String, name: String },
}

impl MixSource {
    pub fn label(&self) -> &str {
        match self {
            Self::Queue => "Current queue",
            Self::Playlist { name, .. } => name,
        }
    }

    fn validate(&self) -> anyhow::Result<()> {
        if let Self::Playlist { id, name } = self {
            anyhow::ensure!(
                crate::model::valid_id(id),
                "Invalid playlist ID in mix recipe"
            );
            anyhow::ensure!(
                !name.trim().is_empty()
                    && name.chars().count() <= 200
                    && !name.chars().any(char::is_control),
                "Invalid playlist name in mix recipe"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MixSettings {
    pub target_minutes: u16,
    pub recommendation_percent: u8,
    /// Number of intervening tracks preferred before repeating an artist.
    pub artist_gap: u8,
}

impl Default for MixSettings {
    fn default() -> Self {
        Self {
            target_minutes: 45,
            recommendation_percent: 25,
            artist_gap: 2,
        }
    }
}

impl MixSettings {
    fn validate(self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=1_440).contains(&self.target_minutes),
            "Mix target must be between 1 minute and 24 hours"
        );
        anyhow::ensure!(
            self.recommendation_percent <= 100,
            "Mix recommendation percentage exceeds 100"
        );
        anyhow::ensure!(self.artist_gap <= 20, "Mix artist gap exceeds 20");
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Provenance {
    Source {
        label: String,
    },
    Recommendation {
        seed_id: String,
        seed_name: String,
        provider: RecommendationSource,
    },
}

impl Provenance {
    pub fn explanation(&self) -> String {
        match self {
            Self::Source { label } => format!("From {label}"),
            Self::Recommendation {
                seed_name,
                provider,
                ..
            } => format!("Suggested using {seed_name} ({})", provider.label()),
        }
    }

    fn is_recommendation(&self) -> bool {
        matches!(self, Self::Recommendation { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MixCandidate {
    pub track: Track,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MixEntry {
    pub track: Track,
    pub provenance: Provenance,
    pub pinned: bool,
    /// Index in the normalized candidate pool, so duplicate source occurrences
    /// remain distinct when a pinned preview is regenerated.
    pub candidate_index: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MixResult {
    pub entries: Vec<MixEntry>,
    pub duration_ms: u64,
    pub recommendation_percent: u8,
    pub note: String,
    pub invalid_pin_warning: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MixRecipe {
    pub name: String,
    pub source: MixSource,
    pub settings: MixSettings,
}

impl MixRecipe {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.name.trim().is_empty()
                && self.name.trim().chars().count() <= 80
                && !self.name.chars().any(char::is_control),
            "Invalid mix recipe name"
        );
        self.source.validate()?;
        self.settings.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecipeSave {
    Added,
    Updated,
    Unchanged,
    CapacityReached,
    Invalid(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MixRecipes {
    pub version: u32,
    pub recipes: Vec<MixRecipe>,
    #[serde(skip)]
    pub revision: u64,
}

impl MixRecipes {
    pub fn validate(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(self.version == 1, "Unsupported mix recipe version");
        anyhow::ensure!(self.recipes.len() <= 100, "Too many saved mix recipes");
        for recipe in &self.recipes {
            recipe.validate()?;
        }
        Ok(())
    }

    pub fn save(&mut self, mut recipe: MixRecipe) -> RecipeSave {
        recipe.name = recipe.name.trim().to_owned();
        if let Err(error) = recipe.validate() {
            return RecipeSave::Invalid(format!("{error:#}"));
        }
        if let Some(existing) = self
            .recipes
            .iter_mut()
            .find(|saved| saved.name.trim().eq_ignore_ascii_case(&recipe.name))
        {
            if existing == &recipe {
                return RecipeSave::Unchanged;
            }
            *existing = recipe;
            self.revision = self.revision.wrapping_add(1);
            return RecipeSave::Updated;
        }
        if self.recipes.len() >= 100 {
            return RecipeSave::CapacityReached;
        }
        self.recipes.push(recipe);
        self.revision = self.revision.wrapping_add(1);
        RecipeSave::Added
    }
}

impl Default for MixRecipes {
    fn default() -> Self {
        Self {
            version: 1,
            recipes: Vec::new(),
            revision: 0,
        }
    }
}

#[derive(Default)]
pub struct MixBuilder {
    pub open: bool,
    pub source: Option<MixSource>,
    pub settings: MixSettings,
    pub source_candidates: Vec<MixCandidate>,
    pub recommendation_candidates: Vec<MixCandidate>,
    pub preview: MixResult,
    pub selected: usize,
    pub generation: u64,
    pub request: u64,
    pub recommendation_request: u64,
    pub loading_source: bool,
    pub source_partial: bool,
    pub source_error: Option<String>,
    pub source_retryable: bool,
    pub source_failures: usize,
    pub source_pages: usize,
    pub loading_recommendations: bool,
    pub recommendation_error: Option<String>,
    pub naming: bool,
    pub recipe_name: String,
    pub recipe_selected: usize,
    pub detail: bool,
    pub detail_scroll: u16,
}

impl MixBuilder {
    pub fn regenerate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.preview = generate_pools(
            &self.source_candidates,
            &self.recommendation_candidates,
            self.settings,
            self.generation,
            &self.preview.entries,
            self.source_partial,
            self.recommendation_error.as_deref(),
        );
        self.selected = self
            .selected
            .min(self.preview.entries.len().saturating_sub(1));
    }

    pub fn refresh(&mut self) {
        let previous_warning = self.preview.invalid_pin_warning.clone();
        let mut preview = generate_pools(
            &self.source_candidates,
            &self.recommendation_candidates,
            self.settings,
            self.generation,
            &self.preview.entries,
            self.source_partial,
            self.recommendation_error.as_deref(),
        );
        if preview.invalid_pin_warning.is_none() {
            preview.invalid_pin_warning = previous_warning;
            if let Some(warning) = &preview.invalid_pin_warning {
                if !preview.note.is_empty() {
                    preview.note.push_str(". ");
                }
                preview.note.push_str(warning);
            }
        }
        self.preview = preview;
        self.selected = self
            .selected
            .min(self.preview.entries.len().saturating_sub(1));
    }
}

#[cfg(test)]
pub fn generate(
    candidates: &[MixCandidate],
    settings: MixSettings,
    generation: u64,
    previous: &[MixEntry],
    partial: bool,
    recommendation_error: Option<&str>,
) -> MixResult {
    generate_refs(
        candidates.iter().enumerate(),
        settings,
        generation,
        previous,
        partial,
        recommendation_error,
    )
}

fn generate_pools(
    source: &[MixCandidate],
    recommendations: &[MixCandidate],
    settings: MixSettings,
    generation: u64,
    previous: &[MixEntry],
    partial: bool,
    recommendation_error: Option<&str>,
) -> MixResult {
    generate_refs(
        source.iter().chain(recommendations).enumerate(),
        settings,
        generation,
        previous,
        partial,
        recommendation_error,
    )
}

fn generate_refs<'a>(
    candidates: impl Iterator<Item = (usize, &'a MixCandidate)>,
    settings: MixSettings,
    generation: u64,
    previous: &[MixEntry],
    partial: bool,
    recommendation_error: Option<&str>,
) -> MixResult {
    let raw: Vec<_> = candidates
        .filter(|(_, candidate)| candidate.track.playable && candidate.track.duration_ms > 0)
        .collect();
    let source_ids: HashSet<_> = raw
        .iter()
        .filter(|(_, candidate)| matches!(candidate.provenance, Provenance::Source { .. }))
        .map(|(_, candidate)| candidate.track.id.as_str())
        .collect();
    let mut recommendation_ids = HashSet::new();
    let normalized = raw
        .into_iter()
        .filter(|(_, candidate)| match &candidate.provenance {
            Provenance::Source { .. } => true,
            Provenance::Recommendation { .. } => {
                !source_ids.contains(candidate.track.id.as_str())
                    && recommendation_ids.insert(candidate.track.id.as_str())
            }
        })
        .collect();
    generate_normalized(
        normalized,
        settings,
        generation,
        previous,
        partial,
        recommendation_error,
    )
}

fn generate_normalized(
    candidates: Vec<(usize, &MixCandidate)>,
    settings: MixSettings,
    generation: u64,
    previous: &[MixEntry],
    partial: bool,
    recommendation_error: Option<&str>,
) -> MixResult {
    let target = u64::from(settings.target_minutes) * 60_000;
    let requested_pins: BTreeMap<usize, &MixEntry> = previous
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.pinned)
        .collect();
    let mut used = HashSet::new();
    let mut pinned = BTreeMap::new();
    let mut invalid_pins = Vec::new();
    // Resolve pins to the current pool. The stored candidate index identifies a
    // source occurrence; ID plus provenance prevents recommendation pins from
    // drifting to a different seed/provider when pools change.
    for (preview_position, entry) in requested_pins {
        let exact = candidates
            .iter()
            .enumerate()
            .find(|(position, (original, candidate))| {
                !used.contains(position)
                    && *original == entry.candidate_index
                    && candidate.track.id == entry.track.id
                    && candidate.provenance == entry.provenance
            });
        let fallback = || {
            candidates
                .iter()
                .enumerate()
                .find(|(position, (_, candidate))| {
                    !used.contains(position)
                        && candidate.track.id == entry.track.id
                        && candidate.provenance == entry.provenance
                })
        };
        if let Some((candidate_position, (original, _))) = exact.or_else(fallback) {
            used.insert(candidate_position);
            pinned.insert(preview_position, (candidate_position, *original));
        } else {
            invalid_pins.push(entry.track.name.clone());
        }
    }
    let last_pin = pinned.keys().next_back().copied().unwrap_or(0);
    let available_recommendations = candidates
        .iter()
        .filter(|(_, candidate)| candidate.provenance.is_recommendation())
        .count();
    let mut entries = Vec::new();
    let mut total = 0u64;
    let mut recommendation_duration = 0u64;
    let mut artist_spacing_limited = false;
    for position in 0..MAX_PREVIEW_TRACKS {
        if let Some((candidate_position, original)) = pinned.get(&position) {
            let candidate = candidates[*candidate_position].1;
            total += u64::from(candidate.track.duration_ms);
            if candidate.provenance.is_recommendation() {
                recommendation_duration += u64::from(candidate.track.duration_ms);
            }
            entries.push(MixEntry {
                track: candidate.track.clone(),
                provenance: candidate.provenance.clone(),
                pinned: true,
                candidate_index: *original,
            });
            continue;
        }
        let need_recommendation = settings.recommendation_percent > 0
            && recommendation_duration.saturating_mul(100)
                < total
                    .max(1)
                    .saturating_mul(u64::from(settings.recommendation_percent));
        if used.len() == candidates.len() || (total >= target && position > last_pin) {
            break;
        }
        let choice = candidates
            .iter()
            .enumerate()
            .filter(|(index, _)| !used.contains(index))
            .min_by_key(|(_, (original, candidate))| {
                let provider_penalty =
                    usize::from(candidate.provenance.is_recommendation() != need_recommendation);
                let artist_penalty = usize::from(artist_conflict(
                    &entries,
                    &candidate.track,
                    usize::from(settings.artist_gap),
                ));
                let after = total + u64::from(candidate.track.duration_ms);
                // Thirty-second buckets keep duration useful while allowing the
                // deterministic generation seed to choose realistic alternatives.
                let duration_penalty = after.abs_diff(target) / 30_000;
                (
                    provider_penalty,
                    artist_penalty,
                    duration_penalty,
                    stable_rank(&candidate.track.id, *original, generation),
                )
            });
        let Some((index, (original, candidate))) = choice else {
            break;
        };
        // Duration is a preference: stop when another unpinned track makes a non-empty mix worse.
        if total > 0
            && position > last_pin
            && total.abs_diff(target)
                < (total + u64::from(candidate.track.duration_ms)).abs_diff(target)
        {
            break;
        }
        artist_spacing_limited |=
            artist_conflict(&entries, &candidate.track, usize::from(settings.artist_gap));
        used.insert(index);
        total += u64::from(candidate.track.duration_ms);
        if candidate.provenance.is_recommendation() {
            recommendation_duration += u64::from(candidate.track.duration_ms);
        }
        entries.push(MixEntry {
            track: candidate.track.clone(),
            provenance: candidate.provenance.clone(),
            pinned: false,
            candidate_index: *original,
        });
    }
    let achieved = recommendation_duration
        .saturating_mul(100)
        .checked_div(total)
        .unwrap_or(0)
        .min(100) as u8;
    let mut notes = Vec::new();
    if candidates.is_empty() {
        notes.push("No playable tracks with known duration are available".to_string());
    } else if total.abs_diff(target) > 60_000 {
        notes.push(format!(
            "Candidate durations limited the target ({} min achieved)",
            (total + 30_000) / 60_000
        ));
    }
    if achieved.abs_diff(settings.recommendation_percent) > 5 {
        notes.push(if available_recommendations == 0 {
            "No recommendation candidates were available".into()
        } else {
            format!("Available candidates limited suggestions to {achieved}%")
        });
    }
    if artist_spacing_limited {
        notes.push(format!(
            "Available artists could not always maintain a {}-track gap",
            settings.artist_gap
        ));
    }
    let invalid_pin_warning = if !invalid_pins.is_empty() {
        let label = invalid_pins
            .first()
            .filter(|name| !name.is_empty())
            .map_or("A pinned track", String::as_str);
        Some(format!(
            "{label} became unavailable or disappeared and was unpinned; review the replacement before applying"
        ))
    } else {
        None
    };
    if let Some(warning) = &invalid_pin_warning {
        notes.push(warning.clone());
    }
    if partial {
        notes.push(
            "Source data is partial or capped; the preview does not represent every item".into(),
        );
    }
    if let Some(error) = recommendation_error {
        notes.push(format!(
            "Recommendations unavailable: {error}; using source tracks"
        ));
    }
    MixResult {
        entries,
        duration_ms: total,
        recommendation_percent: achieved,
        note: notes.join(". "),
        invalid_pin_warning,
    }
}

fn artist_conflict(entries: &[MixEntry], track: &Track, gap: usize) -> bool {
    if gap == 0 {
        return false;
    }
    entries.iter().rev().take(gap).any(|entry| {
        if !track.artist_ids.is_empty() && !entry.track.artist_ids.is_empty() {
            track
                .artist_ids
                .iter()
                .any(|id| entry.track.artist_ids.contains(id))
        } else {
            !track.artists.is_empty() && track.artists.eq_ignore_ascii_case(&entry.track.artists)
        }
    })
}

fn stable_rank(id: &str, occurrence: usize, generation: u64) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    splitmix64(
        hash ^ (occurrence as u64).rotate_left(21) ^ generation.wrapping_mul(0x9e3779b97f4a7c15),
    )
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(index: usize, artist: usize, duration: u32) -> Track {
        Track {
            id: format!("{index:022}"),
            name: format!("Song {index}"),
            artists: format!("Artist {artist}"),
            artist_ids: vec![format!("{artist:022}")],
            duration_ms: duration,
            playable: true,
            ..Default::default()
        }
    }

    fn source(tracks: Vec<Track>) -> Vec<MixCandidate> {
        tracks
            .into_iter()
            .map(|track| MixCandidate {
                track,
                provenance: Provenance::Source {
                    label: "your source playlist".into(),
                },
            })
            .collect()
    }

    #[test]
    fn duration_and_structured_artist_spacing_are_preferences() {
        let candidates = source(vec![
            track(1, 1, 600_000),
            track(2, 1, 600_000),
            track(3, 2, 600_000),
            track(4, 3, 600_000),
        ]);
        let result = generate(
            &candidates,
            MixSettings {
                target_minutes: 30,
                recommendation_percent: 0,
                artist_gap: 1,
            },
            0,
            &[],
            false,
            None,
        );
        assert_eq!(result.duration_ms, 1_800_000);
        assert!(
            result
                .entries
                .windows(2)
                .all(|pair| { pair[0].track.artist_ids[0] != pair[1].track.artist_ids[0] })
        );
    }

    #[test]
    fn regeneration_is_deterministic_and_preserves_pinned_positions() {
        let candidates = source((0..20).map(|i| track(i, i, 180_000)).collect());
        let settings = MixSettings::default();
        let first = generate(&candidates, settings, 1, &[], false, None);
        let same = generate(&candidates, settings, 1, &[], false, None);
        assert_eq!(first, same);
        let mut pinned = first.entries.clone();
        pinned[2].pinned = true;
        let regenerated = generate(&candidates, settings, 2, &pinned, false, None);
        assert_eq!(regenerated.entries[2], pinned[2]);
        assert_ne!(
            first
                .entries
                .iter()
                .map(|e| &e.track.id)
                .collect::<Vec<_>>(),
            regenerated
                .entries
                .iter()
                .map(|e| &e.track.id)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn candidate_edge_cases_are_bounded_and_explained() {
        let empty = generate(&[], MixSettings::default(), 0, &[], true, Some("offline"));
        assert!(empty.entries.is_empty());
        assert!(empty.note.contains("partial"));
        assert!(empty.note.contains("offline"));
        let mut unavailable = track(3, 3, 100_000);
        unavailable.playable = false;
        let duplicate = track(1, 1, 100_000);
        let result = generate(
            &source(vec![duplicate.clone(), duplicate, unavailable]),
            MixSettings {
                target_minutes: 30,
                ..Default::default()
            },
            0,
            &[],
            false,
            None,
        );
        assert_eq!(
            result.entries.len(),
            2,
            "duplicate occurrences are retained"
        );
        assert!(result.note.contains("limited"));
    }

    #[test]
    fn varied_durations_regenerate_reproducibly_with_feasible_alternatives() {
        let candidates = source(
            (0..30)
                .map(|i| track(i, i % 9, 150_000 + (i as u32 * 1_000)))
                .collect(),
        );
        let settings = MixSettings {
            target_minutes: 30,
            recommendation_percent: 0,
            artist_gap: 0,
        };
        let one = generate(&candidates, settings, 1, &[], false, None);
        let one_again = generate(&candidates, settings, 1, &[], false, None);
        let two = generate(&candidates, settings, 2, &[], false, None);
        assert_eq!(one, one_again);
        assert_ne!(
            one.entries
                .iter()
                .map(|entry| &entry.track.id)
                .collect::<Vec<_>>(),
            two.entries
                .iter()
                .map(|entry| &entry.track.id)
                .collect::<Vec<_>>()
        );
        assert!(one.duration_ms.abs_diff(30 * 60_000) <= 180_000);
    }

    #[test]
    fn recommendation_ratio_and_identity_rules_are_honest() {
        let mut candidates = source(vec![
            track(1, 1, 180_000),
            track(1, 1, 180_000),
            track(2, 2, 180_000),
        ]);
        let recommendation = |track: Track, seed: &str| MixCandidate {
            track,
            provenance: Provenance::Recommendation {
                seed_id: seed.repeat(22),
                seed_name: format!("Seed {seed}"),
                provider: RecommendationSource::Spotify,
            },
        };
        candidates.push(recommendation(track(1, 8, 180_000), "8"));
        candidates.push(recommendation(track(3, 3, 180_000), "8"));
        candidates.push(recommendation(track(3, 3, 180_000), "9"));
        let mut same_title = track(4, 4, 180_000);
        same_title.name = candidates[2].track.name.clone();
        candidates.push(recommendation(same_title, "9"));
        let result = generate(
            &candidates,
            MixSettings {
                target_minutes: 18,
                recommendation_percent: 50,
                artist_gap: 0,
            },
            4,
            &[],
            false,
            None,
        );
        assert_eq!(
            result
                .entries
                .iter()
                .filter(|entry| entry.track.id == format!("{:022}", 1))
                .count(),
            2
        );
        assert_eq!(
            result
                .entries
                .iter()
                .filter(|entry| entry.track.id == format!("{:022}", 3))
                .count(),
            1
        );
        assert!(
            result
                .entries
                .iter()
                .any(|entry| entry.track.id == format!("{:022}", 4))
        );
        assert!((40..=60).contains(&result.recommendation_percent));
    }

    #[test]
    fn pins_keep_duplicate_occurrence_identity_and_artist_limits_are_reported() {
        let duplicate = track(1, 1, 180_000);
        let candidates = source(vec![
            duplicate.clone(),
            duplicate,
            track(2, 1, 180_000),
            track(3, 1, 180_000),
        ]);
        let settings = MixSettings {
            target_minutes: 12,
            recommendation_percent: 0,
            artist_gap: 2,
        };
        let first = generate(&candidates, settings, 1, &[], false, None);
        let duplicate_positions: Vec<_> = first
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.track.id == format!("{:022}", 1))
            .map(|(position, _)| position)
            .collect();
        let pinned_position = duplicate_positions[1];
        let mut previous = first.entries.clone();
        previous[pinned_position].pinned = true;
        let occurrence = previous[pinned_position].candidate_index;
        let next = generate(&candidates, settings, 2, &previous, false, None);
        assert!(next.entries[pinned_position].pinned);
        assert_eq!(next.entries[pinned_position].candidate_index, occurrence);
        assert!(next.note.contains("could not always maintain"));
    }

    #[test]
    fn pins_resolve_fresh_metadata_and_release_missing_or_changed_provenance() {
        let settings = MixSettings {
            target_minutes: 12,
            recommendation_percent: 0,
            artist_gap: 1,
        };
        let original = source(vec![
            track(1, 1, 180_000),
            track(2, 2, 180_000),
            track(3, 3, 180_000),
            track(4, 4, 180_000),
        ]);
        let first = generate(&original, settings, 3, &[], false, None);
        let mut previous = first.entries.clone();
        previous[0].pinned = true;
        let pinned_id = previous[0].track.id.clone();
        let mut fresh = original.clone();
        let candidate = fresh
            .iter_mut()
            .find(|candidate| candidate.track.id == pinned_id)
            .unwrap();
        candidate.track.name = "Corrected title".into();
        candidate.track.duration_ms = 240_000;
        candidate.track.artist_ids = vec!["9".repeat(22)];
        let refreshed = generate(&fresh, settings, 3, &previous, false, None);
        assert!(refreshed.entries[0].pinned);
        assert_eq!(refreshed.entries[0].track.name, "Corrected title");
        assert_eq!(refreshed.entries[0].track.duration_ms, 240_000);
        assert_eq!(
            refreshed.duration_ms,
            fresh
                .iter()
                .map(|c| u64::from(c.track.duration_ms))
                .sum::<u64>()
        );

        fresh
            .iter_mut()
            .find(|candidate| candidate.track.id == pinned_id)
            .unwrap()
            .track
            .playable = false;
        let unavailable = generate(&fresh, settings, 3, &previous, false, None);
        assert!(
            !unavailable
                .entries
                .iter()
                .any(|entry| entry.track.id == pinned_id)
        );
        assert!(
            unavailable
                .note
                .contains("became unavailable or disappeared")
        );

        let removed: Vec<_> = original
            .iter()
            .filter(|candidate| candidate.track.id != pinned_id)
            .cloned()
            .collect();
        let disappeared = generate(&removed, settings, 3, &previous, false, None);
        assert!(disappeared.note.contains("was unpinned"));
    }

    #[test]
    fn recommendation_pin_requires_the_same_seed_and_provider() {
        let recommendation = |seed: &str, name: &str| MixCandidate {
            track: Track {
                id: "8".repeat(22),
                name: name.into(),
                artists: "Suggested artist".into(),
                artist_ids: vec!["7".repeat(22)],
                duration_ms: 180_000,
                playable: true,
                ..Default::default()
            },
            provenance: Provenance::Recommendation {
                seed_id: seed.repeat(22),
                seed_name: format!("Seed {seed}"),
                provider: RecommendationSource::Spotify,
            },
        };
        let settings = MixSettings {
            target_minutes: 3,
            recommendation_percent: 100,
            artist_gap: 0,
        };
        let initial = vec![recommendation("1", "Old metadata")];
        let mut previous = generate(&initial, settings, 1, &[], false, None).entries;
        previous[0].pinned = true;
        let same_provenance = generate(
            &[recommendation("1", "Fresh metadata")],
            settings,
            2,
            &previous,
            false,
            None,
        );
        assert!(same_provenance.entries[0].pinned);
        assert_eq!(same_provenance.entries[0].track.name, "Fresh metadata");
        let changed_seed = generate(
            &[recommendation("2", "Different seed")],
            settings,
            2,
            &previous,
            false,
            None,
        );
        assert!(!changed_seed.entries[0].pinned);
        assert!(changed_seed.note.contains("was unpinned"));
    }

    fn recipe(name: impl Into<String>) -> MixRecipe {
        MixRecipe {
            name: name.into(),
            source: MixSource::Queue,
            settings: MixSettings::default(),
        }
    }

    #[test]
    fn recipe_capacity_updates_and_revisions_match_real_mutations() {
        let mut recipes = MixRecipes::default();
        for index in 0..99 {
            assert_eq!(
                recipes.save(recipe(format!("Recipe {index}"))),
                RecipeSave::Added
            );
        }
        assert_eq!(recipes.recipes.len(), 99);
        assert_eq!(recipes.save(recipe("  Last recipe  ")), RecipeSave::Added);
        let full_revision = recipes.revision;
        assert_eq!(
            recipes.save(recipe("Overflow")),
            RecipeSave::CapacityReached
        );
        assert_eq!(recipes.revision, full_revision);
        assert_eq!(recipes.save(recipe("last recipe")), RecipeSave::Updated);
        assert_eq!(recipes.revision, full_revision + 1);
        let unchanged_revision = recipes.revision;
        assert_eq!(recipes.save(recipe("last recipe")), RecipeSave::Unchanged);
        assert_eq!(recipes.revision, unchanged_revision);
        assert_eq!(recipes.recipes.len(), 100);
    }

    #[test]
    fn recipe_validation_rejects_malformed_sources_and_settings() {
        let invalid_source = MixRecipe {
            name: "Bad source".into(),
            source: MixSource::Playlist {
                id: "short".into(),
                name: "Playlist".into(),
            },
            settings: MixSettings::default(),
        };
        assert!(invalid_source.validate().is_err());
        for settings in [
            MixSettings {
                target_minutes: 0,
                ..MixSettings::default()
            },
            MixSettings {
                recommendation_percent: 101,
                ..MixSettings::default()
            },
            MixSettings {
                artist_gap: 21,
                ..MixSettings::default()
            },
        ] {
            assert!(
                MixRecipe {
                    name: "Bad settings".into(),
                    source: MixSource::Queue,
                    settings
                }
                .validate()
                .is_err()
            );
        }
    }

    #[test]
    #[ignore = "Release-only Mix Builder generation benchmark"]
    fn benchmark_mix_generation_work() {
        use std::time::Instant;
        for count in [500usize, 5_000, 25_000, 100_000] {
            let candidates = source(
                (0..count)
                    .map(|i| track(i, i % 250, 150_000 + (i % 121) as u32 * 1_000))
                    .collect(),
            );
            let started = Instant::now();
            let result = generate(&candidates, MixSettings::default(), 7, &[], false, None);
            println!(
                "{count} candidates: {:?}, {} entries",
                started.elapsed(),
                result.entries.len()
            );
            assert!(!result.entries.is_empty());
        }
    }
}
