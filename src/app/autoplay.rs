use super::*;

const PREFETCH_REMAINING: usize = 3;
const MAX_EMPTY_BATCHES: usize = 3;

#[derive(Default)]
pub(super) struct AutoplayTask {
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub pending: bool,
    seed: Option<Track>,
    attempted: HashSet<String>,
    pub round: usize,
    stopped: bool,
}

impl AutoplayTask {
    pub fn reset(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
        *self = Self::default();
    }
}

impl App {
    pub(super) fn finish_queue(&mut self, tx: &mpsc::UnboundedSender<Command>) {
        self.stop(tx);
        self.queue.position_ms = 0;
        if self.config.repeat == crate::model::Repeat::Off && self.queue.current().is_some() {
            self.autoplay_waiting = Some((self.queue.epoch, self.generation));
            self.status = "Autoplay: finding more songs... Space replays the current song.".into();
        } else {
            self.status = "Queue finished. Space replays the current track.".into();
        }
    }
}

impl Tasks {
    /// Prefetch at the tail without replacing or shuffling any chosen songs.
    /// Keep the original musical anchor throughout refills to avoid drifting
    /// through recommendations of recommendations.
    pub(super) fn update_autoplay(&mut self, app: &mut App, tx: &mpsc::UnboundedSender<Command>) {
        if app
            .autoplay_epoch
            .is_some_and(|epoch| epoch != app.queue.epoch)
        {
            app.autoplay_epoch = None;
            app.autoplay_error = None;
        }
        let waiting = app.autoplay_waiting == Some((app.queue.epoch, app.generation));
        if !waiting {
            app.autoplay_waiting = None;
        }
        if app.config.repeat != crate::model::Repeat::Off {
            app.autoplay_waiting = None;
            return;
        }
        if waiting && app.queue.advance(crate::model::Repeat::Off, true) {
            app.load(tx);
            return;
        }
        if !waiting && !matches!(app.state, State::Playing | State::Loading) {
            return;
        }
        let Some(cursor) = app.queue.cursor else {
            app.autoplay_waiting = None;
            return;
        };
        if app.queue.order.len().saturating_sub(cursor + 1) > PREFETCH_REMAINING {
            return;
        }
        // Existing Radio and playlist loaders own their pending songs. A slow
        // Radio response still resumes playback through the waiting path above.
        if self.playlist.is_some()
            || self.recommendations.is_some()
            || self.autoplay.pending
            || self.smart.handle.is_some()
        {
            return;
        }
        if app.radio_epoch == Some(app.queue.epoch)
            || self.autoplay.stopped
            || app.queue.ids.len() >= crate::queue::MAX_TRACKS
        {
            if waiting {
                self.end_autoplay_wait(app);
            }
            return;
        }
        let current_id = app.queue.current().unwrap().to_owned();
        if self.autoplay.attempted.contains(&current_id) {
            if waiting {
                self.end_autoplay_wait(app);
            }
            return;
        }
        let seed = self.autoplay.seed.clone().or_else(|| {
            app.queue.order[..=cursor]
                .iter()
                .rev()
                .filter(|index| !app.queue.suggestions.contains(index))
                .map(|&index| &app.queue.ids[index])
                .filter(|id| !app.radio_suggestions.contains(*id))
                .filter_map(|id| app.cache.get(id))
                .find(|track| track.playable && !track.name.is_empty() && !track.artists.is_empty())
                .cloned()
        });
        let Some(seed) = seed else {
            if waiting {
                self.end_autoplay_wait(app);
            }
            return;
        };
        self.autoplay.attempted.insert(current_id);
        self.autoplay.seed = Some(seed.clone());
        self.autoplay.pending = true;
        app.autoplay_epoch = Some(app.queue.epoch);
        app.autoplay_error = None;
        let epoch = app.queue.epoch;
        let round = self.autoplay.round;
        self.autoplay.round += 1;
        if self.demo {
            let _ = self.tx.send(Background::Autoplay(
                epoch,
                self.autoplay.round,
                Ok(Recommendations {
                    tracks: crate::demo::recommendation_tracks(),
                    source: crate::catalog::RecommendationSource::ArtistSearch,
                }),
            ));
            return;
        }
        let excluded = app
            .queue
            .ids
            .iter()
            .filter_map(|id| app.cache.get(id))
            .cloned()
            .collect::<Vec<_>>();
        let catalog = self.catalog.clone();
        let sender = self.tx.clone();
        self.autoplay.handle = Some(tokio::spawn(async move {
            // A restored queue can already contain the whole first discovery
            // page. Try later pages before declaring the station exhausted.
            for offset in 0..MAX_EMPTY_BATCHES {
                let next_round = round + offset + 1;
                let result = catalog
                    .radio_recommendations(&seed, &excluded, round + offset)
                    .await;
                if result.as_ref().is_ok_and(|batch| batch.tracks.is_empty())
                    && offset + 1 < MAX_EMPTY_BATCHES
                {
                    continue;
                }
                let _ = sender.send(Background::Autoplay(epoch, next_round, result));
                break;
            }
        }));
    }

    fn end_autoplay_wait(&self, app: &mut App) {
        app.autoplay_waiting = None;
        if let Some(error) = app.autoplay_error.as_ref().or(app.radio_error.as_ref()) {
            app.status =
                format!("Autoplay unavailable: {error}. Press R to retry with Track Radio.");
        } else {
            app.status =
                "No more recommendations. Space replays the current song; R starts Radio.".into();
        }
    }

    pub(super) fn finish_autoplay(&mut self, app: &mut App, result: Result<Recommendations>) {
        self.autoplay.handle = None;
        self.autoplay.pending = false;
        match result {
            Ok(batch) => {
                let mut seen: HashSet<_> = app.queue.ids.iter().cloned().collect();
                let mut recordings: HashSet<_> = app
                    .queue
                    .ids
                    .iter()
                    .filter_map(|id| app.cache.get(id))
                    .map(crate::catalog::recording_key)
                    .collect();
                let mut added = 0;
                for track in batch.tracks {
                    if track.playable
                        && seen.insert(track.id.clone())
                        && recordings.insert(crate::catalog::recording_key(&track))
                        && app.queue.enqueue(track.id.clone())
                    {
                        app.radio_suggestions.insert(track.id.clone());
                        app.cache.insert(track.id.clone(), track);
                        added += 1;
                    }
                }
                self.autoplay.stopped = added == 0;
                app.stats.refresh_metadata(&app.cache);
                app.status = format!("Autoplay: added {added} songs ({})", batch.source.label());
            }
            Err(error) => {
                self.autoplay.stopped = true;
                app.ui
                    .diagnostics
                    .history
                    .record(Subsystem::Recommendations, &error);
                app.autoplay_error = Some(format!("{error:#}"));
                app.status = format!(
                    "Autoplay unavailable: {error:#}. Your queued songs still play; R retries with Radio."
                );
            }
        }
    }
}
