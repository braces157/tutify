use super::*;
use std::collections::VecDeque;

const BATCH_SIZE: usize = 15;
const ARTIST_LIMIT: usize = 3;
const ROOT_LIMIT: usize = 2;
const NEIGHBOURS_PER_ROUND: usize = 5;
const MAX_NEIGHBOURS: usize = 12;

fn artist_key(track: &Track) -> String {
    // Count all credits below, but use the primary credit for balancing.
    track.artist_ids.first().cloned().unwrap_or_else(|| {
        track
            .artists
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase()
    })
}

struct Eligibility {
    ids: HashSet<String>,
    recordings: HashSet<(String, String)>,
    preferences: Option<Arc<crate::recommendation::Profile>>,
}

impl Eligibility {
    fn new(seed: &Track, excluded: &[Track]) -> Self {
        let context = || excluded.iter().chain(std::iter::once(seed));
        Self {
            ids: context().map(|track| track.id.clone()).collect(),
            recordings: context().map(crate::catalog::recording_key).collect(),
            preferences: None,
        }
    }

    fn accepts(&self, track: &Track) -> bool {
        track.playable
            && !track.name.trim().is_empty()
            && !track.artists.trim().is_empty()
            && track.duration_ms > 0
            && !crate::catalog::discovery::special_version(track)
            && !self.ids.contains(&track.id)
            && !self
                .recordings
                .contains(&crate::catalog::recording_key(track))
            && self
                .preferences
                .as_ref()
                .is_none_or(|profile| profile.accepts(track))
    }
}

impl Client {
    /// Sparse artist-top responses are not evidence of a sparse discography.
    /// Search supplies missing recordings, with numeric credit verification to
    /// reject covers, same-name artists and broad-query noise.
    async fn recommendation_pool(
        &self,
        id: &str,
        name: &str,
        eligibility: &Eligibility,
        offset: usize,
    ) -> Result<Vec<Track>> {
        let page = self.page(&Browse::Artist(id.into()), offset).await;
        let mut tracks = match page {
            Ok(Page {
                rows: Rows::Tracks(tracks),
                ..
            }) => tracks,
            Err(error) if ServiceFailure::is(&error, FailureKind::MissingItem) => vec![],
            Err(error) => return Err(error),
            _ => unreachable!(),
        };
        tracks.retain(|track| track.artist_ids.first().is_some_and(|credit| credit == id));
        tracks = crate::catalog::discovery::unique_tracks(tracks);
        if tracks
            .iter()
            .filter(|track| eligibility.accepts(track))
            .count()
            < ARTIST_LIMIT
            && !name.trim().is_empty()
        {
            let value = match self
                .get(
                    "/search",
                    &[
                        ("q", name.into()),
                        ("index", offset.to_string()),
                        ("limit", PAGE_SIZE.to_string()),
                    ],
                )
                .await
            {
                Ok(value) => value,
                Err(error) if ServiceFailure::is(&error, FailureKind::MissingItem) => {
                    return Ok(tracks
                        .into_iter()
                        .filter(|track| eligibility.accepts(track))
                        .collect());
                }
                Err(error) => return Err(error),
            };
            let items = value["data"].as_array().ok_or_else(|| {
                ServiceFailure::new(Provider::Deezer, FailureKind::InvalidResponse)
            })?;
            tracks.extend(
                items
                    .iter()
                    .filter_map(parse_track)
                    .filter(|track| track.artist_ids.first().is_some_and(|credit| credit == id)),
            );
        }
        let mut tracks: Vec<_> = crate::catalog::discovery::unique_tracks(tracks)
            .into_iter()
            .filter(|track| eligibility.accepts(track))
            .collect();
        if let Some(profile) = &self.preferences {
            profile.sort_pool(&mut tracks);
        }
        Ok(tracks)
    }

