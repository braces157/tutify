use super::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Default)]
pub(super) struct Profile {
    pub(super) batches: HashMap<usize, Vec<Track>>,
    albums: Option<Vec<Value>>,
    next_album: usize,
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace(['"', '\\'], " "))
}

fn same_artist(a: &Track, b: &Track) -> bool {
    if !a.artist_ids.is_empty() && !b.artist_ids.is_empty() {
        return a.artist_ids.iter().any(|id| b.artist_ids.contains(id));
    }
    a.artists.split(',').any(|artist| {
        !artist.trim().is_empty()
            && b.artists
                .split(',')
                .any(|other| artist.trim().eq_ignore_ascii_case(other.trim()))
    })
}

pub(crate) fn recording(track: &Track) -> (String, String) {
    (
        normalize_title(&track.name),
        track
            .artists
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase(),
    )
}

fn special_version(track: &Track) -> bool {
    let name = track.name.to_lowercase();
    [
        "remix",
        "karaoke",
        "tribute",
        "sped up",
        "slowed",
        "instrumental",
        "piano version",
        "symphony",
        "arr.",
        " - live",
        "(live",
        "[live",
    ]
    .iter()
    .any(|word| name.contains(word))
}

fn add_artists(artists: &mut Vec<Track>, track: &Track) {
    let parts: Vec<_> = track.artists.split(',').map(str::trim).collect();
    for (index, name) in parts.iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        let mut artist = track.clone();
        artist.artists = name.to_string();
        artist.artist_ids = if parts.len() == track.artist_ids.len() {
            vec![track.artist_ids[index].clone()]
        } else {
            vec![]
        };
        if !artists.iter().any(|known| same_artist(known, &artist)) {
            artists.push(artist);
        }
    }
}

pub(super) fn unique_tracks(tracks: impl IntoIterator<Item = Track>) -> Vec<Track> {
    let mut ids = HashSet::new();
    let mut recordings = HashSet::new();
    tracks
        .into_iter()
        .filter(|track| {
            track.playable
                && !special_version(track)
                && ids.insert(track.id.clone())
                && recordings.insert(recording(track))
        })
        .collect()
}

