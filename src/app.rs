use crate::{
    auth::TokenManager,
    catalog::{Browse, Catalog, Page, Rows},
    media_controls::{self, Action as MediaAction},
    model::Track,
    playback::{self, Command, Event},
    queue::Queue,
    stats::{PlaybackAccounting, SongStats},
    storage::{Config, Storage},
    ui,
};
use anyhow::Result;
use crossterm::event::{
    Event as Input, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use futures_util::StreamExt;
use std::{
    cell::{Cell, RefCell},
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Search,
    Playlists,
    Liked,
    Queue,
    Help,
}
impl View {
    pub const ALL: [View; 5] = [
        Self::Search,
        Self::Playlists,
        Self::Liked,
        Self::Queue,
        Self::Help,
    ];
    #[allow(dead_code)]
    pub fn name(self) -> &'static str {
        match self {
            Self::Search => "Search",
            Self::Playlists => "Playlists",
            Self::Liked => "Liked Songs",
            Self::Queue => "Queue",
            Self::Help => "Help",
        }
    }
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|v| *v == self).unwrap()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchScope {
    Spotify,
    Library,
}
impl SearchScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify search",
            Self::Library => "Saved library search",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Paused,
    Loading,
    Playing,
    Failed,
}
#[derive(Default)]
struct FilterCache {
    revision: u64,
    len: usize,
    query: String,
    view: Option<View>,
    indices: Arc<Vec<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseTarget {
    Navigation(View),
    SearchMode(SearchScope),
    Catalog(usize),
    Queue(usize),
    Prompt,
    CatalogScroll,
    QueueScroll,
    PlayPause,
    Seek,
    Menu(usize),
}

pub struct ContextMenu {
    pub x: u16,
    pub y: u16,
    pub selected: usize,
    pub actions: Vec<(&'static str, KeyCode)>,
    view: View,
    row: usize,
    revision: u64,
    filter: String,
}

struct QueueUndo {
    queue: Queue,
    shuffle: bool,
}

pub struct App {
    undo: VecDeque<QueueUndo>,

    pub mouse_hits: RefCell<Vec<(ratatui::layout::Rect, MouseTarget)>>,
    pub context_menu: Option<ContextMenu>,
    pub config: Config,
    pub queue: Queue,
    pub view: View,
    pub sidebar: bool,
    pub nav: usize,
    pub rows: Rows,
    pub selected: usize,
    pub query: String,
    pub search_scope: SearchScope,
    pub library_scanned: usize,
    pub editing: bool,
    pub status: String,
    pub busy: bool,
    pub state: State,
    pub cache: crate::cache::MetadataCache,
    pub title: String,
    pub next: Option<usize>,
    pub browse: Browse,
    pub generation: u64,
    pub loaded: bool,
    /// Volume saved before the last mute action. This is session-only so a
    /// restart still uses the user's persisted volume setting.
    muted_volume: Option<u8>,
    request: u64,
    pub filter: String,
    pub filtering: bool,
    pub quit: bool,
    rows_revision: u64,
    filtered: RefCell<FilterCache>,
    pub catalog_scroll: Cell<usize>,
    pub stats_scroll: Cell<usize>,
    pub queue_scroll: Cell<usize>,
    pub queue_height: Cell<usize>,
    pub terminal_size: Cell<(u16, u16)>,
    pub help_length: Cell<usize>,
    pub lyrics_scroll: usize,
    pub lyrics_length: Cell<usize>,
    pub lyrics_error: Option<String>,
    lyrics_metadata: Option<Track>,
    lyrics_request: u64,
    pub metadata_error: Option<String>,
    position_anchor: Option<(Instant, u32)>,
    pub show_lyrics: bool,
    pub show_visualizer: bool,
    pub lyrics: Option<crate::lyrics::Lyrics>,
    pub lyrics_loading: bool,
    pub lyrics_track_id: Option<String>,
    pub animation_frame: u32,
    pub visualizer: Arc<crate::visualizer::AudioVisualizer>,
    pub stats: SongStats,
    pub accounting: PlaybackAccounting,
    pub show_stats: bool,
}

impl App {
    pub fn new(config: Config, queue: Queue) -> Self {
        let restored = !queue.ids.is_empty();
        Self {
            undo: VecDeque::new(),
            mouse_hits: RefCell::new(Vec::new()),
            context_menu: None,
            config,
            queue,
            view: if restored { View::Queue } else { View::Search },
            sidebar: false,
            nav: if restored { 3 } else { 0 },
            rows: Rows::Tracks(vec![]),
            selected: 0,
            query: String::new(),
            search_scope: SearchScope::Spotify,
            library_scanned: 0,
            editing: false,
            filter: String::new(),
            filtering: false,
            status: "Paused. / search | 1-5 views | Tab navigation | ? help | q quit".into(),
            busy: false,
            state: State::Paused,
            cache: crate::cache::MetadataCache::default(),
            title: "Search".into(),
            next: None,
            browse: Browse::Search(String::new()),
            generation: 0,
            loaded: false,
            muted_volume: None,
            request: 0,
            quit: false,
            rows_revision: 0,
            filtered: RefCell::new(FilterCache::default()),
            catalog_scroll: Cell::new(0),
            stats_scroll: Cell::new(0),
            queue_scroll: Cell::new(0),
            queue_height: Cell::new(40),
            terminal_size: Cell::new((120, 35)),
            help_length: Cell::new(1),
            lyrics_scroll: 0,
            lyrics_length: Cell::new(1),
            lyrics_error: None,
            lyrics_metadata: None,
            lyrics_request: 0,
            metadata_error: None,
            position_anchor: None,
            show_lyrics: false,
            show_visualizer: false,
            lyrics: None,
            lyrics_loading: false,
            lyrics_track_id: None,
            animation_frame: 0,
            visualizer: crate::visualizer::AudioVisualizer::new(),
            stats: SongStats::default(),
            accounting: PlaybackAccounting::new(),
            show_stats: false,
        }
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    fn remember_queue(&mut self) {
        if self.state == State::Playing {
            self.account_playback_time(Instant::now());
        }
        self.interpolate_position();
        // At most ten actions and 100,000 stored IDs across snapshots. Only
        // mutations take a snapshot; rendering and ordinary navigation do not.
        let size = self.queue.ids.len();
        let mut retained: usize = self.undo.iter().map(|entry| entry.queue.ids.len()).sum();
        while self.undo.len() >= 10 || retained + size > crate::queue::MAX_TRACKS {
            let Some(oldest) = self.undo.pop_front() else {
                break;
            };
            retained -= oldest.queue.ids.len();
        }
        self.undo.push_back(QueueUndo {
            queue: self.queue.clone(),
            shuffle: self.config.shuffle,
        });
    }
    fn undo_queue(&mut self, tx: &mpsc::UnboundedSender<Command>) {
        let Some(mut previous) = self.undo.pop_back() else {
            self.status = "No queue changes to undo in this session.".into();
            return;
        };
        self.stop(tx);
        previous.queue.revision = self.queue.revision.wrapping_add(1);
        previous.queue.epoch = self.queue.epoch.wrapping_add(1);
        self.queue = previous.queue;
        self.config.shuffle = previous.shuffle;
        self.view = View::Queue;
        self.nav = View::Queue.index();
        self.sidebar = false;
        self.editing = false;
        self.filtering = false;
        self.filter.clear();
        self.context_menu = None;
        self.show_lyrics = false;
        self.show_visualizer = false;
        self.status =
            "Queue restored, paused at its saved position. Space resumes; u undoes another change."
                .into();
    }
    pub fn is_filtered(&self) -> bool {
        (self.view == View::Liked || self.view == View::Playlists) && !self.filter.is_empty()
    }
    pub fn raw_len(&self) -> usize {
        match &self.rows {
            Rows::Tracks(t) => t.len(),
            Rows::Playlists(p) => p.len(),
        }
    }
    pub fn filtered_indices(&self) -> Arc<Vec<usize>> {
        let mut cached = self.filtered.borrow_mut();
        let query = if self.is_filtered() {
            self.filter.as_str()
        } else {
            ""
        };
        if cached.view == Some(self.view)
            && cached.revision == self.rows_revision
            && cached.len == self.raw_len()
            && cached.query == query
        {
            return cached.indices.clone();
        }
        let terms: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let indices = match &self.rows {
            Rows::Tracks(tracks) => tracks
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    if terms.is_empty() {
                        return true;
                    }
                    let name = t.name.to_lowercase();
                    let artists = t.artists.to_lowercase();
                    terms
                        .iter()
                        .all(|term| name.contains(term) || artists.contains(term))
                })
                .map(|(i, _)| i)
                .collect(),
            Rows::Playlists(playlists) => playlists
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    if terms.is_empty() {
                        return true;
                    }
                    let name = p.name.to_lowercase();
                    let owner = p.owner.to_lowercase();
                    terms
                        .iter()
                        .all(|term| name.contains(term) || owner.contains(term))
                })
                .map(|(i, _)| i)
                .collect(),
        };
        *cached = FilterCache {
            revision: self.rows_revision,
            len: self.raw_len(),
            query: query.to_owned(),
            view: Some(self.view),
            indices: Arc::new(indices),
        };
        cached.indices.clone()
    }
    fn reset_rows(&mut self) {
        self.rows = Rows::Tracks(vec![]);
        self.rows_revision += 1;
        self.catalog_scroll.set(0);
    }
    fn animation_interval(&self) -> Option<Duration> {
        let (width, height) = self.terminal_size.get();
        if self.state != State::Playing || width < 32 || height < 10 {
            return None;
        }
        Some(Duration::from_millis(
            if self.show_visualizer || width >= 50 {
                33
            } else {
                250
            },
        ))
    }
    fn interpolate_position(&mut self) {
        if self.state == State::Playing {
            if let Some((at, position)) = self.position_anchor {
                let duration = self.current_track().map_or(u32::MAX, |t| {
                    if t.duration_ms == 0 {
                        u32::MAX
                    } else {
                        t.duration_ms
                    }
                });
                self.queue.position_ms = position
                    .saturating_add(at.elapsed().as_millis().min(u32::MAX as u128) as u32)
                    .min(duration);
            }
        }
    }
    fn anchor_position(&mut self) {
        self.position_anchor = Some((Instant::now(), self.queue.position_ms));
    }
    pub fn account_playback_time(&mut self, now: Instant) {
        if self.state != State::Playing {
            self.accounting.last_accounted_at = None;
            return;
        }
        let Some(track_id) = self.accounting.track_id.clone() else {
            self.accounting.last_accounted_at = None;
            return;
        };
        if let Some(track) = self.cache.get(&track_id) {
            if track.duration_ms > 0 {
                self.accounting.duration_ms = track.duration_ms;
            }
        }
        let fallback_name = self
            .cache
            .get(&track_id)
            .map(|t| t.name.clone())
            .unwrap_or_else(|| format!("Track {track_id}"));
        let fallback_artists = self
            .cache
            .get(&track_id)
            .map(|t| t.artists.clone())
            .unwrap_or_default();

        self.accounting.account_time(
            now,
            &track_id,
            &fallback_name,
            &fallback_artists,
            &mut self.stats,
        );
    }
    pub fn finalize_playback_accounting(&mut self) {
        if self.state == State::Playing {
            self.account_playback_time(Instant::now());
        }
        self.accounting.last_accounted_at = None;
    }
    pub fn len(&self) -> usize {
        if self.show_stats {
            self.stats.len()
        } else if self.view == View::Help {
            self.help_length.get()
        } else if self.view == View::Queue {
            self.queue.order.len()
        } else if self.is_filtered() {
            self.filtered_indices().len()
        } else {
            self.raw_len()
        }
    }
    #[allow(dead_code)]
    pub fn selection(&self) -> usize {
        if self.view == View::Queue {
            self.queue.selected
        } else {
            self.selected
        }
    }
    pub fn current_track(&self) -> Option<Track> {
        self.queue.current().map(|id| {
            self.cache
                .get(id)
                .cloned()
                .unwrap_or_else(|| Track::unknown(id))
        })
    }
    fn discord_snapshot(&self) -> crate::discord::Snapshot {
        crate::discord::Snapshot {
            // Restoring a queue is not a listening event. Require real metadata
            // and a loaded stream before publishing a listening activity.
            track: self
                .queue
                .current()
                .filter(|_| self.loaded)
                .and_then(|id| self.cache.get(id))
                .cloned(),
            state: self.state,
            position_ms: self.queue.position_ms,
        }
    }
    pub fn window_title(&self) -> String {
        if let Some(track) = self.current_track() {
            let symbol = match self.state {
                State::Playing => "",
                State::Paused => "|| ",
                State::Loading => "... ",
                State::Failed => "! ",
            };
            if track.name.is_empty() {
                "Tuitify".to_string()
            } else if track.artists.is_empty() {
                format!("{symbol}Tuitify • {}", track.name)
            } else {
                format!("{symbol}Tuitify • {} - {}", track.name, track.artists)
            }
        } else {
            "Tuitify".to_string()
        }
    }
    fn selected_track(&self) -> Option<Track> {
        if self.view == View::Queue {
            self.queue
                .order
                .get(self.queue.selected)
                .map(|i| &self.queue.ids[*i])
                .map(|id| {
                    self.cache
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| Track::unknown(id))
                })
        } else if let Rows::Tracks(t) = &self.rows {
            let actual_idx = if self.is_filtered() {
                *self.filtered_indices().get(self.selected)?
            } else {
                self.selected
            };
            t.get(actual_idx).cloned()
        } else {
            None
        }
    }
    fn send(&mut self, tx: &mpsc::UnboundedSender<Command>, command: Command) {
        if tx.send(command).is_err() {
            self.state = State::Failed;
            self.status = "Playback worker stopped; restart Tuitify. Queue remains saved.".into();
        }
    }
    fn load(&mut self, tx: &mpsc::UnboundedSender<Command>) {
        if let Some(id) = self.queue.current().map(str::to_owned) {
            self.finalize_playback_accounting();
            self.generation += 1;
            let duration_ms = self.cache.get(&id).map_or(0, |t| t.duration_ms);
            self.accounting
                .start_generation(self.generation, Some(id.clone()), duration_ms);
            self.loaded = true;
            self.state = State::Loading;
            self.status = "Loading audio... Space pauses | q exits".into();
            self.send(
                tx,
                Command::Load {
                    id,
                    position_ms: self.queue.position_ms,
                    generation: self.generation,
                },
            );
        }
    }
    fn stop(&mut self, tx: &mpsc::UnboundedSender<Command>) {
        self.finalize_playback_accounting();
        self.generation += 1;
        self.state = State::Paused;
        self.loaded = false;
        self.position_anchor = None;
        self.send(tx, Command::Stop);
    }
    fn media_action(&mut self, action: MediaAction, tx: &mpsc::UnboundedSender<Command>) {
        let active = matches!(self.state, State::Playing | State::Loading);
        match action {
            MediaAction::Play if active => (),
            MediaAction::Pause if !active => (),
            MediaAction::Pause | MediaAction::Toggle if active => {
                self.interpolate_position();
                self.finalize_playback_accounting();
                self.state = State::Paused;
                self.send(tx, Command::Pause);
            }
            MediaAction::Play | MediaAction::Toggle => {
                if self.loaded {
                    self.state = State::Loading;
                    self.send(tx, Command::Resume);
                } else {
                    if self.queue.current().is_none() && !self.queue.ids.is_empty() {
                        self.queue.select(0);
                    }
                    self.load(tx);
                }
            }
            MediaAction::Next => {
                if self.queue.advance(self.config.repeat, false) {
                    self.load(tx);
                } else {
                    self.stop(tx);
                    self.status = "End of queue.".into();
                }
            }
            MediaAction::Previous => {
                self.interpolate_position();
                if self.queue.previous() {
                    self.load(tx);
                }
            }
            MediaAction::Pause => (),
        }
    }
    pub fn playback_event(&mut self, event: Event, tx: &mpsc::UnboundedSender<Command>) {
        match event {
            Event::Playing {
                generation,
                position_ms,
            } if generation == self.generation => {
                self.state = State::Playing;
                self.queue.position_ms = position_ms;
                self.anchor_position();
                let track_id = self.queue.current().map(str::to_owned);
                let duration_ms = track_id
                    .as_deref()
                    .and_then(|id| self.cache.get(id))
                    .map_or(0, |t| t.duration_ms);
                self.accounting
                    .on_playing(generation, track_id, duration_ms, Instant::now());
                self.status =
                    "Playing | Space pause | Left/Right seek | +/- volume | n/p next/previous"
                        .into();
            }
            Event::Paused {
                generation,
                position_ms,
            } if generation == self.generation => {
                self.finalize_playback_accounting();
                self.state = State::Paused;
                self.queue.position_ms = position_ms;
                self.anchor_position();
                self.status = "Paused | Space resumes | Left/Right seek | +/- volume".into();
            }
            Event::Position {
                generation,
                position_ms,
            } if generation == self.generation => {
                self.account_playback_time(Instant::now());
                self.queue.position_ms = position_ms;
                self.anchor_position();
            }
            Event::Completed(generation) if generation == self.generation => {
                self.finalize_playback_accounting();
                if let Some(id) = self.accounting.track_id.clone() {
                    let fallback_name = self
                        .cache
                        .get(&id)
                        .map(|t| t.name.clone())
                        .unwrap_or_else(|| format!("Track {id}"));
                    let fallback_artists = self
                        .cache
                        .get(&id)
                        .map(|t| t.artists.clone())
                        .unwrap_or_default();
                    self.accounting.on_completed(
                        generation,
                        &id,
                        &fallback_name,
                        &fallback_artists,
                        &mut self.stats,
                    );
                }
                if self.queue.advance(self.config.repeat, true) {
                    self.load(tx);
                } else {
                    self.stop(tx);
                    self.queue.position_ms = 0;
                    self.status = "Queue finished. Space replays the current track.".into();
                }
            }
            Event::Error(message) => {
                self.finalize_playback_accounting();
                self.stop(tx);
                self.state = State::Failed;
                self.status = message;
            }
            Event::TrackError {
                generation,
                message,
            } if generation == self.generation => self.playback_event(Event::Error(message), tx),
            Event::Volume(volume) => self.config.volume = volume,
            _ => (),
        }
    }
}