    pub async fn recommendations(
        &self,
        seed: &Track,
        excluded: &[Track],
        round: usize,
    ) -> Result<Recommendations> {
        let root = seed
            .artist_ids
            .iter()
            .find(|id| key(id).is_some())
            .context("Deezer artist metadata is unavailable; refresh the track")?;
        let related = self
            .get(
                &format!("/artist/{}/related", key(root).unwrap()),
                &[("limit", "30".into())],
            )
            .await?;
        let artists = related["data"]
            .as_array()
            .ok_or_else(|| ServiceFailure::new(Provider::Deezer, FailureKind::InvalidResponse))?;
        let mut identities = HashSet::from([root.clone()]);
        let neighbours: Vec<_> = artists
            .iter()
            .filter_map(|artist| {
                let id = identity(&artist["id"])?;
                identities
                    .insert(id.clone())
                    .then(|| (id, artist["name"].as_str().unwrap_or("").to_string()))
            })
            .collect();
        // Move through close neighbours without ever changing the original
        // seed. Overlapping pages retain unselected songs on later visits.
        let cycles = neighbours.len().div_ceil(NEIGHBOURS_PER_ROUND).max(1);
        let offset = (round / cycles).saturating_mul(ARTIST_LIMIT);
        let mut candidates = Vec::new();
        let mut eligibility = Eligibility::new(seed, excluded);
        eligibility.preferences = self.preferences.clone();
        let root_name = seed.artists.split(',').next().unwrap_or("");
        let root_tracks = self
            .recommendation_pool(root, root_name, &eligibility, offset)
            .await?;
        candidates.extend(root_tracks);
        for step in 0..neighbours.len().min(MAX_NEIGHBOURS) {
            let index = (round % neighbours.len() * NEIGHBOURS_PER_ROUND + step) % neighbours.len();
            let (id, name) = &neighbours[index];
            let tracks = self
                .recommendation_pool(id, name, &eligibility, offset)
                .await?;
            candidates.extend(tracks);
            // Count usable artist capacity, not raw provider rows. An artist
            // with twenty songs cannot hide five exhausted neighbours.
            let selected = balanced(&candidates, seed, excluded);
            let distinct = selected
                .iter()
                .map(artist_key)
                .collect::<HashSet<_>>()
                .len();
            if selected.len() == BATCH_SIZE && distinct >= NEIGHBOURS_PER_ROUND {
                break;
            }
        }
        Ok(Recommendations {
            tracks: balanced(&candidates, seed, excluded),
            source: RecommendationSource::Deezer,
        })
    }
}

fn balanced(candidates: &[Track], seed: &Track, excluded: &[Track]) -> Vec<Track> {
    let root = artist_key(seed);
    let eligibility = Eligibility::new(seed, excluded);
    let mut pools = Vec::<(String, VecDeque<Track>)>::new();
    let mut seen = HashSet::new();
    let mut recordings = HashSet::new();
    for track in candidates.iter().filter(|track| eligibility.accepts(track)) {
        if !seen.insert(track.id.clone())
            || !recordings.insert(crate::catalog::recording_key(track))
        {
            continue;
        }
        let artist = artist_key(track);
        if let Some((_, pool)) = pools.iter_mut().find(|(id, _)| id == &artist) {
            pool.push_back(track.clone());
        } else {
            pools.push((artist, VecDeque::from([track.clone()])));
        }
    }
    let mut history = HashMap::<String, usize>::new();
    for track in excluded {
        *history.entry(artist_key(track)).or_default() += 1;
    }
    let mut counts = HashMap::<String, usize>::new();
    let mut selected = Vec::<Track>::new();
    while selected.len() < BATCH_SIZE {
        for (_, pool) in &mut pools {
            pool.retain(|track| {
                track.artist_ids.iter().all(|credit| {
                    counts.get(credit).copied().unwrap_or(0)
                        < if credit == &root {
                            ROOT_LIMIT
                        } else {
                            ARTIST_LIMIT
                        }
                })
            });
        }
        let choice = pools
            .iter()
            .enumerate()
            .filter(|(_, (_, pool))| !pool.is_empty())
            .min_by_key(|(index, (id, pool))| {
                let history_tail = selected.iter().rev().chain(excluded.iter().rev());
                let shares = |previous: &Track| {
                    previous
                        .artist_ids
                        .iter()
                        .any(|credit| pool.front().unwrap().artist_ids.contains(credit))
                };
                let adjacent = history_tail.clone().next().is_some_and(shares);
                let recent = history_tail.take(2).any(shares);
                let count = counts.get(id).copied().unwrap_or(0);
                let remaining = pool.len().min(
                    (if id == &root {
                        ROOT_LIMIT
                    } else {
                        ARTIST_LIMIT
                    }) - count,
                );
                // Schedule fuller pools early so scarce alternatives remain
                // available to separate repeats near the end of a short batch.
                (
                    adjacent,
                    std::cmp::Reverse(remaining),
                    recent,
                    count,
                    history.get(id).copied().unwrap_or(0),
                    *index,
                )
            })
            .map(|(index, _)| index);
        let Some(index) = choice else { break };
        let track = pools[index].1.pop_front().unwrap();
        for credit in &track.artist_ids {
            *counts.entry(credit.clone()).or_default() += 1;
        }
        selected.push(track);
    }
    selected
}

#[cfg(test)]
mod tests;