impl Catalog {
    /// Search can return few hits even for artists with multiple albums.
    /// Fill that gap from verified album tracks, never an unanchored genre query.
    async fn artist_candidates(
        &self,
        artist: &Track,
        round: usize,
        albums_allowed: bool,
    ) -> Result<Vec<Track>> {
        let key = format!(
            "{}:{}",
            artist
                .artist_ids
                .first()
                .map(String::as_str)
                .unwrap_or(&artist.artists),
            albums_allowed
        );
        let mut profile = self
            .discovery
            .lock()
            .await
            .get(&key)
            .cloned()
            .unwrap_or_default();
        if let Some(batch) = profile.batches.get(&round) {
            return Ok(batch.clone());
        }
        let mut candidates = Vec::new();
        for page in 0..2 {
            let value = self
                .get(
                    "/search",
                    &[
                        ("q", format!("artist:{}", quoted(&artist.artists))),
                        ("type", "track".into()),
                        ("limit", "10".into()),
                        ("offset", ((round * 2 + page) * 10).to_string()),
                    ],
                )
                .await?;
            candidates.extend(
                value["tracks"]["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(parse_track)
                    .filter(|track| same_artist(artist, track)),
            );
            if value["tracks"].get("next").is_some_and(Value::is_null) {
                break;
            }
        }
        candidates = unique_tracks(candidates);
        if albums_allowed && candidates.len() < 20 {
            if let Some(id) = artist.artist_ids.first().filter(|id| valid_id(id)) {
                if profile.albums.is_none() {
                    let value = self
                        .get(
                            &format!("/artists/{id}/albums"),
                            &[
                                ("include_groups", "album,single".into()),
                                ("limit", "10".into()),
                            ],
                        )
                        .await?;
                    let mut titles = HashSet::new();
                    let mut albums: Vec<_> = value["items"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|album| album["id"].as_str().is_some_and(valid_id))
                        .filter(|album| {
                            titles.insert(album["name"].as_str().unwrap_or("").to_lowercase())
                        })
                        .cloned()
                        .collect();
                    // Full albums before singles. Tracks on soundtracks must
                    // still have the actual searched artist in their credits.
                    albums.sort_by_key(|album| album["album_type"].as_str() != Some("album"));
                    profile.albums = Some(albums);
                }
                for _ in 0..3 {
                    let Some(album) = profile
                        .albums
                        .as_ref()
                        .and_then(|albums| albums.get(profile.next_album))
                    else {
                        break;
                    };
                    let id = album["id"].as_str().expect("validated album ID");
                    let value = self
                        .get(
                            &format!("/albums/{id}/tracks"),
                            &[("limit", "50".into()), ("offset", "0".into())],
                        )
                        .await?;
                    profile.next_album += 1;
                    candidates.extend(
                        value["items"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|item| {
                                let mut item = item.clone();
                                item["album"] = album.clone();
                                parse_track(&item)
                            })
                            .filter(|track| same_artist(artist, track)),
                    );
                    candidates = unique_tracks(candidates);
                    if candidates.len() >= 20 {
                        break;
                    }
                }
            }
        }
        // Preserve unselected recordings for the next refill.
        if round > 0 {
            if let Some(previous) = profile.batches.get(&(round - 1)) {
                candidates.extend(previous.clone());
                candidates = unique_tracks(candidates);
            }
        }
        if profile.batches.len() >= 2 {
            profile.batches.clear();
        }
        profile.batches.insert(round, candidates.clone());
        let mut profiles = self.discovery.lock().await;
        if profiles.len() >= 16 && !profiles.contains_key(&key) {
            profiles.clear();
        }
        profiles.insert(key, profile);
        Ok(candidates)
    }

    async fn connected_candidates(
        &self,
        seed: &Track,
        context: &[Track],
        round: usize,
    ) -> Result<Vec<Track>> {
        if round >= 50 {
            return Ok(vec![]);
        }
        let mut artists = Vec::new();
        add_artists(&mut artists, seed);
        for track in context.iter().filter(|track| same_artist(seed, track)) {
            add_artists(&mut artists, track);
        }
        let Some(primary) = artists.first().cloned() else {
            return Ok(vec![]);
        };
        let mut candidates = self.artist_candidates(&primary, round, true).await?;
        // Only credits on verified ROOT artist tracks establish discovery links.
        // Never walk collaborators-of-collaborators or injected suggestions.
        let mut inferred = Vec::new();
        for track in &candidates {
            add_artists(&mut inferred, track);
        }
        // A one-off guest credit is too weak to redirect a station. Explicit
        // seed/queue collaborations above are intentional context; inferred
        // neighbours need two distinct, non-special recordings with the root.
        inferred.sort_by_key(|artist| {
            std::cmp::Reverse(
                candidates
                    .iter()
                    .filter(|track| same_artist(artist, track))
                    .count(),
            )
        });
        for artist in inferred {
            if candidates
                .iter()
                .filter(|track| same_artist(&artist, track))
                .count()
                >= 2
            {
                add_artists(&mut artists, &artist);
            }
        }
        artists.truncate(3);
        for artist in artists.iter().skip(1) {
            candidates.extend(self.artist_candidates(artist, round, false).await?);
        }
        Ok(unique_tracks(candidates))
    }

    pub(super) async fn queue_context_recommendations(
        &self,
        seed: &Track,
        excluded: &[Track],
        context: &[Track],
    ) -> Result<Recommendations> {
        let similar = self.similarity.is_some();
        let mut tracks = if similar {
            self.similarity_candidates(seed, 0).await?
        } else {
            self.connected_candidates(seed, context, 0).await?
        };
        let ids: HashSet<_> = excluded
            .iter()
            .chain(std::iter::once(seed))
            .map(|track| &track.id)
            .collect();
        let recordings: HashSet<_> = excluded
            .iter()
            .chain(std::iter::once(seed))
            .map(recording)
            .collect();
        tracks.retain(|track| !ids.contains(&track.id) && !recordings.contains(&recording(track)));
        Ok(Recommendations {
            tracks,
            source: if similar {
                RecommendationSource::SimilarArtists
            } else {
                RecommendationSource::ArtistSearch
            },
        })
    }

    /// Every refill stays anchored to the user's original seed.
    pub async fn radio_recommendations(
        &self,
        seed: &Track,
        excluded: &[Track],
        round: usize,
    ) -> Result<Recommendations> {
        let similar = self.similarity.is_some();
        let mut candidates = if similar {
            self.similarity_candidates(seed, round).await?
        } else {
            self.connected_candidates(seed, &[], round).await?
        };
        let mut ids: HashSet<_> = excluded
            .iter()
            .chain(std::iter::once(seed))
            .map(|track| track.id.clone())
            .collect();
        let mut recordings: HashSet<_> = excluded
            .iter()
            .chain(std::iter::once(seed))
            .map(recording)
            .collect();
        candidates
            .retain(|track| !ids.contains(&track.id) && !recordings.contains(&recording(track)));
        let mut selected: Vec<Track> = Vec::new();
        let mut counts = HashMap::<String, usize>::new();
        while selected.len() < 15 && !candidates.is_empty() {
            let index = candidates
                .iter()
                .enumerate()
                .filter(|(_, track)| {
                    !similar || counts.get(&recording(track).1).copied().unwrap_or(0) < 3
                })
                .min_by_key(|(index, track)| {
                    let recent = selected
                        .iter()
                        .rev()
                        .take(3)
                        .filter(|previous| same_artist(previous, track))
                        .count();
                    (
                        recent,
                        counts.get(&recording(track).1).copied().unwrap_or(0),
                        *index,
                    )
                })
                .map(|(index, _)| index);
            let Some(index) = index else { break };
            let track = candidates.remove(index);
            if !ids.insert(track.id.clone()) || !recordings.insert(recording(&track)) {
                continue;
            }
            *counts.entry(recording(&track).1).or_default() += 1;
            selected.push(track);
        }
        Ok(Recommendations {
            tracks: selected,
            source: if similar {
                RecommendationSource::SimilarArtists
            } else {
                RecommendationSource::ArtistSearch
            },
        })
    }
}
