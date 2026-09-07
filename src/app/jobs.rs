use super::*;

pub(super) enum Background {
    LibraryProgress(u64, crate::library::LibraryProgress),
    LibraryDone(u64, Result<()>),
    Page(u64, Result<Page>),
    Metadata(u64, String, Result<Track>),
    MetadataDone(u64),
    SaveError(String),
    Lyrics(u64, Result<Option<crate::lyrics::Lyrics>>),
    PlaylistPage(u64, u64, Vec<Track>, bool),
    PlaylistError(u64, u64, String),
    Recommendations(u64, Result<Recommendations>),
}
pub(super) struct Tasks {
    pub(super) catalog: Catalog,
    pub(super) http: reqwest::Client,
    pub(super) tx: mpsc::UnboundedSender<Background>,
    pub(super) browse: Option<tokio::task::JoinHandle<()>>,
    pub(super) metadata: Option<tokio::task::JoinHandle<()>>,
    pub(super) lyrics: Option<tokio::task::JoinHandle<()>>,
    pub(super) recommendations: Option<tokio::task::JoinHandle<()>>,
    pub(super) playlist: Option<tokio::task::JoinHandle<()>>,
    pub(super) requested: HashSet<String>,
    pub(super) metadata_request: u64,
    pub(super) metadata_blocked: bool,
    pub(super) job_epoch: u64,
    pub(super) playlist_request: u64,
    pub(super) playlist_added: usize,
    pub(super) radio_active: bool,
    pub(super) radio_attempted: HashSet<String>,
}
impl Drop for Tasks {
    fn drop(&mut self) {
        for task in [
            &self.browse,
            &self.metadata,
            &self.lyrics,
            &self.recommendations,
            &self.playlist,
        ]
        .into_iter()
        .flatten()
        {
            task.abort();
        }
    }
}
impl Tasks {
    pub(super) fn new(catalog: Catalog, tx: mpsc::UnboundedSender<Background>) -> Result<Self> {
        Ok(Self {
            catalog,
            http: crate::auth::http_client()?,
            tx,
            browse: None,
            metadata: None,
            lyrics: None,
            recommendations: None,
            playlist: None,
            requested: HashSet::new(),
            metadata_request: 0,
            metadata_blocked: false,
            job_epoch: 0,
            playlist_request: 0,
            playlist_added: 0,
            radio_active: false,
            radio_attempted: HashSet::new(),
        })
    }
    pub(super) fn sync_queue_epoch(&mut self, epoch: u64) {
        if self.job_epoch != epoch {
            if let Some(t) = self.metadata.take() {
                t.abort();
            }
            self.metadata_request += 1;
            self.requested.clear();
            if let Some(t) = self.recommendations.take() {
                t.abort();
            }
            if let Some(t) = self.playlist.take() {
                t.abort();
            }
            self.radio_active = false;
            self.radio_attempted.clear();
            self.job_epoch = epoch;
        }
    }
    pub(super) fn fetch_recommendations(&mut self, track: &Track, epoch: u64) {
        self.sync_queue_epoch(epoch);
        if let Some(t) = self.recommendations.take() {
            t.abort();
        }
        self.radio_attempted.insert(track.id.clone());
        let track = track.clone();
        let catalog = self.catalog.clone();
        let tx = self.tx.clone();
        self.recommendations = Some(tokio::spawn(async move {
            let res = catalog.recommendations(&track).await;
            let _ = tx.send(Background::Recommendations(epoch, res));
        }));
    }
    pub(super) fn start_radio(
        &mut self,
        app: &mut App,
        track: Track,
        tx: &mpsc::UnboundedSender<Command>,
    ) {
        app.remember_queue();
        app.queue.replace(vec![track.id.clone()], 0, false);
        app.cache.insert(track.id.clone(), track.clone());
        app.radio_epoch = Some(app.queue.epoch);
        app.radio_source = None;
        app.radio_suggestions.clear();
        app.load(tx);
        self.fetch_recommendations(&track, app.queue.epoch);
        self.radio_active = true;
        app.status = format!(
            "Track Radio: {} - fetching Spotify recommendations...",
            track.name
        );
    }

