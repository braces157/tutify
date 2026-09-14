use super::*;

const SMART_MAX_SEED_REQUESTS: usize = 3;
const SMART_ARTIST_GAP: usize = 3;

fn artist_keys(track: &Track) -> Vec<String> {
    if !track.artist_ids.is_empty() {
        return track
            .artist_ids
            .iter()
            .map(|id| format!("id:{id}"))
            .collect();
    }
    track
        .artists
        .split(',')
        .map(str::trim)
        .filter(|artist| !artist.is_empty())
        .map(|artist| format!("name:{}", artist.to_lowercase()))
        .collect()
}

fn shares_artist(a: &Track, b: &Track) -> bool {
    if a.artist_ids.is_empty() || b.artist_ids.is_empty() {
        return a.artists.split(',').map(str::trim).any(|artist| {
            !artist.is_empty()
                && b.artists
                    .split(',')
                    .any(|other| artist.eq_ignore_ascii_case(other.trim()))
        });
    }
    let a = artist_keys(a);
    let b: HashSet<_> = artist_keys(b).into_iter().collect();
    a.iter().any(|key| b.contains(key))
}

fn index_shares_artist(app: &App, left: usize, right: usize) -> bool {
    let Some(left) = app.cache.get(&app.queue.ids[left]) else {
        return false;
    };
    let Some(right) = app.cache.get(&app.queue.ids[right]) else {
        return false;
    };
    shares_artist(left, right)
}

/// Re-balance only the unplayed portion of the already-shuffled queue.
/// The existing shuffled order is the tie-breaker, so this only intervenes
/// when it can avoid a recent artist repeat with another queued track.
pub(super) fn diversify_smart_order(app: &mut App) {
    let Some(cursor) = app.queue.cursor else {
        return;
    };
    if cursor + 1 >= app.queue.order.len() {
        return;
    }

    let selected_entry = app.queue.order.get(app.queue.selected).copied();
    let prefix = app.queue.order[..=cursor].to_vec();
    let remaining = app.queue.order[cursor + 1..].to_vec();
    let mut groups = std::collections::HashMap::<String, VecDeque<usize>>::new();
    let mut first_position = std::collections::HashMap::<String, usize>::new();
    for (position, &index) in remaining.iter().enumerate() {
        let key = app
            .cache
            .get(&app.queue.ids[index])
            .and_then(|track| artist_keys(track).into_iter().next())
            .unwrap_or_else(|| format!("track:{index}"));
        first_position.entry(key.clone()).or_insert(position);
        groups.entry(key).or_default().push_back(index);
    }

    // Artists with more remaining songs get scheduled earlier, which prevents
    // a repeated artist from being deferred until all diverse tracks are gone.
    // The existing shuffled position breaks ties so Smart Shuffle stays random.
    let mut heap = std::collections::BinaryHeap::new();
    for (key, entries) in &groups {
        heap.push((
            entries.len(),
            std::cmp::Reverse(first_position[key]),
            key.clone(),
        ));
    }

    let mut arranged = Vec::with_capacity(remaining.len());
    let mut previous = prefix.last().copied();
    while !heap.is_empty() {
        let mut deferred = Vec::new();
        let mut chosen = None;
        while let Some(entry) = heap.pop() {
            let candidate = *groups[&entry.2]
                .front()
                .expect("smart shuffle artist group cannot be empty");
            if previous.is_none_or(|previous| !index_shares_artist(app, candidate, previous)) {
                chosen = Some(entry);
                break;
            }
            deferred.push(entry);
        }
        let entry = chosen.unwrap_or_else(|| deferred.remove(0));
        for deferred_entry in deferred {
            heap.push(deferred_entry);
        }

        let queue = groups
            .get_mut(&entry.2)
            .expect("smart shuffle artist group must exist");
        let next = queue
            .pop_front()
            .expect("smart shuffle artist group cannot be empty");
        arranged.push(next);
        previous = Some(next);
        if !queue.is_empty() {
            heap.push((
                queue.len(),
                std::cmp::Reverse(first_position[&entry.2]),
                entry.2,
            ));
        }
    }

    let changed = arranged != app.queue.order[cursor + 1..];
    if !changed {
        return;
    }
    app.queue.order.truncate(cursor + 1);
    app.queue.order.extend(arranged);
    if let Some(selected_entry) = selected_entry {
        if let Some(selected) = app
            .queue
            .order
            .iter()
            .position(|&entry| entry == selected_entry)
        {
            app.queue.selected = selected;
        }
    }
    app.queue.revision += 1;
}

