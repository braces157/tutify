mod actions;
mod browsing;
mod controls;
mod input;
mod jobs;
mod lyrics_state;
mod mouse;
mod persistence;
mod runtime;
mod smart_shuffle;
pub(crate) mod ui_state;

use actions::Action;
use browsing::BrowseState;
use controls::{Control, Seek};
use input::key;
use jobs::*;
use lyrics_state::LyricsState;
use mouse::*;
use persistence::*;
pub use runtime::run;
pub use ui_state::{Overlay, RenderState, UiState};

use crate::{
    auth::TokenManager,
    catalog::{Browse, Catalog, Page, Recommendations, Rows},
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

pub use crate::model::PlaybackState as State;
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
    pub selected: usize,
    filter: String,
    pub x: u16,
    pub y: u16,
    actions: Vec<(&'static str, Action)>,
    view: View,
    row: usize,
    revision: u64,
}

impl ContextMenu {
    pub fn labels(&self) -> impl ExactSizeIterator<Item = &'static str> + '_ {
        self.actions.iter().map(|(label, _)| *label)
    }
}

struct QueueUndo {
    queue: Queue,
    shuffle: bool,
}

pub struct App {
    undo: VecDeque<QueueUndo>,

    pub context_menu: Option<ContextMenu>,
    pub config: Config,
    pub queue: Queue,
    pub catalog: BrowseState,
    pub lyrics: LyricsState,
    pub status: String,
    pub state: State,
    pub cache: crate::cache::MetadataCache,
    pub generation: u64,
    pub loaded: bool,
    /// Volume saved before the last mute action. This is session-only so a
    /// restart still uses the user's persisted volume setting.
    muted_volume: Option<u8>,
    pub quit: bool,
    pub metadata_error: Option<String>,
    pub catalog_health: crate::catalog::Health,
    position_anchor: Option<(Instant, u32)>,
    pub animation_frame: u32,
    pub visualizer: Arc<crate::visualizer::AudioVisualizer>,
    pub stats: SongStats,
    pub ui: UiState,
    pub accounting: PlaybackAccounting,
    pub radio_epoch: Option<u64>,
    pub radio_source: Option<crate::catalog::RecommendationSource>,
    radio_suggestions: HashSet<String>,
}

impl App {
    pub fn new(mut config: Config, queue: Queue) -> Self {
        if queue.smart_shuffle {
            config.shuffle = true;
        }
        let restored = !queue.ids.is_empty();
        Self {
            undo: VecDeque::new(),
            context_menu: None,
            config,
            queue,
            catalog: BrowseState::new(restored),
            lyrics: LyricsState::default(),
            status: "Paused. / search | 1-5 views | Tab navigation | ? help | q quit".into(),
            state: State::Paused,
            cache: crate::cache::MetadataCache::default(),
            generation: 0,
            loaded: false,
            muted_volume: None,
            quit: false,
            metadata_error: None,
            catalog_health: crate::catalog::Health::Unknown,
            position_anchor: None,
            animation_frame: 0,
            visualizer: crate::visualizer::AudioVisualizer::new(),
            stats: SongStats::default(),
            ui: UiState::default(),
            accounting: PlaybackAccounting::new(),
            radio_epoch: None,
            radio_source: None,
            radio_suggestions: HashSet::new(),
        }
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    fn enqueue_manual(&mut self, id: String) -> bool {
        let before = if self.radio_epoch == Some(self.queue.epoch) {
            self.queue.cursor.and_then(|cursor| {
                self.queue
                    .order
                    .iter()
                    .enumerate()
                    .skip(cursor + 1)
                    .find(|(_, index)| self.radio_suggestions.contains(&self.queue.ids[**index]))
                    .map(|(position, _)| position)
            })
        } else {
            None
        };
        if !self.queue.enqueue(id.clone()) {
            return false;
        }
        self.radio_suggestions.remove(&id);
        if let Some(before) = before {
            let selected = self.queue.selected;
            self.queue.move_item(self.queue.order.len() - 1, before);
            self.queue.selected = selected + usize::from(selected >= before);
        }
        true
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
        self.catalog.view = View::Queue;
        self.catalog.nav = View::Queue.index();
        self.catalog.sidebar = false;
        self.catalog.editing = false;
        self.catalog.filtering = false;
        self.catalog.filter.clear();
        self.context_menu = None;
        self.ui.close(Overlay::Lyrics);
        self.ui.close(Overlay::Visualizer);
        self.status =
            "Queue restored, paused at its saved position. Space resumes; u undoes another change."
                .into();
    }
    pub fn is_filtered(&self) -> bool {
        self.catalog.is_filtered()
    }
    pub fn raw_len(&self) -> usize {
        self.catalog.raw_len()
    }
    pub fn filtered_indices(&self) -> Arc<Vec<usize>> {
        self.catalog.filtered_indices()
    }
    fn reset_rows(&mut self) {
        self.catalog.reset_rows();
        self.ui.render.borrow_mut().catalog_scroll = 0;
    }
    fn animation_interval(&self) -> Option<Duration> {
        let (width, height) = self.ui.render.borrow().terminal_size;
        if self.state != State::Playing || width < 32 || height < 10 {
            return None;
        }
        Some(Duration::from_millis(
            if self.ui.overlay == Overlay::Visualizer || width >= 50 {
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
        if self.ui.overlay == Overlay::Stats {
            let mut view = self.ui.stats.borrow_mut();
            view.refresh(&self.stats);
            view.rows.len()
        } else if self.catalog.view == View::Help {
            self.ui.render.borrow().help_length
        } else if self.catalog.view == View::Queue {
            self.queue.order.len()
        } else if self.is_filtered() {
            self.filtered_indices().len()
        } else {
            self.raw_len()
        }
    }
    #[allow(dead_code)]
    pub fn selection(&self) -> usize {
        if self.catalog.view == View::Queue {
            self.queue.selected
        } else {
            self.catalog.selected
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
        if self.catalog.view == View::Queue {
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
        } else if let Rows::Tracks(t) = &self.catalog.rows {
            let actual_idx = if self.is_filtered() {
                *self.filtered_indices().get(self.catalog.selected)?
            } else {
                self.catalog.selected
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

#[cfg(test)]
mod tests;