    pub(super) fn refill_radio(&mut self, app: &App) {
        if !self.radio_active
            || self.recommendations.is_some()
            || app.state != State::Playing
            || app.radio_epoch != Some(app.queue.epoch)
            || app.queue.ids.len() >= crate::queue::MAX_TRACKS
        {
            return;
        }
        let Some(cursor) = app.queue.cursor else {
            return;
        };
        if app.queue.order.len().saturating_sub(cursor + 1) > 3 {
            return;
        }
        if let Some(track) = app.current_track().filter(|t| {
            t.playable && !t.artists.is_empty() && !self.radio_attempted.contains(&t.id)
        }) {
            self.fetch_recommendations(&track, app.queue.epoch);
        }
    }

    pub(super) fn update_lyrics(&mut self, app: &mut App) {
        let track = app
            .queue
            .current()
            .and_then(|id| app.cache.get(id))
            .cloned();
        if app.queue.current() != app.lyrics.track_id.as_deref() || track != app.lyrics.metadata {
            if let Some(t) = self.lyrics.take() {
                t.abort();
            }
            app.lyrics.request += 1;
            app.lyrics.track_id = app.queue.current().map(str::to_owned);
            app.lyrics.metadata = track.clone();
            app.lyrics.content = None;
            app.lyrics.error = None;
            app.lyrics.loading = false;
            app.lyrics.scroll = 0;
        }
        if app.ui.overlay != Overlay::Lyrics
            || app.lyrics.loading
            || app.lyrics.content.is_some()
            || app.lyrics.error.is_some()
        {
            return;
        }
        let Some(track) =
            track.filter(|t| !t.name.is_empty() && !t.artists.is_empty() && t.duration_ms > 0)
        else {
            return;
        };
        app.lyrics.loading = true;
        let request = app.lyrics.request;
        let client = self.http.clone();
        let tx = self.tx.clone();
        self.lyrics = Some(tokio::spawn(async move {
            let lyr =
                crate::lyrics::fetch(&client, &track.name, &track.artists, track.duration_ms).await;
            let _ = tx.send(Background::Lyrics(request, lyr));
        }));
    }
    pub(super) fn enqueue_playlist(&mut self, app: &mut App, playlist_id: String, name: String) {
        self.sync_queue_epoch(app.queue.epoch);
        if self.playlist.is_some() {
            app.status =
                "A playlist is already being added. Clear/replace the queue to cancel.".into();
            return;
        }
        if app.queue.ids.len() >= crate::queue::MAX_TRACKS {
            app.status = "Queue limit reached (100,000 tracks).".into();
            return;
        }
        app.remember_queue();
        self.playlist_request += 1;
        self.playlist_added = 0;
        let request = self.playlist_request;
        let epoch = app.queue.epoch;
        let capacity = crate::queue::MAX_TRACKS.saturating_sub(app.queue.ids.len());
        let catalog = self.catalog.clone();
        let tx = self.tx.clone();
        app.status = format!("Adding playlist '{name}'...");
        self.playlist = Some(tokio::spawn(async move {
            let mut offset = 0;
            let mut received = 0;
            loop {
                match catalog
                    .page(&Browse::Playlist(playlist_id.clone()), offset)
                    .await
                {
                    Ok(page) => {
                        let Rows::Tracks(mut tracks) = page.rows else {
                            break;
                        };
                        tracks.retain(|t| t.playable);
                        tracks.truncate(capacity.saturating_sub(received));
                        received += tracks.len();
                        let next = page.next.filter(|n| {
                            *n > offset && *n < crate::queue::MAX_TRACKS && received < capacity
                        });
                        if tx
                            .send(Background::PlaylistPage(
                                epoch,
                                request,
                                tracks,
                                next.is_none(),
                            ))
                            .is_err()
                        {
                            break;
                        }
                        let Some(next) = next else {
                            break;
                        };
                        offset = next;
                    }
                    Err(e) => {
                        let _ =
                            tx.send(Background::PlaylistError(epoch, request, format!("{e:#}")));
                        break;
                    }
                }
            }
        }));
    }
    pub(super) fn request(&mut self, app: &mut App, offset: usize) {
        if let Some(task) = self.browse.take() {
            task.abort();
        }
        app.catalog.request += 1;
        app.catalog.busy = true;
        app.status = "Fetching from Spotify... playback controls remain available.".into();
        let request = app.catalog.request;
        let browse = app.catalog.browse.clone();
        let catalog = self.catalog.clone();
        let tx = self.tx.clone();
        if app.catalog.view == View::Search && app.catalog.query.trim().is_empty() {
            app.catalog.busy = false;
            app.catalog.next = None;
            app.status =
                "Enter a search. F2: Spotify catalog | F3: all saved Liked Songs and playlists."
                    .into();
            return;
        }
        if app.catalog.view == View::Search && app.catalog.search_scope == SearchScope::Library {
            app.reset_rows();
            app.catalog.selected = 0;
            app.catalog.library_scanned = 0;
            app.catalog.next = None;
            app.catalog.title = "Saved library — scanning".into();
            app.status = "Scanning saved Liked Songs and playlists. Partial matches appear as pages arrive; Esc cancels.".into();
            let query = app.catalog.query.clone();
            self.browse = Some(tokio::spawn(async move {
                let (progress_tx, mut progress_rx) = mpsc::channel(2);
                let scan = crate::library::search(catalog, query, progress_tx);
                tokio::pin!(scan);
                let mut open = true;
                loop {
                    tokio::select! {
                        result = &mut scan => {
                            while let Ok(progress) = progress_rx.try_recv() { let _ = tx.send(Background::LibraryProgress(request, progress)); }
                            let _ = tx.send(Background::LibraryDone(request, result));
                            break;
                        }
                        progress = progress_rx.recv(), if open => {
                            match progress {
                                Some(progress) => { if tx.send(Background::LibraryProgress(request, progress)).is_err() { break; } }
                                None => open = false,
                            }
                        }
                    }
                }
            }));
            return;
        }
        self.browse = Some(tokio::spawn(async move {
            let result = catalog.page(&browse, offset).await;
            let _ = tx.send(Background::Page(request, result));
        }));
    }
    pub(super) fn metadata(&mut self, app: &App) {
        if self.metadata_blocked || self.metadata.is_some() {
            return;
        }
        let mut ids = Vec::new();
        if let Some(id) = app.queue.current() {
            ids.push(id.to_owned());
        }
        let start = if app.catalog.view == View::Queue {
            app.ui.render.borrow().queue_scroll
        } else {
            app.queue.cursor.unwrap_or(0)
        };
        ids.extend(
            app.queue
                .order
                .iter()
                .skip(start)
                .take(app.ui.render.borrow().queue_height.clamp(10, 200) + 8)
                .map(|i| app.queue.ids[*i].clone()),
        );
        ids.retain(|id| !app.cache.contains_key(id) && self.requested.insert(id.clone()));
        if ids.is_empty() {
            return;
        }
        let catalog = self.catalog.clone();
        let tx = self.tx.clone();
        let request = self.metadata_request;
        self.metadata = Some(tokio::spawn(async move {
            let mut stream = catalog.tracks(ids);
            while let Some((id, result)) = stream.next().await {
                let failed = result
                    .as_ref()
                    .is_err_and(|e| !e.is::<crate::catalog::MissingItem>());
                if tx.send(Background::Metadata(request, id, result)).is_err() {
                    return;
                }
                if failed {
                    break;
                }
            }
            let _ = tx.send(Background::MetadataDone(request));
        }));
    }
    pub(super) fn retry_metadata(&mut self, app: &mut App) {
        self.metadata_request += 1;
        if let Some(t) = self.metadata.take() {
            t.abort();
        }
        self.requested.clear();
        self.metadata_blocked = false;
        app.metadata_error = None;
        let mut ids = Vec::new();
        if let Some(id) = app.queue.current() {
            ids.push(id.to_owned());
        }
        ids.extend(
            app.queue
                .order
                .iter()
                .skip(app.ui.render.borrow().queue_scroll)
                .take(app.ui.render.borrow().queue_height.clamp(10, 200) + 8)
                .map(|i| app.queue.ids[*i].clone()),
        );
        for id in ids {
            app.cache.remove(&id);
        }
        app.lyrics.request += 1;
        app.lyrics.track_id = None;
        app.lyrics.error = None;
        app.lyrics.content = None;
        app.lyrics.loading = false;
        if let Some(t) = self.lyrics.take() {
            t.abort();
        }
    }
    pub(super) fn view(&mut self, app: &mut App, view: View) {
        app.catalog.view = view;
        app.catalog.nav = view.index();
        app.catalog.sidebar = false;
        app.ui.overlay = Overlay::None;
        app.catalog.selected = 0;
        app.catalog.editing = false;
        app.catalog.filter.clear();
        app.catalog.filtering = false;
        app.catalog.request += 1;
        app.catalog.busy = false;
        if let Some(task) = self.browse.take() {
            task.abort();
        }
        match view {
            View::Search => {
                app.catalog.browse = Browse::Search(app.catalog.query.clone());
                app.catalog.title = app.catalog.search_scope.label().into();
            }
            View::Playlists => {
                app.catalog.browse = Browse::Playlists;
                app.catalog.title = "Playlists".into();
            }
            View::Liked => {
                app.catalog.browse = Browse::Liked;
                app.catalog.title = "Liked Songs".into();
            }
            _ => return,
        }
        app.reset_rows();
        app.catalog.next = None;
        self.request(app, 0);
    }
}