enum Background {
    LibraryProgress(u64, crate::library::LibraryProgress),
    LibraryDone(u64, Result<()>),
    Page(u64, Result<Page>),
    Metadata(u64, String, Result<Track>),
    MetadataDone(u64),
    SaveError(String),
    Lyrics(u64, Result<Option<crate::lyrics::Lyrics>>),
    PlaylistPage(u64, u64, Vec<Track>, bool),
    PlaylistError(u64, u64, String),
    Recommendations(u64, Result<Vec<Track>>),
}
struct Tasks {
    catalog: Catalog,
    http: reqwest::Client,
    tx: mpsc::UnboundedSender<Background>,
    browse: Option<tokio::task::JoinHandle<()>>,
    metadata: Option<tokio::task::JoinHandle<()>>,
    lyrics: Option<tokio::task::JoinHandle<()>>,
    recommendations: Option<tokio::task::JoinHandle<()>>,
    playlist: Option<tokio::task::JoinHandle<()>>,
    requested: HashSet<String>,
    metadata_request: u64,
    metadata_blocked: bool,
    job_epoch: u64,
    playlist_request: u64,
    playlist_added: usize,
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
    fn new(catalog: Catalog, tx: mpsc::UnboundedSender<Background>) -> Result<Self> {
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
        })
    }
    fn sync_queue_epoch(&mut self, epoch: u64) {
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
            self.job_epoch = epoch;
        }
    }
    fn fetch_recommendations(&mut self, track: &Track, epoch: u64) {
        self.sync_queue_epoch(epoch);
        if let Some(t) = self.recommendations.take() {
            t.abort();
        }
        let track = track.clone();
        let catalog = self.catalog.clone();
        let tx = self.tx.clone();
        self.recommendations = Some(tokio::spawn(async move {
            let res = catalog.recommendations(&track).await;
            let _ = tx.send(Background::Recommendations(epoch, res));
        }));
    }
    fn update_lyrics(&mut self, app: &mut App) {
        let track = app
            .queue
            .current()
            .and_then(|id| app.cache.get(id))
            .cloned();
        if app.queue.current() != app.lyrics_track_id.as_deref() || track != app.lyrics_metadata {
            if let Some(t) = self.lyrics.take() {
                t.abort();
            }
            app.lyrics_request += 1;
            app.lyrics_track_id = app.queue.current().map(str::to_owned);
            app.lyrics_metadata = track.clone();
            app.lyrics = None;
            app.lyrics_error = None;
            app.lyrics_loading = false;
            app.lyrics_scroll = 0;
        }
        if !app.show_lyrics
            || app.lyrics_loading
            || app.lyrics.is_some()
            || app.lyrics_error.is_some()
        {
            return;
        }
        let Some(track) =
            track.filter(|t| !t.name.is_empty() && !t.artists.is_empty() && t.duration_ms > 0)
        else {
            return;
        };
        app.lyrics_loading = true;
        let request = app.lyrics_request;
        let client = self.http.clone();
        let tx = self.tx.clone();
        self.lyrics = Some(tokio::spawn(async move {
            let lyr =
                crate::lyrics::fetch(&client, &track.name, &track.artists, track.duration_ms).await;
            let _ = tx.send(Background::Lyrics(request, lyr));
        }));
    }
    fn enqueue_playlist(&mut self, app: &mut App, playlist_id: String, name: String) {
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
    fn request(&mut self, app: &mut App, offset: usize) {
        if let Some(task) = self.browse.take() {
            task.abort();
        }
        app.request += 1;
        app.busy = true;
        app.status = "Fetching from Spotify... playback controls remain available.".into();
        let request = app.request;
        let browse = app.browse.clone();
        let catalog = self.catalog.clone();
        let tx = self.tx.clone();
        if app.view == View::Search && app.query.trim().is_empty() {
            app.busy = false;
            app.next = None;
            app.status =
                "Enter a search. F2: Spotify catalog | F3: all saved Liked Songs and playlists."
                    .into();
            return;
        }
        if app.view == View::Search && app.search_scope == SearchScope::Library {
            app.reset_rows();
            app.selected = 0;
            app.library_scanned = 0;
            app.next = None;
            app.title = "Saved library — scanning".into();
            app.status = "Scanning saved Liked Songs and playlists. Partial matches appear as pages arrive; Esc cancels.".into();
            let query = app.query.clone();
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
    fn metadata(&mut self, app: &App) {
        if self.metadata_blocked || self.metadata.is_some() {
            return;
        }
        let mut ids = Vec::new();
        if let Some(id) = app.queue.current() {
            ids.push(id.to_owned());
        }
        let start = if app.view == View::Queue {
            app.queue_scroll.get()
        } else {
            app.queue.cursor.unwrap_or(0)
        };
        ids.extend(
            app.queue
                .order
                .iter()
                .skip(start)
                .take(app.queue_height.get().clamp(10, 200) + 8)
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
    fn retry_metadata(&mut self, app: &mut App) {
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
                .skip(app.queue_scroll.get())
                .take(app.queue_height.get().clamp(10, 200) + 8)
                .map(|i| app.queue.ids[*i].clone()),
        );
        for id in ids {
            app.cache.remove(&id);
        }
        app.lyrics_request += 1;
        app.lyrics_track_id = None;
        app.lyrics_error = None;
        app.lyrics = None;
        app.lyrics_loading = false;
        if let Some(t) = self.lyrics.take() {
            t.abort();
        }
    }
    fn view(&mut self, app: &mut App, view: View) {
        app.view = view;
        app.nav = view.index();
        app.sidebar = false;
        app.show_lyrics = false;
        app.show_visualizer = false;
        app.show_stats = false;
        app.selected = 0;
        app.editing = false;
        app.filter.clear();
        app.filtering = false;
        app.request += 1;
        app.busy = false;
        if let Some(task) = self.browse.take() {
            task.abort();
        }
        match view {
            View::Search => {
                app.browse = Browse::Search(app.query.clone());
                app.title = app.search_scope.label().into();
            }
            View::Playlists => {
                app.browse = Browse::Playlists;
                app.title = "Playlists".into();
            }
            View::Liked => {
                app.browse = Browse::Liked;
                app.title = "Liked Songs".into();
            }
            _ => return,
        }
        app.reset_rows();
        app.next = None;
        self.request(app, 0);
    }
}

fn format_time(ms: u32) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1_000 % 60)
}

fn choose_search(app: &mut App, scope: SearchScope, tasks: &mut Tasks) {
    if app.is_filtered() {
        app.query = app.filter.clone();
    }
    app.search_scope = scope;
    tasks.view(app, View::Search);
    app.context_menu = None;
    app.show_lyrics = false;
    app.show_visualizer = false;
    app.show_stats = false;
    app.editing = app.query.trim().is_empty();
}
fn perform_undo(app: &mut App, tasks: &mut Tasks, tx: &mpsc::UnboundedSender<Command>) {
    let existed = app.can_undo();
    app.undo_queue(tx);
    if existed {
        tasks.view(app, View::Queue);
        tasks.sync_queue_epoch(app.queue.epoch);
    }
}

fn activate_menu(
    app: &mut App,
    action: usize,
    tasks: &mut Tasks,
    tx: &mpsc::UnboundedSender<Command>,
) {
    let Some(menu) = app.context_menu.take() else {
        return;
    };
    let revision = if menu.view == View::Queue {
        app.queue.revision
    } else {
        app.rows_revision
    };
    if app.view != menu.view
        || app.selection() != menu.row
        || revision != menu.revision
        || app.filter != menu.filter
    {
        app.status = "List changed; right-click the track again.".into();
        return;
    }
    if let Some((_, code)) = menu.actions.get(action) {
        key(app, KeyEvent::new(*code, KeyModifiers::NONE), tasks, tx);
    }
}

fn mouse(
    app: &mut App,
    event: MouseEvent,
    tasks: &mut Tasks,
    tx: &mpsc::UnboundedSender<Command>,
) -> bool {
    if !matches!(
        event.kind,
        MouseEventKind::Down(MouseButton::Left | MouseButton::Right)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
    ) {
        return false;
    }
    let hit = app
        .mouse_hits
        .borrow()
        .iter()
        .rev()
        .find(|(area, _)| area.contains((event.column, event.row).into()))
        .copied();
    if app.context_menu.is_some() {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some((_, MouseTarget::Menu(index))) = hit {
                activate_menu(app, index, tasks, tx);
                return true;
            }
        }
        app.context_menu = None;
        return true;
    }
    let Some((area, target)) = hit else {
        return false;
    };
    if event.kind == MouseEventKind::ScrollUp || event.kind == MouseEventKind::ScrollDown {
        if app.show_stats {
            let code = if event.kind == MouseEventKind::ScrollUp {
                KeyCode::Up
            } else {
                KeyCode::Down
            };
            for _ in 0..3 {
                key(app, KeyEvent::new(code, KeyModifiers::NONE), tasks, tx);
            }
            return true;
        }
        if app.show_visualizer
            || (app.show_lyrics
                && app
                    .lyrics
                    .as_ref()
                    .is_none_or(|lyrics| !lyrics.lines.is_empty()))
        {
            return false;
        }
        if !matches!(
            target,
            MouseTarget::Catalog(_)
                | MouseTarget::Queue(_)
                | MouseTarget::CatalogScroll
                | MouseTarget::QueueScroll
        ) {
            return false;
        }
        if matches!(target, MouseTarget::Queue(_) | MouseTarget::QueueScroll)
            && app.view != View::Queue
        {
            tasks.view(app, View::Queue);
            app.show_lyrics = false;
            app.show_visualizer = false;
            app.show_stats = false;
        }
        app.editing = false;
        app.filtering = false;
        app.sidebar = false;
        let code = if event.kind == MouseEventKind::ScrollUp {
            KeyCode::Up
        } else {
            KeyCode::Down
        };
        for _ in 0..3 {
            key(app, KeyEvent::new(code, KeyModifiers::NONE), tasks, tx);
        }
        return true;
    }
    let right = event.kind == MouseEventKind::Down(MouseButton::Right);
    if app.show_stats {
        return false;
    }
    match target {
        MouseTarget::SearchMode(scope) if !right => choose_search(app, scope, tasks),
        MouseTarget::Navigation(view) if !right => {
            tasks.view(app, view);
            app.show_lyrics = false;
            app.show_visualizer = false;
            app.show_stats = false;
        }
        MouseTarget::QueueScroll if right && app.can_undo() => {
            if app.view != View::Queue {
                tasks.view(app, View::Queue);
            }
            app.sidebar = false;
            app.editing = false;
            app.filtering = false;
            app.context_menu = Some(ContextMenu {
                x: event.column,
                y: event.row,
                selected: 0,
                actions: vec![("Undo queue change", KeyCode::Char('u'))],
                view: app.view,
                row: app.selection(),
                revision: app.queue.revision,
                filter: app.filter.clone(),
            });
        }
        MouseTarget::Prompt if !right => {
            app.sidebar = false;
            if app.view == View::Search {
                app.editing = true;
            } else {
                app.filtering = true;
            }
        }
        MouseTarget::Catalog(index) | MouseTarget::Queue(index) => {
            if matches!(target, MouseTarget::Queue(_)) {
                if app.view != View::Queue {
                    tasks.view(app, View::Queue);
                }
                app.show_lyrics = false;
                app.show_visualizer = false;
                app.show_stats = false;
                app.queue.selected = index.min(app.queue.order.len().saturating_sub(1));
            } else {
                app.selected = index;
            }
            app.editing = false;
            app.filtering = false;
            app.sidebar = false;
            if right {
                let playlist =
                    app.view == View::Playlists && matches!(app.rows, Rows::Playlists(_));
                let mut actions = if playlist {
                    vec![
                        ("Open playlist", KeyCode::Enter),
                        ("Add playlist to queue", KeyCode::Char('a')),
                    ]
                } else {
                    vec![
                        ("Play", KeyCode::Enter),
                        ("Add to queue", KeyCode::Char('a')),
                        ("Play next", KeyCode::Char('A')),
                    ]
                };
                if app.view == View::Queue {
                    actions.push(("Remove from queue", KeyCode::Delete));
                    if app.can_undo() {
                        actions.push(("Undo queue change", KeyCode::Char('u')));
                    }
                }
                app.context_menu = Some(ContextMenu {
                    x: event.column,
                    y: event.row,
                    selected: 0,
                    actions,
                    view: app.view,
                    row: app.selection(),
                    filter: app.filter.clone(),
                    revision: if app.view == View::Queue {
                        app.queue.revision
                    } else {
                        app.rows_revision
                    },
                });
            } else {
                app.status = "Selected. Right-click for actions; Enter plays/opens.".into();
            }
        }
        MouseTarget::PlayPause if !right => {
            app.editing = false;
            app.filtering = false;
            key(
                app,
                KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
                tasks,
                tx,
            );
        }
        MouseTarget::Seek if !right && app.loaded => {
            if let Some(track) = app.current_track().filter(|t| t.duration_ms > 0) {
                let fraction = event.column.saturating_sub(area.x) as u64;
                let position = (fraction * track.duration_ms.saturating_sub(1) as u64
                    / area.width.saturating_sub(1).max(1) as u64)
                    as u32;
                app.queue.position_ms = position;
                app.position_anchor =
                    (app.state == State::Playing).then_some((Instant::now(), position));
                app.send(tx, Command::Seek(position));
            }
        }
        _ => return false,
    }
    true
}