fn nearby_artist_conflict(app: &App, slot: usize, candidate: &Track) -> bool {
    let start = slot.saturating_sub(SMART_ARTIST_GAP);
    let end = (slot + SMART_ARTIST_GAP).min(app.queue.order.len());
    app.queue.order[start..end].iter().any(|&index| {
        app.cache
            .get(&app.queue.ids[index])
            .is_some_and(|track| shares_artist(track, candidate))
    })
}

pub(super) fn diverse_candidates(app: &App, tracks: &[Track]) -> Vec<Track> {
    let slots = app.queue.smart_slots();
    if slots.is_empty() {
        return Vec::new();
    }

    let queued: HashSet<_> = app.queue.ids.iter().cloned().collect();
    let mut recordings = HashSet::new();
    let recording_keys = |track: &Track| {
        let title = crate::catalog::normalize_title(&track.name);
        track
            .artists
            .split(',')
            .map(str::trim)
            .filter(|artist| !artist.is_empty() && !title.is_empty())
            .map(|artist| (title.clone(), artist.to_lowercase()))
            .collect::<Vec<_>>()
    };
    for id in &app.queue.ids {
        if let Some(track) = app.cache.get(id) {
            recordings.extend(recording_keys(track));
        }
    }
    let mut used_ids = HashSet::new();
    let mut used_artists = std::collections::HashMap::<String, usize>::new();
    for &index in &app.queue.suggestions {
        if let Some(track) = app.cache.get(&app.queue.ids[index]) {
            for artist in artist_keys(track) {
                *used_artists.entry(artist).or_default() += 1;
            }
        }
    }

    let eligible =
        |track: &Track, used_ids: &HashSet<String>, recordings: &HashSet<(String, String)>| {
            track.playable
                && !queued.contains(&track.id)
                && !used_ids.contains(&track.id)
                && recording_keys(track)
                    .iter()
                    .all(|key| !recordings.contains(key))
        };

    let mut selected = Vec::new();
    for slot in slots {
        let strict = tracks.iter().find(|track| {
            eligible(track, &used_ids, &recordings)
                && artist_keys(track)
                    .iter()
                    .all(|artist| !used_artists.contains_key(artist))
                && !nearby_artist_conflict(app, slot, track)
        });
        let choice = strict.or_else(|| {
            tracks.iter().find(|track| {
                eligible(track, &used_ids, &recordings)
                    && artist_keys(track)
                        .iter()
                        .all(|artist| !used_artists.contains_key(artist))
            })
        });
        // A narrow but relevant pool can fill further slots. The queue already
        // separates suggestions with three originals; do not impose a lifetime
        // one-song-per-artist cap that starves single-artist playlists.
        let choice = choice.or_else(|| {
            tracks
                .iter()
                .filter(|track| eligible(track, &used_ids, &recordings))
                .min_by_key(|track| {
                    artist_keys(track)
                        .iter()
                        .map(|artist| used_artists.get(artist).copied().unwrap_or(0))
                        .max()
                        .unwrap_or(0)
                })
        });
        let Some(track) = choice else {
            break;
        };
        used_ids.insert(track.id.clone());
        recordings.extend(recording_keys(track));
        for artist in artist_keys(track) {
            *used_artists.entry(artist).or_default() += 1;
        }
        selected.push(track.clone());
    }
    selected
}

pub(super) fn smart_seed<'a>(app: &'a App, attempted: &HashSet<String>) -> Option<&'a Track> {
    // Smart Shuffle describes the whole queued context, so keep seed selection
    // stable when playback advances. Walking forward from the cursor let an old
    // recommendation that had become a normal queue item become the next seed,
    // recursively pulling the mix into that unrelated artist cluster.
    let attempted_tracks: Vec<_> = attempted
        .iter()
        .filter_map(|id| app.cache.get(id))
        .collect();
    let candidates = || {
        app.queue
            .order
            .iter()
            .filter(|i| !app.queue.suggestions.contains(i))
            .filter_map(|&i| app.cache.get(&app.queue.ids[i]))
            .filter(|track| !app.radio_suggestions.contains(&track.id))
            .filter(|track| {
                track.playable && !track.artists.is_empty() && !attempted.contains(&track.id)
            })
    };
    candidates()
        .find(|track| {
            attempted_tracks
                .iter()
                .all(|previous| !shares_artist(track, previous))
        })
        .or_else(|| candidates().next())
}

#[derive(Default)]
pub(super) struct SmartTask {
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub request: u64,
    attempted: HashSet<String>,
    stopped: bool,
    checked: Option<(u64, Option<usize>, u64)>,
}