pub(super) fn background(app: &mut App, tasks: &mut Tasks, event: Background) -> bool {
    match event {
        Background::LibraryProgress(id, progress) if id == app.catalog.request => {
            app.catalog.library_scanned = progress.scanned;
            app.catalog.append_tracks(progress.tracks);
            app.catalog.title = if progress.complete {
                "Saved library — complete"
            } else {
                "Saved library — scanning"
            }
            .into();
            app.status = format!(
                "{} matches; {} saved tracks scanned. {}",
                app.raw_len(),
                app.catalog.library_scanned,
                if progress.complete {
                    "Search complete."
                } else {
                    "Partial results; Esc cancels."
                }
            );
        }
        Background::LibraryDone(id, result) if id == app.catalog.request => {
            app.catalog.busy = false;
            tasks.browse = None;
            match result {
                Ok(()) => {
                    app.catalog.title = "Saved library — complete".into();
                    app.status = format!(
                        "{} matches across {} saved tracks. F2 searches Spotify; / edits the query.",
                        app.raw_len(),
                        app.catalog.library_scanned
                    );
                }
                Err(error) => {
                    app.catalog.title = "Saved library — partial results".into();
                    app.status = format!(
                        "Library search stopped: {error:#}. {} partial matches retained; F5 restarts.",
                        app.raw_len()
                    );
                }
            }
        }
        Background::Page(id, result) if id == app.catalog.request => {
            app.catalog.busy = false;
            match result {
                Ok(page) => {
                    if let Rows::Tracks(tracks) = &page.rows {
                        for track in tracks {
                            app.cache.insert(track.id.clone(), track.clone());
                        }
                    }
                    if page.offset == 0 {
                        app.ui.render.borrow_mut().catalog_scroll = 0;
                    }
                    app.catalog.apply_page(page);
                    app.status = format!(
                        "{} loaded | Enter play/open | a enqueue{}",
                        app.len(),
                        if app.catalog.next.is_some() {
                            " | PgDn loads more"
                        } else {
                            ""
                        }
                    );
                }
                Err(e) => app.status = format!("{e:#}"),
            }
        }
        Background::Metadata(request, id, result) if request == tasks.metadata_request => {
            tasks.requested.remove(&id);
            match result {
                Ok(track) => {
                    app.cache.insert(track.id.clone(), track);
                    app.stats.refresh_metadata(&app.cache);
                }
                Err(e) if e.is::<crate::catalog::MissingItem>() => {
                    app.cache.insert(
                        id.clone(),
                        Track {
                            id,
                            name: "Track unavailable (F5 rechecks)".into(),
                            playable: false,
                            ..Track::default()
                        },
                    );
                }
                Err(e) => {
                    tasks.metadata_blocked = true;
                    let message = format!("Queue metadata: {e:#}. Press F5 to retry.");
                    app.metadata_error = Some(message.clone());
                    app.status = message;
                }
            }
        }
        Background::MetadataDone(request) if request == tasks.metadata_request => {
            tasks.metadata = None;
            tasks.requested.clear();
        }
        Background::Lyrics(request, result) if request == app.lyrics.request => {
            app.lyrics.loading = false;
            match result {
                Ok(Some(lyrics)) => app.lyrics.content = Some(lyrics),
                Ok(None) => {
                    app.lyrics.error = Some("No lyrics found for this track. F5 retries.".into())
                }
                Err(e) => {
                    app.lyrics.error = Some(format!("Lyrics request failed: {e:#}. F5 retries."))
                }
            }
        }
        Background::PlaylistPage(epoch, request, tracks, done)
            if epoch == app.queue.epoch && request == tasks.playlist_request =>
        {
            if done {
                tasks.playlist = None;
            }
            for track in tracks {
                if track.playable && app.enqueue_manual(track.id.clone()) {
                    tasks.playlist_added += 1;
                    app.cache.insert(track.id.clone(), track);
                }
            }
            app.stats.refresh_metadata(&app.cache);
            app.status = format!(
                "Added {} playlist tracks{}",
                tasks.playlist_added,
                if app.queue.ids.len() == crate::queue::MAX_TRACKS {
                    "; queue limit reached."
                } else if done {
                    "."
                } else {
                    "; loading next page..."
                }
            );
        }
        Background::PlaylistError(epoch, request, error)
            if epoch == app.queue.epoch && request == tasks.playlist_request =>
        {
            tasks.playlist = None;
            app.status = format!(
                "Playlist stopped after {} additions: {error}. Added tracks remain queued.",
                tasks.playlist_added
            );
        }
        Background::Recommendations(epoch, result) if epoch == app.queue.epoch => {
            tasks.recommendations = None;
            match result {
                Ok(batch) => {
                    app.radio_source = Some(batch.source);
                    let mut seen: HashSet<String> = app.queue.ids.iter().cloned().collect();
                    let mut added = 0;
                    for track in batch.tracks {
                        if track.playable
                            && seen.insert(track.id.clone())
                            && app.queue.enqueue(track.id.clone())
                        {
                            added += 1;
                            app.radio_suggestions.insert(track.id.clone());
                            app.cache.insert(track.id.clone(), track);
                        }
                    }
                    app.stats.refresh_metadata(&app.cache);
                    if added == 0 {
                        tasks.radio_active = false;
                    }
                    app.status = format!(
                        "{}: added {added} tracks.{}",
                        batch.source.label(),
                        if added == 0 {
                            " No new suggestions; automatic refill stopped."
                        } else {
                            ""
                        }
                    );
                }
                Err(e) => {
                    tasks.radio_active = false;
                    app.status =
                        format!("Track Radio failed: {e:#}. Press R to start Radio again.");
                }
            }
        }
        Background::SaveError(error) => {
            app.status = error;
            return true;
        }
        _ => (),
    }
    false
}