fn key(app: &mut App, key: KeyEvent, tasks: &mut Tasks, tx: &mpsc::UnboundedSender<Command>) {
    if key.kind == KeyEventKind::Release {
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.quit = true;
        return;
    }
    if app.show_stats {
        match key.code {
            KeyCode::Char('S') | KeyCode::Esc => {
                app.show_stats = false;
                app.selected = app.selected.min(app.len().saturating_sub(1));
                app.status = "Exited song statistics".into();
            }
            KeyCode::Char('q') => app.quit = true,
            KeyCode::Up | KeyCode::Char('k') => {
                app.selected = app.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max_idx = app.stats.len().saturating_sub(1);
                app.selected = (app.selected + 1).min(max_idx);
            }
            KeyCode::PageUp => {
                app.selected = app.selected.saturating_sub(15);
            }
            KeyCode::PageDown | KeyCode::Char('>') => {
                let max_idx = app.stats.len().saturating_sub(1);
                app.selected = (app.selected + 15).min(max_idx);
            }
            KeyCode::Char(' ') => app.media_action(MediaAction::Toggle, tx),
            KeyCode::Char('n') => app.media_action(MediaAction::Next, tx),
            KeyCode::Char('p') => app.media_action(MediaAction::Previous, tx),
            KeyCode::Home | KeyCode::End | KeyCode::Left | KeyCode::Right => {
                if let Some(track) = app.current_track() {
                    let duration = track.duration_ms;
                    if duration > 0 {
                        let position = match key.code {
                            KeyCode::Home => 0,
                            KeyCode::End => duration.saturating_sub(1),
                            KeyCode::Left => app.queue.position_ms.saturating_sub(10_000),
                            KeyCode::Right => app.queue.position_ms.saturating_add(10_000),
                            _ => unreachable!(),
                        };
                        app.queue.position_ms = position.min(duration.saturating_sub(1));
                        app.anchor_position();
                        if app.loaded {
                            app.send(tx, Command::Seek(app.queue.position_ms));
                        }
                        app.status = format!(
                            "Seeked to {}. Left/Right seek 10s | Home/End jump",
                            format_time(app.queue.position_ms)
                        );
                    }
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                app.config.volume = (app.config.volume + 5).min(100);
                app.muted_volume = None;
                app.send(tx, Command::Volume(app.config.volume));
                app.status = format!("Volume {}%", app.config.volume);
            }
            KeyCode::Char('-') => {
                app.config.volume = app.config.volume.saturating_sub(5);
                if app.config.volume > 0 {
                    app.muted_volume = None;
                }
                app.send(tx, Command::Volume(app.config.volume));
                app.status = format!("Volume {}%", app.config.volume);
            }
            KeyCode::Char(']') => {
                app.config.volume = (app.config.volume + 1).min(100);
                app.muted_volume = None;
                app.send(tx, Command::Volume(app.config.volume));
                app.status = format!("Volume {}% (fine)", app.config.volume);
            }
            KeyCode::Char('[') => {
                app.config.volume = app.config.volume.saturating_sub(1);
                if app.config.volume > 0 {
                    app.muted_volume = None;
                }
                app.send(tx, Command::Volume(app.config.volume));
                app.status = format!("Volume {}% (fine)", app.config.volume);
            }
            KeyCode::Char('m') => {
                if app.config.volume == 0 {
                    app.config.volume = app.muted_volume.take().unwrap_or(50);
                } else {
                    app.muted_volume = Some(app.config.volume);
                    app.config.volume = 0;
                }
                app.send(tx, Command::Volume(app.config.volume));
                app.status = if app.config.volume == 0 {
                    "Muted. Press m to restore volume".into()
                } else {
                    format!("Volume {}%", app.config.volume)
                };
            }
            _ => (),
        }
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('z') {
        perform_undo(app, tasks, tx);
        return;
    }
    if matches!(key.code, KeyCode::F(2) | KeyCode::F(3)) {
        choose_search(
            app,
            if key.code == KeyCode::F(2) {
                SearchScope::Spotify
            } else {
                SearchScope::Library
            },
            tasks,
        );
        return;
    }
    if let Some(menu) = &mut app.context_menu {
        match key.code {
            KeyCode::Esc => app.context_menu = None,
            KeyCode::Down => menu.selected = (menu.selected + 1) % menu.actions.len(),
            KeyCode::Up => {
                menu.selected = (menu.selected + menu.actions.len() - 1) % menu.actions.len()
            }
            KeyCode::Enter => {
                let action = menu.selected;
                activate_menu(app, action, tasks, tx);
            }
            KeyCode::Char('q') => app.quit = true,
            _ => (),
        }
        return;
    }
    if app.editing {
        match key.code {
            KeyCode::Esc => app.editing = false,
            KeyCode::Enter => {
                app.editing = false;
                app.browse = Browse::Search(app.query.trim().into());
                app.selected = 0;
                app.reset_rows();
                tasks.request(app, 0);
            }
            KeyCode::Backspace => {
                app.query.pop();
            }
            KeyCode::Char(c) if !c.is_control() && app.query.len() < 500 => app.query.push(c),
            _ => (),
        }
        return;
    }
    if app.filtering {
        match key.code {
            KeyCode::Esc => {
                app.filter.clear();
                app.filtering = false;
                app.selected = 0;
                return;
            }
            KeyCode::Enter => {
                app.filtering = false;
            }
            KeyCode::Down => {
                app.filtering = false;
                if app.len() > 1 {
                    app.selected = 1;
                }
                return;
            }
            KeyCode::Tab => {
                app.filtering = false;
                return;
            }
            KeyCode::Backspace => {
                app.filter.pop();
                app.selected = 0;
                return;
            }
            KeyCode::Char(c) if !c.is_control() && app.filter.len() < 100 => {
                app.filter.push(c);
                app.selected = 0;
                return;
            }
            _ => return,
        }
    }
    if app.show_lyrics && app.lyrics.as_ref().is_some_and(|l| l.lines.is_empty()) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                app.lyrics_scroll =
                    (app.lyrics_scroll + 1).min(app.lyrics_length.get().saturating_sub(1));
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.lyrics_scroll = app.lyrics_scroll.saturating_sub(1);
                return;
            }
            KeyCode::PageDown => {
                app.lyrics_scroll =
                    (app.lyrics_scroll + 10).min(app.lyrics_length.get().saturating_sub(1));
                return;
            }
            KeyCode::PageUp => {
                app.lyrics_scroll = app.lyrics_scroll.saturating_sub(10);
                return;
            }
            _ => (),
        }
    }
    match key.code {
        KeyCode::Char('u') => perform_undo(app, tasks, tx),
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Esc
            if app.view == View::Search && app.search_scope == SearchScope::Library && app.busy =>
        {
            if let Some(scan) = tasks.browse.take() {
                scan.abort();
            }
            app.request += 1;
            app.busy = false;
            app.title = "Saved library — partial results".into();
            app.status = format!(
                "Library search cancelled after {} tracks. Partial results retained; F5 restarts.",
                app.library_scanned
            );
        }
        KeyCode::Esc if app.show_lyrics => {
            app.show_lyrics = false;
            app.status = "Exited lyrics view".into();
        }
        KeyCode::Esc if app.show_visualizer => {
            app.show_visualizer = false;
            app.status = "Exited visualizer".into();
        }
        KeyCode::Esc if !app.filter.is_empty() => {
            app.filter.clear();
            app.filtering = false;
            app.selected = 0;
        }
        KeyCode::Esc if app.view != View::Help => app.quit = true,
        KeyCode::Esc => tasks.view(app, View::Search),
        KeyCode::Tab | KeyCode::BackTab => {
            app.sidebar = !app.sidebar;
            app.nav = app.view.index();
        }
        KeyCode::Char('?') | KeyCode::F(1) => tasks.view(app, View::Help),
        KeyCode::Char(c @ '1'..='5') => tasks.view(app, View::ALL[c as usize - '1' as usize]),
        KeyCode::Char('/') => {
            if matches!(app.view, View::Liked | View::Playlists) {
                app.filter.clear();
                app.filtering = true;
                app.selected = 0;
            } else {
                tasks.view(app, View::Search);
                app.editing = true;
            }
        }
        KeyCode::Char('f') if matches!(app.view, View::Liked | View::Playlists) => {
            app.filter.clear();
            app.filtering = true;
            app.selected = 0;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.sidebar {
                app.nav = app.nav.saturating_sub(1);
            } else if app.view == View::Queue {
                app.queue.selected = app.queue.selected.saturating_sub(1);
            } else {
                app.selected = app.selected.saturating_sub(1);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.sidebar {
                app.nav = (app.nav + 1).min(4);
            } else if app.view == View::Queue {
                app.queue.selected =
                    (app.queue.selected + 1).min(app.queue.ids.len().saturating_sub(1));
            } else {
                let at = (app.selected + 1).min(app.len().saturating_sub(1));
                app.selected = at;
                if !app.show_stats && !app.busy && !app.is_filtered() && at + 5 >= app.len() {
                    if let Some(offset) = app.next {
                        tasks.request(app, offset);
                    }
                }
            }
        }
        KeyCode::PageUp => {
            if app.sidebar {
                app.nav = 0;
            } else if app.view == View::Queue {
                app.queue.selected = app.queue.selected.saturating_sub(15);
            } else {
                app.selected = app.selected.saturating_sub(15);
            }
        }
        KeyCode::PageDown | KeyCode::Char('>') if !matches!(app.view, View::Help) => {
            if app.sidebar {
                app.nav = 4;
            } else if app.view == View::Queue {
                app.queue.selected =
                    (app.queue.selected + 15).min(app.queue.ids.len().saturating_sub(1));
            } else {
                app.selected = (app.selected + 15).min(app.len().saturating_sub(1));
                if !app.busy && !app.is_filtered() {
                    if let Some(offset) = app.next {
                        tasks.request(app, offset);
                    }
                }
            }
        }
        KeyCode::F(5) => {
            tasks.retry_metadata(app);
            if matches!(app.view, View::Search | View::Playlists | View::Liked) {
                tasks.request(app, 0);
            }
        }
        KeyCode::Backspace if matches!(app.browse, Browse::Playlist(_)) => {
            tasks.view(app, View::Playlists)
        }
        KeyCode::Enter if app.sidebar => tasks.view(app, View::ALL[app.nav]),
        KeyCode::Enter if app.view != View::Help => {
            if let Rows::Playlists(rows) = &app.rows {
                if app.view == View::Playlists {
                    let actual_idx = if app.is_filtered() {
                        app.filtered_indices().get(app.selected).copied()
                    } else {
                        Some(app.selected)
                    };
                    if let Some(idx) = actual_idx {
                        if let Some(p) = rows.get(idx).cloned() {
                            app.browse = Browse::Playlist(p.id);
                            app.title = p.name;
                            app.reset_rows();
                            app.selected = 0;
                            app.filter.clear();
                            app.filtering = false;
                            tasks.request(app, 0);
                        }
                    }
                    return;
                }
            }
            if let Some(track) = app.selected_track() {
                if !track.playable {
                    app.status = "This track is unavailable for your account or region.".into();
                    return;
                }
                if app.view == View::Queue {
                    app.queue.select(app.queue.selected);
                } else if let Rows::Tracks(tracks) = &app.rows {
                    let (track_ids, index) = if app.is_filtered() {
                        let filtered = app.filtered_indices();
                        let playable_filtered: Vec<(usize, &Track)> = filtered
                            .iter()
                            .filter_map(|&i| tracks.get(i).map(|t| (i, t)))
                            .filter(|(_, t)| t.playable)
                            .collect();
                        let play_idx = playable_filtered
                            .iter()
                            .position(|(orig_idx, _)| filtered.get(app.selected) == Some(orig_idx))
                            .unwrap_or(0);
                        let ids: Vec<String> = playable_filtered
                            .into_iter()
                            .map(|(_, t)| t.id.clone())
                            .collect();
                        (ids, play_idx)
                    } else {
                        let index = tracks[..app.selected.min(tracks.len())]
                            .iter()
                            .filter(|t| t.playable)
                            .count();
                        let ids: Vec<String> = tracks
                            .iter()
                            .filter(|t| t.playable)
                            .map(|t| t.id.clone())
                            .collect();
                        (ids, index)
                    };
                    if track_ids.len() > crate::queue::MAX_TRACKS {
                        app.status = "This list exceeds the 100,000-track queue limit; filter it before playing.".into();
                        return;
                    }
                    app.remember_queue();
                    app.queue.replace(track_ids, index, app.config.shuffle);
                }
                app.cache.insert(track.id.clone(), track);
                app.load(tx);
            }
        }
        KeyCode::Char(' ') => app.media_action(MediaAction::Toggle, tx),
        KeyCode::Char('n') => app.media_action(MediaAction::Next, tx),
        KeyCode::Char('p') => app.media_action(MediaAction::Previous, tx),
        KeyCode::Home | KeyCode::End | KeyCode::Left | KeyCode::Right => {
            let Some(track) = app.current_track() else {
                app.status = "Choose a track before seeking.".into();
                return;
            };
            let duration = track.duration_ms;
            if duration == 0 {
                app.status = "Track duration is not available yet.".into();
                return;
            }
            let position = match key.code {
                KeyCode::Home => 0,
                KeyCode::End => duration.saturating_sub(1),
                KeyCode::Left => app.queue.position_ms.saturating_sub(10_000),
                KeyCode::Right => app.queue.position_ms.saturating_add(10_000),
                _ => unreachable!(),
            };
            app.queue.position_ms = position.min(duration.saturating_sub(1));
            app.anchor_position();
            if app.loaded {
                app.send(tx, Command::Seek(app.queue.position_ms));
            }
            app.status = format!(
                "Seeked to {}. Left/Right seek 10s | Home/End jump",
                format_time(app.queue.position_ms)
            );
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            app.config.volume = (app.config.volume + 5).min(100);
            app.muted_volume = None;
            app.send(tx, Command::Volume(app.config.volume));
            app.status = format!("Volume {}%", app.config.volume);
        }
        KeyCode::Char('-') => {
            app.config.volume = app.config.volume.saturating_sub(5);
            if app.config.volume > 0 {
                app.muted_volume = None;
            }
            app.send(tx, Command::Volume(app.config.volume));
            app.status = format!("Volume {}%", app.config.volume);
        }
        KeyCode::Char(']') => {
            app.config.volume = (app.config.volume + 1).min(100);
            app.muted_volume = None;
            app.send(tx, Command::Volume(app.config.volume));
            app.status = format!("Volume {}% (fine)", app.config.volume);
        }
        KeyCode::Char('[') => {
            app.config.volume = app.config.volume.saturating_sub(1);
            if app.config.volume > 0 {
                app.muted_volume = None;
            }
            app.send(tx, Command::Volume(app.config.volume));
            app.status = format!("Volume {}% (fine)", app.config.volume);
        }
        KeyCode::Char('m') => {
            if app.config.volume == 0 {
                app.config.volume = app.muted_volume.take().unwrap_or(50);
            } else {
                app.muted_volume = Some(app.config.volume);
                app.config.volume = 0;
            }
            app.send(tx, Command::Volume(app.config.volume));
            app.status = if app.config.volume == 0 {
                "Muted. Press m to restore volume".into()
            } else {
                format!("Volume {}%", app.config.volume)
            };
        }
        KeyCode::Char('s') => {
            app.remember_queue();
            app.config.shuffle = !app.config.shuffle;
            app.queue.set_shuffle(app.config.shuffle);
            app.status = format!("Shuffle {}", if app.config.shuffle { "on" } else { "off" });
        }
        KeyCode::Char('r') => {
            app.config.repeat = app.config.repeat.cycle();
            app.status = format!("Repeat: {:?}", app.config.repeat);
        }
        KeyCode::Char('t') => {
            let current = ui::Theme::from_str(&app.config.theme);
            let next = current.next();
            app.config.theme = next.as_str().to_string();
            app.status = format!("Theme: {}", next.name());
        }
        KeyCode::Char('l') => {
            app.show_lyrics = !app.show_lyrics;
            if app.show_lyrics {
                app.show_visualizer = false;
                app.show_stats = false;
                app.status = "Lyrics active (press l or Esc to exit)".into();
            } else {
                app.status = "Exited lyrics".into();
            }
        }
        KeyCode::Char('v') => {
            app.show_visualizer = !app.show_visualizer;
            if app.show_visualizer {
                app.show_lyrics = false;
                app.show_stats = false;
                app.status = "Retro visualizer active (press v or Esc to exit)".into();
            } else {
                app.status = "Exited visualizer".into();
            }
        }
        KeyCode::Char('S') => {
            app.show_stats = true;
            app.show_lyrics = false;
            app.show_visualizer = false;
            app.sidebar = false;
            app.selected = 0;
            app.stats_scroll.set(0);
            app.stats.refresh_metadata(&app.cache);
            app.status = "Song statistics (press S or Esc to exit)".into();
        }
        KeyCode::Char('a') if app.view != View::Help => {
            if let Rows::Playlists(playlists) = &app.rows {
                if app.view == View::Playlists {
                    let actual_idx = if app.is_filtered() {
                        app.filtered_indices().get(app.selected).copied()
                    } else {
                        Some(app.selected)
                    };
                    if let Some(idx) = actual_idx {
                        if let Some(p) = playlists.get(idx).cloned() {
                            tasks.enqueue_playlist(app, p.id, p.name);
                            return;
                        }
                    }
                }
            }
            if let Some(track) = app.selected_track() {
                if track.playable {
                    if app.queue.ids.len() < crate::queue::MAX_TRACKS {
                        app.remember_queue();
                    }
                    app.status = if app.queue.enqueue(track.id) {
                        format!("Added {} to queue", track.name)
                    } else {
                        "Queue limit reached (100,000 tracks).".into()
                    };
                } else {
                    app.status = "Unavailable track cannot be queued.".into();
                }
            }
        }
        KeyCode::Char('A') if app.view != View::Help => {
            if let Some(track) = app.selected_track() {
                if track.playable {
                    if app.queue.ids.len() < crate::queue::MAX_TRACKS {
                        app.remember_queue();
                    }
                    app.status = if app.queue.insert_next(track.id) {
                        format!("Playing next: {}", track.name)
                    } else {
                        "Queue limit reached (100,000 tracks).".into()
                    };
                } else {
                    app.status = "Unavailable track cannot be queued.".into();
                }
            }
        }
        KeyCode::Char('R') if app.view != View::Help => {
            if let Some(track) = app.selected_track() {
                if track.artists.is_empty() || track.name.is_empty() {
                    app.status =
                        "Wait for track metadata before starting Radio; F5 retries metadata."
                            .into();
                    return;
                }
                if track.playable {
                    let selected_track = track.clone();
                    app.remember_queue();
                    app.queue.replace(vec![selected_track.id.clone()], 0, false);
                    app.load(tx);
                    tasks.fetch_recommendations(&selected_track, app.queue.epoch);
                    app.status = format!(
                        "Track Radio: {} • Loading recommended tracks...",
                        selected_track.name
                    );
                } else {
                    app.status = "Unavailable track cannot start Radio.".into();
                }
            }
        }
        KeyCode::Char('C') if app.view == View::Queue => {
            if !app.queue.ids.is_empty() {
                app.remember_queue();
            }
            if app.queue.clear() {
                app.stop(tx);
            }
            app.status = "Queue cleared. Press u to undo.".into();
        }
        KeyCode::Char('K') if app.view == View::Queue && app.queue.selected > 0 => {
            let from = app.queue.selected;
            let to = from - 1;
            app.remember_queue();
            app.queue.move_item(from, to);
            app.status = "Moved track up in queue".into();
        }
        KeyCode::Char('J')
            if app.view == View::Queue
                && !app.queue.ids.is_empty()
                && app.queue.selected + 1 < app.queue.ids.len() =>
        {
            let from = app.queue.selected;
            let to = from + 1;
            app.remember_queue();
            app.queue.move_item(from, to);
            app.status = "Moved track down in queue".into();
        }
        KeyCode::Char('.') | KeyCode::Char('c') if app.view == View::Queue => {
            if let Some(c) = app.queue.cursor {
                app.queue.selected = c;
                app.status = "Jumped to currently playing track.".into();
            }
        }
        KeyCode::Delete | KeyCode::Char('d') | KeyCode::Char('x') if app.view == View::Queue => {
            if app.queue.selected < app.queue.order.len() {
                app.remember_queue();
            }
            if app.queue.remove(app.queue.selected) {
                app.stop(tx);
            }
            app.status = "Queue item removed. Press u to undo.".into();
        }
        _ => (),
    }
}