impl Tasks {
    pub(super) fn cancel_smart_shuffle(&mut self) {
        if let Some(handle) = self.smart.handle.take() {
            handle.abort();
        }
        self.smart.request = self.smart.request.wrapping_add(1);
        self.smart.attempted.clear();
        self.smart.stopped = false;
        self.smart.checked = None;
    }

    pub(super) fn cycle_shuffle(&mut self, app: &mut App) {
        app.remember_queue();
        self.sync_queue_epoch(app.queue.epoch);
        self.cancel_smart_shuffle();
        if app.queue.smart_shuffle {
            app.queue.set_shuffle(false);
            app.config.shuffle = false;
            app.status = "Shuffle off. Smart suggestions removed; current track preserved.".into();
        } else if app.config.shuffle {
            if let Some(handle) = self.recommendations.take() {
                handle.abort();
            }
            self.radio_active = false;
            app.radio_epoch = None;
            app.radio_source = None;
            app.radio_error = None;
            diversify_smart_order(app);
            app.queue.smart_shuffle = true;
            app.queue.revision += 1;
            app.status = "Smart Shuffle on: balances upcoming artists and mixes a suggestion after every 3 queue tracks. ✦ marks suggestions.".into();
            self.refill_smart_shuffle(app);
        } else {
            app.queue.set_shuffle(true);
            app.config.shuffle = true;
            app.status = "Shuffle on. Press s for Smart Shuffle.".into();
        }
    }

    pub(super) fn refill_smart_shuffle(&mut self, app: &App) {
        if !app.queue.smart_shuffle
            || self.smart.handle.is_some()
            || self.smart.stopped
            || app.queue.ids.len() >= crate::queue::MAX_TRACKS
            || self.smart.attempted.len() >= SMART_MAX_SEED_REQUESTS
        {
            return;
        }
        // Seed from an original queued track, never from our own suggestions.
        // One request per seed, with a bounded number of different seeds per session.
        let stamp = (app.queue.revision, app.queue.cursor, app.cache.revision);
        if self.smart.checked == Some(stamp) {
            return;
        }
        self.smart.checked = Some(stamp);
        if app.queue.smart_slots().is_empty() {
            return;
        }
        let seed = smart_seed(app, &self.smart.attempted);
        let Some(seed) = seed.cloned() else {
            return;
        };
        self.smart.attempted.insert(seed.id.clone());
        let epoch = app.queue.epoch;
        let request = self.smart.request;
        if self.demo {
            let _ = self.tx.send(Background::SmartRecommendations(
                epoch,
                request,
                Ok(Recommendations {
                    tracks: crate::demo::recommendation_tracks(),
                    source: crate::catalog::RecommendationSource::ArtistSearch,
                }),
            ));
            return;
        }
        let catalog = self.catalog.clone();
        let excluded = app
            .queue
            .ids
            .iter()
            .filter_map(|id| app.cache.get(id))
            .cloned()
            .collect::<Vec<_>>();
        let tx = self.tx.clone();
        let context = app
            .queue
            .ids
            .iter()
            .enumerate()
            .filter(|(index, _)| !app.queue.suggestions.contains(index))
            .filter(|(_, id)| !app.radio_suggestions.contains(*id))
            .filter_map(|(_, id)| app.cache.get(id))
            .cloned()
            .collect::<Vec<_>>();
        self.smart.handle = Some(tokio::spawn(async move {
            let result = catalog
                .smart_recommendations(&seed, &excluded, &context)
                .await;
            let _ = tx.send(Background::SmartRecommendations(epoch, request, result));
        }));
    }

    pub(super) fn finish_smart_shuffle(&mut self, app: &mut App, result: Result<Recommendations>) {
        self.smart.handle = None;
        self.smart.checked = None;
        match result {
            Ok(batch) => {
                let selected = diverse_candidates(app, &batch.tracks);
                let ids = selected.iter().map(|track| track.id.clone()).collect();
                let added: HashSet<_> = app.queue.add_smart_suggestions(ids).into_iter().collect();
                for track in selected {
                    if added.contains(&track.id) {
                        app.cache.insert(track.id.clone(), track);
                    }
                }
                app.status = format!(
                    "Smart Shuffle: {} suggestions added ({}){}",
                    added.len(),
                    batch.source.label(),
                    if added.is_empty() {
                        ". No new suggestions in this batch."
                    } else {
                        ". ✦ marks suggestions."
                    }
                );
            }
            Err(error) => {
                self.smart.stopped = true;
                app.status = format!(
                    "Smart Shuffle recommendations unavailable: {error:#}. Your queue still plays; cycle s to retry."
                );
            }
        }
    }
}
