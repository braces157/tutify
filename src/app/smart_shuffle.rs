use super::*;

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
            app.queue.smart_shuffle = true;
            app.queue.revision += 1;
            app.status = "Smart Shuffle on: mixes a suggestion after every 3 queue tracks. ✦ marks suggestions.".into();
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
            || self.smart.attempted.len() >= 3
        {
            return;
        }
        // Seed from an original queued track, never from our own suggestions.
        // One request per seed, with at most three different seeds per session.
        let stamp = (app.queue.revision, app.queue.cursor, app.cache.revision);
        if self.smart.checked == Some(stamp) {
            return;
        }
        self.smart.checked = Some(stamp);
        if app.queue.smart_slots().is_empty() {
            return;
        }
        let seed = app
            .queue
            .order
            .iter()
            .skip(app.queue.cursor.unwrap_or(0))
            .filter(|i| !app.queue.suggestions.contains(i))
            .filter_map(|&i| app.cache.get(&app.queue.ids[i]))
            .find(|track| {
                track.playable
                    && !track.artists.is_empty()
                    && !self.smart.attempted.contains(&track.id)
            });
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
        let tx = self.tx.clone();
        self.smart.handle = Some(tokio::spawn(async move {
            let result = catalog.recommendations(&seed).await;
            let _ = tx.send(Background::SmartRecommendations(epoch, request, result));
        }));
    }

    pub(super) fn finish_smart_shuffle(&mut self, app: &mut App, result: Result<Recommendations>) {
        self.smart.handle = None;
        self.smart.checked = None;
        match result {
            Ok(batch) => {
                let ids = batch
                    .tracks
                    .iter()
                    .filter(|t| t.playable)
                    .map(|t| t.id.clone())
                    .collect();
                let added: HashSet<_> = app.queue.add_smart_suggestions(ids).into_iter().collect();
                for track in batch.tracks {
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