fn background(app: &mut App, tasks: &mut Tasks, event: Background) -> bool {
    match event {
        Background::LibraryProgress(id, progress) if id == app.request => {
            app.library_scanned = progress.scanned;
            let count = progress.tracks.len();
            if let Rows::Tracks(rows) = &mut app.rows {
                rows.extend(progress.tracks);
            }
            if count > 0 {
                app.rows_revision = app.rows_revision.wrapping_add(1);
            }
            app.title = if progress.complete {
                "Saved library — complete"
            } else {
                "Saved library — scanning"
            }
            .into();
            app.status = format!(
                "{} matches; {} saved tracks scanned. {}",
                app.raw_len(),
                app.library_scanned,
                if progress.complete {
                    "Search complete."
                } else {
                    "Partial results; Esc cancels."
                }
            );
        }
        Background::LibraryDone(id, result) if id == app.request => {
            app.busy = false;
            tasks.browse = None;
            match result {
                Ok(()) => {
                    app.title = "Saved library — complete".into();
                    app.status = format!(
                        "{} matches across {} saved tracks. F2 searches Spotify; / edits the query.",
                        app.raw_len(),
                        app.library_scanned
                    );
                }
                Err(error) => {
                    app.title = "Saved library — partial results".into();
                    app.status = format!(
                        "Library search stopped: {error:#}. {} partial matches retained; F5 restarts.",
                        app.raw_len()
                    );
                }
            }
        }
        Background::Page(id, result) if id == app.request => {
            app.busy = false;
            match result {
                Ok(page) => {
                    app.next = page.next;
                    if let Rows::Tracks(tracks) = &page.rows {
                        for track in tracks {
                            app.cache.insert(track.id.clone(), track.clone());
                        }
                    }
                    app.rows_revision += 1;
                    if page.offset == 0 {
                        app.rows = page.rows;
                        app.selected = 0;
                        app.catalog_scroll.set(0);
                    } else {
                        match (&mut app.rows, page.rows) {
                            (Rows::Tracks(a), Rows::Tracks(b)) => a.extend(b),
                            (Rows::Playlists(a), Rows::Playlists(b)) => a.extend(b),
                            _ => (),
                        }
                    }
                    app.status = format!(
                        "{} loaded | Enter play/open | a enqueue{}",
                        app.len(),
                        if app.next.is_some() {
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
        Background::Lyrics(request, result) if request == app.lyrics_request => {
            app.lyrics_loading = false;
            match result {
                Ok(Some(lyrics)) => app.lyrics = Some(lyrics),
                Ok(None) => {
                    app.lyrics_error = Some("No lyrics found for this track. F5 retries.".into())
                }
                Err(e) => {
                    app.lyrics_error = Some(format!("Lyrics request failed: {e:#}. F5 retries."))
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
                if track.playable && app.queue.enqueue(track.id.clone()) {
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
        Background::Recommendations(epoch, result) if epoch == app.queue.epoch => match result {
            Ok(tracks) => {
                let mut seen: HashSet<String> = app.queue.ids.iter().cloned().collect();
                let mut added = 0;
                for track in tracks {
                    if track.playable
                        && seen.insert(track.id.clone())
                        && app.queue.enqueue(track.id.clone())
                    {
                        added += 1;
                        app.cache.insert(track.id.clone(), track);
                    }
                }
                app.stats.refresh_metadata(&app.cache);
                app.status = format!("Track Radio: added {added} related tracks.");
            }
            Err(e) => app.status = format!("Track Radio failed: {e:#}. Press R to retry."),
        },
        Background::SaveError(error) => {
            app.status = error;
            return true;
        }
        _ => (),
    }
    false
}

/// Each independent file has a coalescing writer. Failed snapshots are retried
/// by the next checkpoint; the final failure is returned after terminal cleanup.
fn writer<T, F>(
    mut rx: watch::Receiver<Option<T>>,
    tx: mpsc::UnboundedSender<Background>,
    save: F,
) -> tokio::task::JoinHandle<Result<()>>
where
    T: Clone + Send + Sync + 'static,
    F: Fn(T) -> Result<()> + Send + Sync + 'static,
{
    let save = Arc::new(save);
    tokio::spawn(async move {
        let mut last_error = None;
        while rx.changed().await.is_ok() {
            let value = rx.borrow_and_update().clone();
            let Some(value) = value else {
                continue;
            };
            let save = save.clone();
            match tokio::task::spawn_blocking(move || save(value)).await {
                Ok(Ok(())) => last_error = None,
                result => {
                    let error = match result {
                        Ok(Err(e)) => format!("Could not save state: {e:#}"),
                        Err(e) => format!("State writer failed: {e}"),
                        _ => unreachable!(),
                    };
                    let _ = tx.send(Background::SaveError(error.clone()));
                    last_error = Some(error);
                }
            }
        }
        if let Some(e) = last_error {
            anyhow::bail!(e);
        }
        Ok(())
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct QueueStamp(u64, Option<usize>, usize, u32);
fn queue_stamp(queue: &Queue) -> QueueStamp {
    QueueStamp(
        queue.revision,
        queue.cursor,
        queue.selected,
        queue.position_ms,
    )
}
struct Checkpoints {
    config: Config,
    queue: QueueStamp,
    cache: u64,
    stats: u64,
    retry: bool,
    config_tx: watch::Sender<Option<Config>>,
    queue_tx: watch::Sender<Option<Queue>>,
    cache_tx: watch::Sender<Option<crate::cache::MetadataCache>>,
    stats_tx: watch::Sender<Option<crate::stats::SongStats>>,
}
impl Checkpoints {
    fn send(&mut self, app: &App) {
        if self.retry || self.config != app.config {
            self.config_tx.send_replace(Some(app.config.clone()));
            self.config = app.config.clone();
        }
        let stamp = queue_stamp(&app.queue);
        if self.retry || self.queue != stamp {
            self.queue_tx.send_replace(Some(app.queue.clone()));
            self.queue = stamp;
        }
        if self.retry || self.cache != app.cache.revision {
            self.cache_tx.send_replace(Some(app.cache.clone()));
            self.cache = app.cache.revision;
        }
        if self.retry || self.stats != app.stats.revision {
            self.stats_tx.send_replace(Some(app.stats.clone()));
            self.stats = app.stats.revision;
        }
        self.retry = false;
    }
}

pub async fn run(store: Storage) -> Result<()> {
    let config = store.config()?;
    let queue = store.queue()?;
    let catalog = Catalog::new(TokenManager::load(&config)?)?;
    let mut app = App::new(config, queue);
    let mut playback = playback::Playback::spawn_with_visualizer(
        TokenManager::load_streaming()?,
        app.config.client_id.clone(),
        app.config.volume,
        app.visualizer.clone(),
    );
    match store.cache() {
        Ok(cache) => app.cache = cache,
        Err(_) => app.status = "Old or invalid metadata cache ignored; names will reload. Use clear-cache to remove it.".into(),
    }
    match store.stats() {
        Ok(stats) => app.stats = stats,
        Err(_) => app.status = "Old or invalid song statistics ignored; starting fresh.".into(),
    }
    app.stats.refresh_metadata(&app.cache);
    let (bg_tx, mut bg_rx) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(catalog, bg_tx.clone())?;
    let (config_tx, config_rx) = watch::channel(None);
    let (queue_tx, queue_rx) = watch::channel(None);
    let (cache_tx, cache_rx) = watch::channel(None);
    let (stats_tx, stats_rx) = watch::channel(None);
    let config_store = store.clone();
    let queue_store = store.clone();
    let cache_store = store.clone();
    let stats_store = store.clone();
    let config_writer = writer(config_rx, bg_tx.clone(), move |config| {
        config_store.save_config(&config)
    });
    let queue_writer = writer(queue_rx, bg_tx.clone(), move |queue| {
        queue_store.save_queue(&queue)
    });
    let cache_writer = writer(cache_rx, bg_tx.clone(), move |cache| {
        cache_store.save_cache(&cache)
    });
    let stats_writer = writer(stats_rx, bg_tx, move |stats| stats_store.save_stats(&stats));
    let mut checkpoints = Checkpoints {
        config: app.config.clone(),
        queue: queue_stamp(&app.queue),
        cache: 0,
        stats: 0,
        retry: false,
        config_tx,
        queue_tx,
        cache_tx,
        stats_tx,
    };
    let mut terminal = ui::TerminalGuard::enter()?;
    let (media_tx, mut media_rx) = mpsc::unbounded_channel();
    let mut media_controls = match media_controls::MediaControls::spawn(media_tx) {
        Ok(controls) => Some(controls),
        Err(_) => {
            app.status = "Windows media controls unavailable; terminal controls still work.".into();
            None
        }
    };
    let mut discord_presence = crate::discord::DiscordPresence::spawn(
        app.config.discord_rpc,
        app.config.discord_client_id.clone(),
    );
    let mut current_window_title = String::new();
    let mut keys = EventStream::new();
    let mut save_tick = tokio::time::interval(Duration::from_secs(2));
    save_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut playback_open = true;
    let mut dirty = true;
    let mut metadata_dirty = true;
    let mut lyrics_dirty = true;
    let mut last_metadata_view = None;
    let mut last_draw = Instant::now() - Duration::from_millis(33);
    let result: Result<()> = async {
        loop {
            if let Some(controls) = &mut media_controls {
                controls.update(media_controls::Snapshot {
                    track: app.current_track(), state: app.state, position_ms: app.queue.position_ms,
                });
            }
            discord_presence.update(app.discord_snapshot());
            tasks.sync_queue_epoch(app.queue.epoch);
            if lyrics_dirty { tasks.update_lyrics(&mut app); lyrics_dirty = false; }
            if dirty && last_draw.elapsed() >= Duration::from_millis(33) {
                app.interpolate_position();
                app.account_playback_time(Instant::now());
                let title = app.window_title();
                if title != current_window_title { ui::set_title(&title); current_window_title = title; }
                terminal.terminal.draw(|frame| ui::draw(frame, &app))?;
                dirty = false; last_draw = Instant::now();
                let viewport = (app.queue.revision, app.queue.cursor, app.queue_scroll.get(), app.queue_height.get(), app.view);
                if last_metadata_view != Some(viewport) { metadata_dirty = true; last_metadata_view = Some(viewport); }
            }
            if metadata_dirty { tasks.metadata(&app); metadata_dirty = false; }
            let animation = app.animation_interval();
            let delay = if dirty { Duration::from_millis(33) } else { animation.unwrap_or(Duration::from_secs(1)) };
            tokio::select! {
                Some(event) = media_rx.recv() => {
                    match event {
                        media_controls::Event::Action(action) => app.media_action(action, &playback.commands),
                        media_controls::Event::Unavailable => {
                            media_controls = None;
                            app.status = "Windows media controls unavailable; terminal controls still work.".into();
                        }
                    }
                    app.mouse_hits.borrow_mut().clear();
                    dirty = true; metadata_dirty = true; lyrics_dirty = true;
                }
                key_event = keys.next() => {
                    match key_event {
                        Some(Ok(Input::Key(event))) if event.kind != KeyEventKind::Release => key(&mut app, event, &mut tasks, &playback.commands),
                        Some(Ok(Input::Mouse(event))) if !mouse(&mut app, event, &mut tasks, &playback.commands) => continue,
                        Some(Ok(Input::Resize(_, _))) => { app.context_menu = None; },
                        Some(Ok(Input::Paste(text))) if app.editing => app.query.extend(text.chars().filter(|c| !c.is_control()).take(500usize.saturating_sub(app.query.chars().count()))),
                        Some(Ok(Input::Paste(text))) if app.filtering => { app.filter.extend(text.chars().filter(|c| !c.is_control()).take(100usize.saturating_sub(app.filter.chars().count()))); app.selected = 0; }
                        Some(Err(e)) => return Err(e.into()), None => break,
                        Some(Ok(Input::Key(_))) => continue,
                        _ => (),
                    }
                    app.mouse_hits.borrow_mut().clear();
                    dirty = true; metadata_dirty = true; lyrics_dirty = true;
                }
                event = playback.events.recv(), if playback_open => {
                    if let Some(event) = event { app.playback_event(event, &playback.commands); }
                    else {
                        playback_open = false;
                        if app.state == State::Playing {
                            app.finalize_playback_accounting();
                        }
                        app.loaded = false;
                        app.state = State::Failed;
                        app.status = "Playback worker exited; restart Tuitify.".into();
                    }
                    app.mouse_hits.borrow_mut().clear();
                    dirty = true; metadata_dirty = true; lyrics_dirty = true;
                }
                Some(event) = bg_rx.recv() => {
                    checkpoints.retry |= background(&mut app, &mut tasks, event);
                    // Process bursts together; bound the batch so keyboard input stays fair.
                    for _ in 0..63 { match bg_rx.try_recv() { Ok(event) => checkpoints.retry |= background(&mut app, &mut tasks, event), Err(_) => break } }
                    app.mouse_hits.borrow_mut().clear();
                    dirty = true; metadata_dirty = true; lyrics_dirty = true;
                }
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(last_draw + delay)), if dirty || animation.is_some() => {
                    if animation.is_some() { app.animation_frame = app.animation_frame.wrapping_add(1); }
                    dirty = true;
                }
                _ = save_tick.tick() => {
                    app.interpolate_position();
                    app.account_playback_time(Instant::now());
                    if app.cache.prune_expired() { dirty = true; metadata_dirty = true; lyrics_dirty = true; }
                    checkpoints.send(&app);
                }
                _ = tokio::signal::ctrl_c() => break,
            }
            if app.quit { break; }
        }
        Ok(())
    }.await;
    app.interpolate_position();
    app.finalize_playback_accounting();
    app.send(&playback.commands, Command::Stop);
    checkpoints.send(&app);
    drop(checkpoints);
    drop(tasks);
    drop(terminal);
    drop(media_controls);
    discord_presence.close().await;
    let (config_saved, queue_saved, cache_saved, stats_saved) =
        tokio::join!(config_writer, queue_writer, cache_writer, stats_writer);
    config_saved??;
    queue_saved??;
    cache_saved??;
    stats_saved??;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discord_does_not_publish_restored_unknown_or_stopped_tracks() {
        let mut queue = Queue::default();
        queue.replace(vec![test_track(1).id], 0, false);
        let mut app = App::new(Config::default(), queue);
        assert!(app.discord_snapshot().track.is_none());
        let (tx, _rx) = mpsc::unbounded_channel();
        app.media_action(MediaAction::Play, &tx);
        assert!(app.discord_snapshot().track.is_none());
        app.cache.insert(test_track(1).id, test_track(1));
        assert!(crate::discord::build_activity(&app.discord_snapshot(), 1000, None).is_none());
        app.playback_event(
            Event::Playing {
                generation: app.generation,
                position_ms: 0,
            },
            &tx,
        );
        assert!(crate::discord::build_activity(&app.discord_snapshot(), 1000, None).is_some());
        app.media_action(MediaAction::Pause, &tx);
        assert!(crate::discord::build_activity(&app.discord_snapshot(), 1000, None).is_some());
        app.stop(&tx);
        assert!(app.discord_snapshot().track.is_none());
    }
    #[test]
    fn media_actions_work_during_text_entry_and_are_idempotent() {
        let mut queue = Queue::default();
        queue.replace(vec![test_track(1).id, test_track(2).id], 0, false);
        let mut app = App::new(Config::default(), queue);
        app.editing = true;
        app.filtering = true;
        app.query = "my search".into();
        app.filter = "my filter".into();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.media_action(MediaAction::Pause, &tx);
        assert!(rx.try_recv().is_err());
        app.media_action(MediaAction::Play, &tx);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Command::Load { position_ms: 0, .. }
        ));
        app.media_action(MediaAction::Play, &tx);
        assert!(rx.try_recv().is_err());
        app.media_action(MediaAction::Pause, &tx);
        assert!(matches!(rx.try_recv().unwrap(), Command::Pause));
        app.media_action(MediaAction::Pause, &tx);
        assert!(rx.try_recv().is_err());
        app.media_action(MediaAction::Play, &tx);
        assert!(matches!(rx.try_recv().unwrap(), Command::Resume));
        assert_eq!(app.query, "my search");
        assert_eq!(app.filter, "my filter");
        assert!(app.editing && app.filtering);
    }

    #[test]
    fn media_navigation_preserves_manual_repeat_and_previous_behavior() {
        let mut queue = Queue::default();
        queue.replace(vec![test_track(1).id, test_track(2).id], 0, false);
        let mut app = App::new(Config::default(), queue);
        app.config.repeat = crate::model::Repeat::Track;
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.media_action(MediaAction::Next, &tx);
        assert_eq!(app.queue.current(), Some(test_track(2).id.as_str()));
        assert!(matches!(rx.try_recv().unwrap(), Command::Load { .. }));
        app.queue.position_ms = 5000;
        app.media_action(MediaAction::Previous, &tx);
        assert_eq!(app.queue.current(), Some(test_track(2).id.as_str()));
        assert!(matches!(
            rx.try_recv().unwrap(),
            Command::Load { position_ms: 0, .. }
        ));
        app.media_action(MediaAction::Previous, &tx);
        assert_eq!(app.queue.current(), Some(test_track(1).id.as_str()));
        assert!(matches!(rx.try_recv().unwrap(), Command::Load { .. }));
        app.media_action(MediaAction::Next, &tx);
        rx.try_recv().unwrap();
        app.media_action(MediaAction::Next, &tx);
        assert!(matches!(rx.try_recv().unwrap(), Command::Stop));
        assert_eq!(app.state, State::Paused);
    }
    fn tasks() -> (Tasks, mpsc::UnboundedReceiver<Background>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Tasks::new(Catalog::mock("http://127.0.0.1:1"), tx).unwrap(),
            rx,
        )
    }
    fn test_track(i: usize) -> Track {
        Track {
            id: format!("{i:022}"),
            name: format!("Song {i}"),
            artists: "Artist".into(),
            duration_ms: 200000,
            playable: true,
            ..Default::default()
        }
    }
    #[tokio::test]
    async fn shift_j_reorders_and_page_down_only_navigates() {
        let (mut tasks, _) = tasks();
        let mut q = Queue::default();
        q.replace((0..20).map(|i| format!("{i:022}")).collect(), 0, false);
        let mut app = App::new(Config::default(), q);
        let (tx, _) = mpsc::unbounded_channel();
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT),
            &mut tasks,
            &tx,
        );
        assert_eq!(&app.queue.order[..3], &[1, 0, 2]);
        assert_eq!(app.queue.selected, 1);
        assert_eq!(app.queue.cursor, Some(1));
        key(
            &mut app,
            KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(&app.queue.order[..3], &[1, 0, 2]);
        assert_eq!(app.queue.selected, 16);
    }
    #[tokio::test]
    async fn cold_restore_lyrics_wait_for_real_metadata_and_explicit_open() {
        let (mut tasks, _) = tasks();
        let mut q = Queue::default();
        q.replace(vec![test_track(1).id], 0, false);
        let mut app = App::new(Config::default(), q);
        app.show_lyrics = true;
        tasks.update_lyrics(&mut app);
        assert!(!app.lyrics_loading);
        assert!(tasks.lyrics.is_none());
        app.show_lyrics = false;
        app.cache.insert(test_track(1).id, test_track(1));
        tasks.update_lyrics(&mut app);
        assert!(tasks.lyrics.is_none());
        app.show_lyrics = true;
        tasks.update_lyrics(&mut app);
        assert!(app.lyrics_loading);
        // Drop cancels before the mock-independent task gets a chance to access Lrclib.
        tasks.lyrics.take().unwrap().abort();
        let request = app.lyrics_request;
        background(
            &mut app,
            &mut tasks,
            Background::Lyrics(request, Err(anyhow::anyhow!("offline"))),
        );
        assert!(app.lyrics_error.as_ref().unwrap().contains("offline"));
        tasks.retry_metadata(&mut app);
        assert!(app.lyrics_error.is_none());
        app.queue.clear();
        tasks.update_lyrics(&mut app);
        assert!(app.lyrics_track_id.is_none());
        assert!(app.lyrics.is_none());
    }
    #[tokio::test]
    async fn stale_background_queue_and_lyrics_results_are_rejected() {
        let (mut tasks, _) = tasks();
        let mut app = App::new(Config::default(), Queue::default());
        app.queue.replace(vec![test_track(0).id], 0, false);
        let old = app.queue.epoch;
        app.queue.clear();
        background(
            &mut app,
            &mut tasks,
            Background::Recommendations(old, Ok(vec![test_track(1)])),
        );
        background(
            &mut app,
            &mut tasks,
            Background::PlaylistPage(old, 0, vec![test_track(1)], true),
        );
        assert!(app.queue.ids.is_empty());
        app.lyrics_request = 2;
        background(
            &mut app,
            &mut tasks,
            Background::Lyrics(1, Ok(Some(crate::lyrics::Lyrics::default()))),
        );
        assert!(app.lyrics.is_none());
        let epoch = app.queue.epoch;
        background(
            &mut app,
            &mut tasks,
            Background::Recommendations(epoch, Ok(vec![test_track(1), test_track(1)])),
        );
        assert_eq!(app.queue.ids.len(), 1);
    }
    #[tokio::test]
    async fn metadata_failure_is_visible_and_f5_allows_retry() {
        let (mut tasks, _) = tasks();
        let mut app = App::new(Config::default(), Queue::default());
        let id = test_track(1).id;
        app.queue.replace(vec![id.clone()], 0, false);
        tasks.requested.insert(id.clone());
        background(
            &mut app,
            &mut tasks,
            Background::Metadata(0, id.clone(), Err(anyhow::anyhow!("HTTP 503"))),
        );
        assert!(app.status.contains("503"));
        assert!(tasks.metadata_blocked);
        assert!(!tasks.requested.contains(&id));
        app.cache.insert(id.clone(), test_track(1));
        tasks.retry_metadata(&mut app);
        assert!(!tasks.metadata_blocked);
        assert!(app.metadata_error.is_none());
        assert!(app.cache.get(&id).is_none());
        background(
            &mut app,
            &mut tasks,
            Background::Metadata(0, id.clone(), Ok(test_track(1))),
        );
        assert!(app.cache.get(&id).is_none()); // Response predates F5.
    }
    #[tokio::test]
    async fn missing_track_does_not_block_other_metadata() {
        let (mut tasks, _) = tasks();
        let mut app = App::new(Config::default(), Queue::default());
        let id = test_track(1).id;
        background(
            &mut app,
            &mut tasks,
            Background::Metadata(0, id.clone(), Err(crate::catalog::MissingItem.into())),
        );
        assert!(!tasks.metadata_blocked);
        assert!(!app.cache.get(&id).unwrap().playable);
        background(
            &mut app,
            &mut tasks,
            Background::Metadata(0, test_track(2).id, Ok(test_track(2))),
        );
        assert!(app.cache.get(&test_track(2).id).is_some());
    }
    #[tokio::test]
    async fn playlist_job_stays_active_until_final_message_is_applied() {
        let (mut tasks, _) = tasks();
        let mut app = App::new(Config::default(), Queue::default());
        tasks.playlist = Some(tokio::spawn(async {}));
        tokio::task::yield_now().await;
        tasks.enqueue_playlist(&mut app, test_track(1).id, "Again".into());
        assert_eq!(tasks.playlist_request, 0);
        assert!(app.status.contains("already being added"));
        background(
            &mut app,
            &mut tasks,
            Background::PlaylistPage(0, 0, vec![test_track(1)], true),
        );
        assert!(tasks.playlist.is_none());
        assert_eq!(app.queue.ids.len(), 1);
    }
    #[tokio::test]
    async fn playlist_enqueue_follows_pages_counts_playable_tracks_and_reports_partial_failure() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{path, query_param},
        };
        let server = MockServer::start().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut tasks = Tasks::new(Catalog::mock(&server.uri()), tx).unwrap();
        let mut app = App::new(Config::default(), Queue::default());
        for (offset, next, status) in [
            (0, Some("next"), 200),
            (50, Some("next"), 200),
            (100, None, 503),
        ] {
            let mut template = ResponseTemplate::new(status);
            if status == 200 {
                template=template.set_body_json(serde_json::json!({"items":[{"item":{"id":format!("{:022}",offset),"name":"Song","type":"track","is_playable":true}},{"item":{"id":format!("{:022}",offset+1),"type":"track","is_playable":false}}],"next":next}));
            }
            Mock::given(path("/playlists/0000000000000000000001/items"))
                .and(query_param("offset", offset.to_string()))
                .respond_with(template)
                .expect(1)
                .mount(&server)
                .await;
        }
        tasks.enqueue_playlist(&mut app, "0000000000000000000001".into(), "Test".into());
        for _ in 0..3 {
            let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .unwrap()
                .unwrap();
            background(&mut app, &mut tasks, event);
        }
        assert_eq!(app.queue.ids.len(), 2);
        assert!(app.status.contains("after 2 additions"));
        assert!(app.status.contains("503"));
    }
    #[tokio::test]
    async fn undo_restores_removed_current_and_cleared_queue_paused_with_fresh_epoch() {
        let mut queue = Queue::default();
        queue.replace((0..5).map(|i| test_track(i).id).collect(), 2, true);
        queue.position_ms = 42_000;
        let original = queue.clone();
        let mut app = App::new(
            Config {
                shuffle: true,
                ..Config::default()
            },
            queue,
        );
        let (mut tasks, _) = tasks();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.queue.selected = app.queue.cursor.unwrap();
        key(
            &mut app,
            KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert!(app.queue.current().is_none());
        let removed_epoch = app.queue.epoch;
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.queue.ids, original.ids);
        assert_eq!(app.queue.order, original.order);
        assert_eq!(app.queue.current(), original.current());
        assert_eq!(app.queue.position_ms, 42_000);
        assert_eq!(app.state, State::Paused);
        assert!(!app.loaded);
        assert!(app.queue.epoch > removed_epoch);
        app.queue.validate().unwrap();
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT),
            &mut tasks,
            &tx,
        );
        assert!(app.queue.ids.is_empty());
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.queue.ids, original.ids);
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .all(|command| matches!(command, Command::Stop))
        );
    }

    #[tokio::test]
    async fn undo_queue_replacement_rejects_late_jobs_and_restores_previous_position() {
        let mut queue = Queue::default();
        queue.replace(vec![test_track(1).id], 0, false);
        queue.position_ms = 8000;
        let mut app = App::new(Config::default(), queue);
        let (mut tasks, _) = tasks();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        app.view = View::Search;
        app.rows = Rows::Tracks(vec![test_track(2)]);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        let replaced_epoch = app.queue.epoch;
        assert_eq!(app.queue.current(), Some(test_track(2).id.as_str()));
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        background(
            &mut app,
            &mut tasks,
            Background::Recommendations(replaced_epoch, Ok(vec![test_track(3)])),
        );
        assert_eq!(app.queue.ids, vec![test_track(1).id]);
        assert_eq!(app.queue.position_ms, 8000);
        assert_eq!(app.state, State::Paused);
    }

    #[test]
    fn undo_history_is_bounded_by_action_count_and_total_tracks() {
        let mut app = App::new(Config::default(), Queue::default());
        for _ in 0..20 {
            app.remember_queue();
        }
        assert_eq!(app.undo.len(), 10);
        app.queue
            .replace((0..50_000).map(|i| test_track(i).id).collect(), 0, false);
        for _ in 0..4 {
            app.remember_queue();
        }
        assert_eq!(
            app.undo
                .iter()
                .map(|entry| entry.queue.ids.len())
                .sum::<usize>(),
            100_000
        );
        assert_eq!(app.undo.len(), 2);
    }

    #[tokio::test]
    async fn library_progress_keeps_partial_matches_and_ignores_results_after_cancel_or_scope_change()
     {
        let mut app = App::new(Config::default(), Queue::default());
        let (mut tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();
        app.search_scope = SearchScope::Library;
        app.busy = true;
        app.request = 10;
        background(
            &mut app,
            &mut tasks,
            Background::LibraryProgress(
                10,
                crate::library::LibraryProgress {
                    tracks: vec![test_track(1)],
                    scanned: 150,
                    complete: false,
                },
            ),
        );
        assert_eq!(app.len(), 1);
        assert!(app.busy);
        assert_eq!(app.library_scanned, 150);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert!(!app.busy);
        assert!(!app.quit);
        assert!(app.title.contains("partial"));
        background(
            &mut app,
            &mut tasks,
            Background::LibraryProgress(
                10,
                crate::library::LibraryProgress {
                    tracks: vec![test_track(2)],
                    scanned: 200,
                    complete: true,
                },
            ),
        );
        assert_eq!(app.len(), 1);
        let request = app.request;
        background(
            &mut app,
            &mut tasks,
            Background::LibraryDone(request, Err(anyhow::anyhow!("rate limit"))),
        );
        assert_eq!(app.len(), 1);
        assert!(app.status.contains("partial matches retained"));
        app.query.clear();
        key(
            &mut app,
            KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.search_scope, SearchScope::Spotify);
        assert!(app.editing);
        assert!(!app.busy);
        background(
            &mut app,
            &mut tasks,
            Background::LibraryDone(request, Ok(())),
        );
        assert_eq!(app.title, "Spotify search");
    }

    fn draw_mouse(app: &App, width: u16, height: u16) {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    }
    fn click_target(
        app: &mut App,
        target: MouseTarget,
        button: MouseButton,
        tasks: &mut Tasks,
        tx: &mpsc::UnboundedSender<Command>,
    ) {
        let area = app
            .mouse_hits
            .borrow()
            .iter()
            .find(|(_, hit)| *hit == target)
            .unwrap()
            .0;
        assert!(mouse(
            app,
            MouseEvent {
                kind: MouseEventKind::Down(button),
                column: area.x,
                row: area.y,
                modifiers: KeyModifiers::NONE
            },
            tasks,
            tx
        ));
    }

    #[tokio::test]
    async fn right_click_filtered_row_opens_actions_without_pasting_and_enqueues_that_track() {
        let mut app = App::new(Config::default(), Queue::default());
        app.view = View::Liked;
        let mut first = test_track(1);
        first.name = "Other".into();
        let mut second = test_track(2);
        second.name = "Wanted".into();
        app.rows = Rows::Tracks(vec![first, second.clone()]);
        app.filter = "Wanted".into();
        app.filtering = true;
        let (mut tasks, _) = tasks();
        let (tx, mut rx) = mpsc::unbounded_channel();
        draw_mouse(&app, 120, 35);
        click_target(
            &mut app,
            MouseTarget::Catalog(0),
            MouseButton::Right,
            &mut tasks,
            &tx,
        );
        assert!(!app.filtering);
        assert_eq!(app.filter, "Wanted");
        assert!(app.context_menu.is_some());
        assert!(app.queue.ids.is_empty());
        draw_mouse(&app, 120, 35);
        click_target(
            &mut app,
            MouseTarget::Menu(1),
            MouseButton::Left,
            &mut tasks,
            &tx,
        );
        assert_eq!(app.queue.ids, vec![second.id]);
        assert!(app.context_menu.is_none());
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn mouse_maps_scrolled_queue_rows_and_rejects_stale_menu_actions() {
        let mut app = App::new(Config::default(), Queue::default());
        for i in 0..70 {
            app.queue.enqueue(test_track(i).id);
        }
        app.view = View::Queue;
        app.queue.selected = 55;
        let (mut tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();
        draw_mouse(&app, 80, 24);
        assert!(app.queue_scroll.get() > 0);
        click_target(
            &mut app,
            MouseTarget::Queue(55),
            MouseButton::Right,
            &mut tasks,
            &tx,
        );
        app.queue.enqueue(test_track(80).id);
        activate_menu(&mut app, 3, &mut tasks, &tx);
        assert_eq!(app.queue.ids.len(), 71);
        assert!(app.status.contains("List changed"));
        draw_mouse(&app, 80, 24);
        click_target(
            &mut app,
            MouseTarget::Queue(55),
            MouseButton::Right,
            &mut tasks,
            &tx,
        );
        activate_menu(&mut app, 3, &mut tasks, &tx);
        assert_eq!(app.queue.ids.len(), 70);
        assert!(!app.queue.ids.contains(&test_track(55).id));
    }

    #[tokio::test]
    async fn mouse_queue_sidebar_selection_and_small_terminal_menu_stay_in_bounds() {
        let mut app = App::new(Config::default(), Queue::default());
        for i in 0..5 {
            app.queue.enqueue(test_track(i).id);
        }
        let (mut tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();
        draw_mouse(&app, 120, 35);
        click_target(
            &mut app,
            MouseTarget::Queue(3),
            MouseButton::Right,
            &mut tasks,
            &tx,
        );
        assert_eq!(app.view, View::Queue);
        assert_eq!(app.queue.selected, 3);
        let menu = app.context_menu.as_mut().unwrap();
        menu.x = 119;
        menu.y = 34;
        for (width, height) in [(120, 35), (80, 24), (32, 10)] {
            draw_mouse(&app, width, height);
            let hits = app.mouse_hits.borrow();
            assert_eq!(hits.len(), 4);
            assert!(
                hits.iter()
                    .all(|(rect, _)| rect.right() <= width && rect.bottom() <= height)
            );
        }
        key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert!(app.context_menu.is_none());
        assert!(!app.quit);
    }

    #[tokio::test]
    async fn mouse_wheel_and_playback_badge_use_existing_controls() {
        let mut app = App::new(Config::default(), Queue::default());
        app.rows = Rows::Tracks((0..30).map(test_track).collect());
        let (mut tasks, _) = tasks();
        let (tx, mut rx) = mpsc::unbounded_channel();
        draw_mouse(&app, 80, 24);
        let row = app
            .mouse_hits
            .borrow()
            .iter()
            .find(|(_, target)| *target == MouseTarget::Catalog(0))
            .unwrap()
            .0;
        assert!(mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: row.x,
                row: row.y,
                modifiers: KeyModifiers::NONE
            },
            &mut tasks,
            &tx
        ));
        assert_eq!(app.selected, 3);
        app.state = State::Playing;
        app.loaded = true;
        draw_mouse(&app, 80, 24);
        click_target(
            &mut app,
            MouseTarget::PlayPause,
            MouseButton::Left,
            &mut tasks,
            &tx,
        );
        assert_eq!(app.state, State::Paused);
        assert!(matches!(rx.try_recv(), Ok(Command::Pause)));
        assert!(!mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Moved,
                column: row.x,
                row: row.y,
                modifiers: KeyModifiers::NONE
            },
            &mut tasks,
            &tx
        ));
    }

    #[test]
    fn checkpoint_sends_only_changed_components() {
        let mut app = App::new(Config::default(), Queue::default());
        let (config_tx, config_rx) = watch::channel(None);
        let (queue_tx, mut queue_rx) = watch::channel(None);
        let (cache_tx, cache_rx) = watch::channel(None);
        let (stats_tx, stats_rx) = watch::channel(None);
        let mut checkpoints = Checkpoints {
            config: app.config.clone(),
            queue: queue_stamp(&app.queue),
            cache: app.cache.revision,
            stats: app.stats.revision,
            retry: false,
            config_tx,
            queue_tx,
            cache_tx,
            stats_tx,
        };
        app.query = "typing".into();
        app.selected = 5;
        checkpoints.send(&app);
        assert!(!config_rx.has_changed().unwrap());
        assert!(!queue_rx.has_changed().unwrap());
        assert!(!cache_rx.has_changed().unwrap());
        assert!(!stats_rx.has_changed().unwrap());
        app.queue.enqueue(test_track(1).id);
        checkpoints.send(&app);
        assert!(queue_rx.has_changed().unwrap());
        queue_rx.borrow_and_update();
        app.config.volume = 31;
        checkpoints.send(&app);
        assert!(config_rx.has_changed().unwrap());
        assert!(!queue_rx.has_changed().unwrap());
        assert!(!cache_rx.has_changed().unwrap());
        assert!(!stats_rx.has_changed().unwrap());
        app.cache.insert(test_track(1).id, test_track(1));
        checkpoints.send(&app);
        assert!(cache_rx.has_changed().unwrap());
        assert!(!stats_rx.has_changed().unwrap());
        app.stats
            .add_listened_ms(&"0".repeat(22), 1000, "Song", "Artist");
        checkpoints.send(&app);
        assert!(stats_rx.has_changed().unwrap());
    }
    #[tokio::test]
    async fn writer_failure_is_reported_and_later_snapshot_can_recover() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (tx, rx) = watch::channel(None);
        let (bg, mut errors) = mpsc::unbounded_channel();
        let (done, mut completed) = mpsc::unbounded_channel();
        let writer = writer(rx, bg, move |value: u8| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                anyhow::bail!("disk unavailable");
            }
            done.send(value).unwrap();
            Ok(())
        });
        tx.send_replace(Some(1));
        assert!(matches!(
            errors.recv().await.unwrap(),
            Background::SaveError(_)
        ));
        tx.send_replace(Some(2));
        assert_eq!(completed.recv().await, Some(2));
        drop(tx);
        writer.await.unwrap().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[tokio::test]
    async fn writer_flushes_pending_final_value_and_returns_final_error() {
        let (tx, rx) = watch::channel(None);
        let (bg, _) = mpsc::unbounded_channel();
        let saved = Arc::new(std::sync::Mutex::new(Vec::new()));
        let output = saved.clone();
        let task = writer(rx, bg, move |value: u8| {
            output.lock().unwrap().push(value);
            Ok(())
        });
        tx.send_replace(Some(9));
        drop(tx);
        task.await.unwrap().unwrap();
        assert_eq!(*saved.lock().unwrap(), vec![9]);
        let (tx, rx) = watch::channel(None);
        let (bg, _) = mpsc::unbounded_channel();
        let task = writer(rx, bg, |_: u8| anyhow::bail!("final disk failure"));
        tx.send_replace(Some(1));
        drop(tx);
        assert!(
            task.await
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("final disk failure")
        );
    }
    #[test]
    fn filter_cache_reuses_indices_until_rows_or_query_change() {
        let mut app = App::new(Config::default(), Queue::default());
        app.view = View::Liked;
        app.rows = Rows::Tracks(vec![test_track(1)]);
        app.filter = "artist".into();
        let first = app.filtered_indices();
        assert!(Arc::ptr_eq(&first, &app.filtered_indices()));
        app.filter = "missing".into();
        assert!(app.filtered_indices().is_empty());
        app.rows = Rows::Tracks(vec![Track {
            name: "Missing".into(),
            ..test_track(2)
        }]);
        app.rows_revision += 1;
        assert_eq!(*app.filtered_indices(), vec![0]);
    }
    #[test]
    fn animation_is_suspended_when_paused_or_too_small_and_slower_without_bars() {
        let mut app = App::new(Config::default(), Queue::default());
        assert_eq!(app.animation_interval(), None);
        app.state = State::Loading;
        assert_eq!(app.animation_interval(), None);
        app.state = State::Playing;
        assert_eq!(app.animation_interval(), Some(Duration::from_millis(33)));
        app.terminal_size.set((40, 20));
        assert_eq!(app.animation_interval(), Some(Duration::from_millis(250)));
        app.show_visualizer = true;
        assert_eq!(app.animation_interval(), Some(Duration::from_millis(33)));
        app.terminal_size.set((20, 6));
        assert_eq!(app.animation_interval(), None);
    }
    #[test]
    fn playback_position_uses_elapsed_time_and_paused_position_is_stable() {
        let mut app = App::new(Config::default(), Queue::default());
        app.position_anchor = Some((Instant::now() - Duration::from_millis(1200), 500));
        app.state = State::Playing;
        app.interpolate_position();
        assert!(app.queue.position_ms >= 1700);
        app.state = State::Paused;
        let paused = app.queue.position_ms;
        app.interpolate_position();
        assert_eq!(paused, app.queue.position_ms);
    }
    #[test]
    fn restored_queue_starts_paused_and_stale_completion_is_ignored() {
        let mut q = Queue::default();
        q.replace(vec!["0".repeat(22), "1".repeat(22)], 0, false);
        q.position_ms = 12345;
        let mut app = App::new(Config::default(), q);
        let (tx, mut rx) = mpsc::unbounded_channel();
        assert_eq!(app.state, State::Paused);
        assert!(!app.loaded);
        assert_eq!(app.queue.position_ms, 12345);
        app.generation = 4;
        app.playback_event(
            Event::TrackError {
                generation: 3,
                message: "stale failure".into(),
            },
            &tx,
        );
        assert_eq!(app.state, State::Paused);
        app.playback_event(Event::Completed(3), &tx);
        assert_eq!(app.queue.cursor, Some(0));
        assert!(rx.try_recv().is_err());
        app.playback_event(Event::Completed(4), &tx);
        assert_eq!(app.queue.cursor, Some(1));
        assert!(matches!(rx.try_recv().unwrap(), Command::Load { .. }));
    }
    #[test]
    fn unavailable_stops_without_skipping_loop() {
        let mut q = Queue::default();
        q.replace(vec!["0".repeat(22), "1".repeat(22)], 0, false);
        let mut app = App::new(Config::default(), q);
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.playback_event(Event::Error("unavailable".into()), &tx);
        assert_eq!(app.queue.cursor, Some(0));
        assert_eq!(app.state, State::Failed);
        assert!(matches!(rx.try_recv().unwrap(), Command::Stop));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn filter_tracks_and_playlists() {
        let mut app = App::new(Config::default(), Queue::default());
        app.view = View::Liked;
        app.rows = Rows::Tracks(vec![
            Track {
                id: "1".into(),
                name: "Bohemian Rhapsody".into(),
                artists: "Queen".into(),
                duration_ms: 354000,
                playable: true,
                ..Default::default()
            },
            Track {
                id: "2".into(),
                name: "Yellow".into(),
                artists: "Coldplay".into(),
                duration_ms: 269000,
                playable: true,
                ..Default::default()
            },
            Track {
                id: "3".into(),
                name: "Under Pressure".into(),
                artists: "Queen, David Bowie".into(),
                duration_ms: 248000,
                playable: true,
                ..Default::default()
            },
        ]);

        assert_eq!(app.len(), 3);
        assert!(!app.is_filtered());

        // Filter by artist
        app.filter = "queen".into();
        assert!(app.is_filtered());
        assert_eq!(app.len(), 2);
        assert_eq!(*app.filtered_indices(), vec![0, 2]);

        // Filter by title
        app.filter = "yellow".into();
        assert_eq!(app.len(), 1);
        assert_eq!(*app.filtered_indices(), vec![1]);

        // Filter by multiple terms across title and artist
        app.filter = "bowie pressure".into();
        assert_eq!(app.len(), 1);
        assert_eq!(*app.filtered_indices(), vec![2]);

        // Non-matching filter
        app.filter = "nonexistent".into();
        assert_eq!(app.len(), 0);
        assert_eq!(*app.filtered_indices(), Vec::<usize>::new());

        // Empty filter
        app.filter.clear();
        assert!(!app.is_filtered());
        assert_eq!(app.len(), 3);

        // Test Playlists filtering
        app.view = View::Playlists;
        app.rows = Rows::Playlists(vec![
            crate::model::Playlist {
                id: "p1".into(),
                name: "Rock Classics".into(),
                owner: "Spotify".into(),
            },
            crate::model::Playlist {
                id: "p2".into(),
                name: "Chill Lofi Beats".into(),
                owner: "ChilledCow".into(),
            },
        ]);

        app.filter = "chilled".into();
        assert!(app.is_filtered());
        assert_eq!(app.len(), 1);
        assert_eq!(*app.filtered_indices(), vec![1]);

        app.filter = "classics".into();
        assert_eq!(app.len(), 1);
        assert_eq!(*app.filtered_indices(), vec![0]);
    }
    #[test]
    fn window_title_formats() {
        let mut app = App::new(Config::default(), Queue::default());
        assert_eq!(app.window_title(), "Tuitify");

        let track = Track {
            id: "t1".into(),
            name: "牵丝戏".into(),
            artists: "银临, Aki阿杰".into(),
            duration_ms: 239000,
            playable: true,
            ..Default::default()
        };
        app.cache.insert("t1".into(), track);
        app.queue.replace(vec!["t1".into()], 0, false);

        app.state = State::Playing;
        assert_eq!(app.window_title(), "Tuitify • 牵丝戏 - 银临, Aki阿杰");

        app.state = State::Paused;
        assert_eq!(app.window_title(), "|| Tuitify • 牵丝戏 - 银临, Aki阿杰");

        app.state = State::Loading;
        assert_eq!(app.window_title(), "... Tuitify • 牵丝戏 - 银临, Aki阿杰");

        app.state = State::Failed;
        assert_eq!(app.window_title(), "! Tuitify • 牵丝戏 - 银临, Aki阿杰");
    }

    #[tokio::test]
    async fn stats_overlay_toggle_mutual_exclusion_and_esc() {
        let mut app = App::new(Config::default(), Queue::default());
        let (mut tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();

        assert!(!app.show_stats);
        assert!(!app.show_lyrics);
        assert!(!app.show_visualizer);

        // Start with lyrics open
        app.show_lyrics = true;

        // Press 'S' to open stats: closes lyrics
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
            &mut tasks,
            &tx,
        );
        assert!(app.show_stats);
        assert!(!app.show_lyrics);
        assert!(!app.show_visualizer);
        assert!(app.status.contains("Song statistics"));

        // While stats is open, 'l' and 'v' are ignored (overlay is isolated)
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert!(app.show_stats);
        assert!(!app.show_lyrics);

        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert!(app.show_stats);
        assert!(!app.show_visualizer);

        // Press 'S' to dismiss
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
            &mut tasks,
            &tx,
        );
        assert!(!app.show_stats);
        assert!(app.status.contains("Exited"));

        // Start with visualizer open
        app.show_visualizer = true;

        // Press 'S': closes visualizer, opens stats
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
            &mut tasks,
            &tx,
        );
        assert!(app.show_stats);
        assert!(!app.show_visualizer);

        // Dismiss via Esc
        key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert!(!app.show_stats);
        assert!(!app.quit);
        assert!(app.status.contains("Exited"));
    }

    #[tokio::test]
    async fn stats_overlay_navigation_and_enter_safety() {
        let mut app = App::new(Config::default(), Queue::default());
        let (mut tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();

        // Populate 3 tracks in stats
        let id1 = "1".repeat(22);
        let id2 = "2".repeat(22);
        let id3 = "3".repeat(22);
        app.stats.add_play(&id1, "Track 1", "Artist 1");
        app.stats.add_play(&id2, "Track 2", "Artist 2");
        app.stats.add_play(&id3, "Track 3", "Artist 3");

        // Open stats overlay
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
            &mut tasks,
            &tx,
        );
        assert!(app.show_stats);
        assert_eq!(app.selected, 0);

        // Down arrow navigates within stats bounds
        key(
            &mut app,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.selected, 1);

        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.selected, 2);

        // Down at end clamps to len - 1 (index 2)
        key(
            &mut app,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.selected, 2);

        // Enter is suppressed while stats overlay is open
        key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert!(app.show_stats);
        assert!(app.queue.ids.is_empty());
    }

    #[tokio::test]
    async fn stats_save_error_updates_status_and_triggers_retry() {
        let mut app = App::new(Config::default(), Queue::default());
        let (mut tasks, _) = tasks();
        let retry = background(
            &mut app,
            &mut tasks,
            Background::SaveError("Could not save state: stats write error".into()),
        );
        assert!(retry);
        assert!(app.status.contains("stats write error"));
    }

    #[test]
    fn regression_continuous_accounting_after_queue_mutation() {
        let mut app = App::new(Config::default(), Queue::default());
        let id = "0".repeat(22);
        app.queue.replace(vec![id.clone()], 0, false);
        let start = Instant::now();
        app.generation = 1;
        app.state = State::Playing;
        app.accounting
            .start_generation(1, Some(id.clone()), 200_000);
        app.accounting
            .on_playing(1, Some(id.clone()), 200_000, start);

        // Account 5 seconds
        app.account_playback_time(start + Duration::from_secs(5));
        assert_eq!(app.stats.tracks.get(&id).unwrap().listened_ms, 5_000);

        // Queue mutation calls remember_queue
        app.remember_queue();
        // Accounting continuity is preserved: last_accounted_at must not be None
        assert!(app.accounting.last_accounted_at.is_some());

        // Play for another 5 seconds
        app.account_playback_time(start + Duration::from_secs(10));
        assert_eq!(app.stats.tracks.get(&id).unwrap().listened_ms, 10_000);
    }

    #[tokio::test]
    async fn regression_switching_queued_tracks_finalizes_generation_pinned_old_track() {
        let mut app = App::new(Config::default(), Queue::default());
        let id1 = "1".repeat(22);
        let id2 = "2".repeat(22);
        app.queue.replace(vec![id1.clone(), id2.clone()], 0, false);
        let start = Instant::now();
        app.generation = 1;
        app.state = State::Playing;
        app.accounting
            .start_generation(1, Some(id1.clone()), 200_000);
        app.accounting
            .on_playing(1, Some(id1.clone()), 200_000, start);

        // Advance 10 seconds of playback time in 5s chunks
        for i in 1..=2 {
            app.account_playback_time(start + Duration::from_secs(i * 5));
        }
        assert_eq!(app.stats.tracks.get(&id1).unwrap().listened_ms, 10_000);

        // User selects track 2 in queue
        app.queue.select(1);
        assert_eq!(app.queue.current(), Some(id2.as_str()));

        // Call load for new track: it should finalize track 1, not track 2!
        let (tx, _rx) = mpsc::unbounded_channel();
        app.load(&tx);

        assert_eq!(app.stats.tracks.get(&id1).unwrap().listened_ms, 10_000);
        assert!(!app.stats.tracks.contains_key(&id2));
        assert_eq!(app.accounting.track_id, Some(id2.clone()));
    }

    #[tokio::test]
    async fn regression_stats_overlay_from_view_queue_navigates_selected_without_changing_queue_selected()
     {
        let mut app = App::new(Config::default(), Queue::default());
        let (mut tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();

        for i in 0..5 {
            app.queue.enqueue(test_track(i).id);
        }
        app.view = View::Queue;
        app.queue.selected = 3;

        let id1 = "1".repeat(22);
        let id2 = "2".repeat(22);
        app.stats.add_play(&id1, "Track 1", "Artist 1");
        app.stats.add_play(&id2, "Track 2", "Artist 2");

        // Open stats overlay
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
            &mut tasks,
            &tx,
        );
        assert!(app.show_stats);
        assert_eq!(app.selected, 0);
        assert_eq!(app.queue.selected, 3);

        // Press Down / j
        key(
            &mut app,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.selected, 1);
        assert_eq!(app.queue.selected, 3); // queue.selected unchanged!

        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.selected, 0);
        assert_eq!(app.queue.selected, 3); // queue.selected unchanged!
    }

    #[tokio::test]
    async fn regression_destructive_and_settings_keys_ignored_in_stats_overlay() {
        let mut app = App::new(Config::default(), Queue::default());
        let (mut tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();

        for i in 0..5 {
            app.queue.enqueue(test_track(i).id);
        }
        app.view = View::Queue;
        let initial_queue_len = app.queue.ids.len();
        let initial_shuffle = app.config.shuffle;
        let initial_repeat = app.config.repeat;
        let initial_theme = app.config.theme.clone();

        app.show_stats = true;

        // Press 's' (toggle shuffle) - must be ignored!
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.config.shuffle, initial_shuffle);

        // Press 'd' / 'x' / Delete (remove from queue) - must be ignored!
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.queue.ids.len(), initial_queue_len);

        // Press 'C' (clear queue) - must be ignored!
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.queue.ids.len(), initial_queue_len);

        // Press 'r' (cycle repeat) - must be ignored!
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.config.repeat, initial_repeat);

        // Press 't' (cycle theme) - must be ignored!
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.config.theme, initial_theme);

        // Press 'a' / 'A' - must be ignored!
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.queue.ids.len(), initial_queue_len);

        // Press 'J' / 'K' - must be ignored!
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT),
            &mut tasks,
            &tx,
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT),
            &mut tasks,
            &tx,
        );

        // Press '1'..='5' - must not change view!
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.view, View::Queue);
    }

    #[tokio::test]
    async fn regression_catalog_scroll_preserved_across_stats_overlay() {
        let mut app = App::new(Config::default(), Queue::default());
        let (mut tasks, _) = tasks();
        let (tx, _) = mpsc::unbounded_channel();

        app.catalog_scroll.set(17);
        assert_eq!(app.catalog_scroll.get(), 17);

        // Open stats overlay
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
            &mut tasks,
            &tx,
        );
        assert!(app.show_stats);
        assert_eq!(app.catalog_scroll.get(), 17); // catalog_scroll preserved!
        assert_eq!(app.stats_scroll.get(), 0);

        // Navigate in stats
        key(
            &mut app,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.catalog_scroll.get(), 17);

        // Close stats
        key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut tasks,
            &tx,
        );
        assert!(!app.show_stats);
        assert_eq!(app.catalog_scroll.get(), 17); // catalog_scroll preserved!
    }
}
