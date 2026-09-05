use crate::{
    auth::TokenManager,
    catalog::{Browse, Catalog, Page, Rows},
    model::Track,
    playback::{self, Command, Event},
    queue::Queue,
    storage::{Config, Storage},
    ui,
};
use anyhow::Result;
use crossterm::event::{
    Event as Input, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use futures_util::StreamExt;
use std::collections::{HashMap, HashSet};
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
pub enum State {
    Paused,
    Loading,
    Playing,
    Failed,
}
pub struct App {
    pub config: Config,
    pub queue: Queue,
    pub view: View,
    pub sidebar: bool,
    pub nav: usize,
    pub rows: Rows,
    pub selected: usize,
    pub query: String,
    pub editing: bool,
    pub status: String,
    pub busy: bool,
    pub state: State,
    pub cache: HashMap<String, Track>,
    pub title: String,
    pub next: Option<usize>,
    pub browse: Browse,
    pub generation: u64,
    pub loaded: bool,
    /// Volume saved before the last mute action. This is session-only so a
    /// restart still uses the user's persisted volume setting.
    muted_volume: Option<u8>,
    request: u64,
    pub quit: bool,
}

impl App {
    pub fn new(config: Config, queue: Queue) -> Self {
        let restored = !queue.ids.is_empty();
        Self {
            config,
            queue,
            view: if restored { View::Queue } else { View::Search },
            sidebar: false,
            nav: if restored { 3 } else { 0 },
            rows: Rows::Tracks(vec![]),
            selected: 0,
            query: String::new(),
            editing: false,
            status: "Paused. / search | 1-5 views | Tab navigation | ? help | q quit".into(),
            busy: false,
            state: State::Paused,
            cache: HashMap::new(),
            title: "Search".into(),
            next: None,
            browse: Browse::Search(String::new()),
            generation: 0,
            loaded: false,
            muted_volume: None,
            request: 0,
            quit: false,
        }
    }
    pub fn len(&self) -> usize {
        if self.view == View::Help {
            33
        } else if self.view == View::Queue {
            self.queue.order.len()
        } else {
            match &self.rows {
                Rows::Tracks(t) => t.len(),
                Rows::Playlists(p) => p.len(),
            }
        }
    }
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
            t.get(self.selected).cloned()
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
            self.generation += 1;
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
        self.generation += 1;
        self.state = State::Paused;
        self.loaded = false;
        self.send(tx, Command::Stop);
    }
    pub fn playback_event(&mut self, event: Event, tx: &mpsc::UnboundedSender<Command>) {
        match event {
            Event::Playing {
                generation,
                position_ms,
            } if generation == self.generation => {
                self.state = State::Playing;
                self.queue.position_ms = position_ms;
                self.status =
                    "Playing | Space pause | Left/Right seek | +/- volume | n/p next/previous"
                        .into();
            }
            Event::Paused {
                generation,
                position_ms,
            } if generation == self.generation => {
                self.state = State::Paused;
                self.queue.position_ms = position_ms;
                self.status = "Paused | Space resumes | Left/Right seek | +/- volume".into();
            }
            Event::Position {
                generation,
                position_ms,
            } if generation == self.generation => self.queue.position_ms = position_ms,
            Event::Completed(generation) if generation == self.generation => {
                if self.queue.advance(self.config.repeat, true) {
                    self.load(tx);
                } else {
                    self.stop(tx);
                    self.queue.position_ms = 0;
                    self.status = "Queue finished. Space replays the current track.".into();
                }
            }
            Event::Error(message) => {
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
    Page(u64, Result<Page>),
    Metadata(Track),
    MetadataError(String),
    SaveError(String),
}
struct Tasks {
    catalog: Catalog,
    tx: mpsc::UnboundedSender<Background>,
    browse: Option<tokio::task::JoinHandle<()>>,
    metadata: Option<tokio::task::JoinHandle<()>>,
    requested: HashSet<String>,
}
impl Drop for Tasks {
    fn drop(&mut self) {
        if let Some(t) = &self.browse {
            t.abort();
        }
        if let Some(t) = &self.metadata {
            t.abort();
        }
    }
}
impl Tasks {
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
        self.browse = Some(tokio::spawn(async move {
            let result = catalog.page(&browse, offset).await;
            let _ = tx.send(Background::Page(request, result));
        }));
    }
    fn metadata(&mut self, app: &App) {
        if self.metadata.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        let mut ids = vec![];
        if let Some(id) = app.queue.current() {
            ids.push(id.to_owned());
        }
        let start = if app.view == View::Queue {
            app.queue.selected.saturating_sub(4)
        } else {
            app.queue.cursor.unwrap_or(0)
        };
        ids.extend(
            app.queue
                .order
                .iter()
                .skip(start)
                .take(16)
                .map(|i| app.queue.ids[*i].clone()),
        );
        ids.retain(|id| !app.cache.contains_key(id) && self.requested.insert(id.clone()));
        if ids.is_empty() {
            return;
        }
        let catalog = self.catalog.clone();
        let tx = self.tx.clone();
        self.metadata = Some(tokio::spawn(async move {
            for id in ids {
                match catalog.track(&id).await {
                    Ok(track) => {
                        let _ = tx.send(Background::Metadata(track));
                    }
                    Err(e) => {
                        let _ = tx.send(Background::MetadataError(format!(
                            "Queue names could not load: {e:#}"
                        )));
                        break;
                    }
                }
            }
        }));
    }
    fn view(&mut self, app: &mut App, view: View) {
        app.view = view;
        app.nav = view.index();
        app.sidebar = false;
        app.selected = 0;
        app.editing = false;
        app.request += 1;
        app.busy = false;
        if let Some(task) = self.browse.take() {
            task.abort();
        }
        match view {
            View::Search => {
                app.browse = Browse::Search(app.query.clone());
                app.title = "Search".into();
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
        app.rows = Rows::Tracks(vec![]);
        app.next = None;
        self.request(app, 0);
    }
}

fn format_time(ms: u32) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1_000 % 60)
}

fn key(app: &mut App, key: KeyEvent, tasks: &mut Tasks, tx: &mpsc::UnboundedSender<Command>) {
    if key.kind == KeyEventKind::Release {
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.quit = true;
        return;
    }
    if app.editing {
        match key.code {
            KeyCode::Esc => app.editing = false,
            KeyCode::Enter => {
                app.editing = false;
                app.browse = Browse::Search(app.query.trim().into());
                app.selected = 0;
                app.rows = Rows::Tracks(vec![]);
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
    match key.code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Esc if app.view != View::Help => app.quit = true,
        KeyCode::Esc => tasks.view(app, View::Search),
        KeyCode::Tab | KeyCode::BackTab => {
            app.sidebar = !app.sidebar;
            app.nav = app.view.index();
        }
        KeyCode::Char('?') | KeyCode::F(1) => tasks.view(app, View::Help),
        KeyCode::Char(c @ '1'..='5') => tasks.view(app, View::ALL[c as usize - '1' as usize]),
        KeyCode::Char('/') => {
            tasks.view(app, View::Search);
            app.editing = true;
            app.query.clear();
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
            } else {
                let at = (app.selection() + 1).min(app.len().saturating_sub(1));
                if app.view == View::Queue {
                    app.queue.selected = at;
                } else {
                    app.selected = at;
                }
            }
        }
        KeyCode::PageDown if !app.busy && !matches!(app.view, View::Queue | View::Help) => {
            if let Some(offset) = app.next {
                tasks.request(app, offset);
            }
        }
        KeyCode::F(5) => {
            tasks.requested.clear();
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
                    if let Some(p) = rows.get(app.selected).cloned() {
                        app.browse = Browse::Playlist(p.id);
                        app.title = p.name;
                        app.rows = Rows::Tracks(vec![]);
                        app.selected = 0;
                        tasks.request(app, 0);
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
                    // Preserve the selected occurrence even when duplicate IDs are present.
                    let index = tracks[..app.selected].iter().filter(|t| t.playable).count();
                    app.queue.replace(
                        tracks
                            .iter()
                            .filter(|t| t.playable)
                            .map(|t| t.id.clone())
                            .collect(),
                        index,
                        app.config.shuffle,
                    );
                }
                app.load(tx);
            }
        }
        KeyCode::Char(' ') => {
            if app.state == State::Playing || app.state == State::Loading {
                app.send(tx, Command::Pause);
                app.state = State::Paused;
            } else if app.loaded {
                app.send(tx, Command::Resume);
                app.state = State::Loading;
            } else {
                if app.queue.current().is_none() && !app.queue.ids.is_empty() {
                    app.queue.select(0);
                }
                app.load(tx);
            }
        }
        KeyCode::Char('n') => {
            if app.queue.advance(app.config.repeat, false) {
                app.load(tx);
            } else {
                app.stop(tx);
                app.status = "End of queue.".into();
            }
        }
        KeyCode::Char('p') if app.queue.previous() => app.load(tx),
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
            app.config.shuffle = !app.config.shuffle;
            app.queue.set_shuffle(app.config.shuffle);
            app.status = format!("Shuffle {}", if app.config.shuffle { "on" } else { "off" });
        }
        KeyCode::Char('r') => {
            app.config.repeat = app.config.repeat.cycle();
            app.status = format!("Repeat: {:?}", app.config.repeat);
        }
        KeyCode::Char('a') if app.view != View::Help => {
            if let Some(track) = app.selected_track() {
                if track.playable {
                    app.queue.enqueue(track.id);
                    app.status = format!("Added {} to queue", track.name);
                } else {
                    app.status = "Unavailable track cannot be queued.".into();
                }
            }
        }
        KeyCode::Delete if app.view == View::Queue => {
            if app.queue.remove(app.queue.selected) {
                app.stop(tx);
            }
            app.status = "Queue item removed.".into();
        }
        _ => (),
    }
}

pub async fn run(store: Storage) -> Result<()> {
    let config = store.config()?;
    let queue = store.queue()?;
    let catalog = Catalog::new(TokenManager::load(&config)?)?;
    let mut playback = playback::Playback::spawn(
        TokenManager::load_streaming()?,
        config.client_id.clone(),
        config.volume,
    );
    let mut app = App::new(config, queue);
    let (bg_tx, mut bg_rx) = mpsc::unbounded_channel();
    let mut tasks = Tasks {
        catalog,
        tx: bg_tx.clone(),
        browse: None,
        metadata: None,
        requested: HashSet::new(),
    };
    let (save_tx, mut save_rx) = watch::channel((app.config.clone(), app.queue.clone()));
    let persistence = tokio::spawn(async move {
        while save_rx.changed().await.is_ok() {
            let (config, queue) = save_rx.borrow_and_update().clone();
            let store = store.clone();
            let result = tokio::task::spawn_blocking(move || store.save(&config, &queue)).await;
            if let Err(e) = result.map_err(anyhow::Error::from).and_then(|r| r) {
                let _ = bg_tx.send(Background::SaveError(format!(
                    "Could not save settings/queue: {e:#}"
                )));
            }
        }
    });
    let mut terminal = ui::TerminalGuard::enter()?;
    let mut keys = EventStream::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(100));
    let mut save_tick = tokio::time::interval(std::time::Duration::from_secs(2));
    let mut playback_open = true;
    let result: Result<()> = async {
        loop {
            terminal.terminal.draw(|frame| ui::draw(frame,&app))?;
            tokio::select! {
                key_event = keys.next() => match key_event {
                    Some(Ok(Input::Key(event))) => { key(&mut app,event,&mut tasks,&playback.commands); save_tx.send_replace((app.config.clone(),app.queue.clone())); },
                    Some(Ok(Input::Paste(text))) if app.editing => app.query.extend(text.chars().filter(|c| !c.is_control()).take(500usize.saturating_sub(app.query.len()))),
                    Some(Err(e)) => return Err(e.into()), None => break, _ => (),
                },
                event = playback.events.recv(), if playback_open => if let Some(event) = event { app.playback_event(event,&playback.commands); } else { playback_open = false; app.loaded = false; app.state = State::Failed; app.status = "Playback worker exited. Quit and restart Tuitify; your queue is saved.".into(); },
                event = bg_rx.recv() => match event {
                    Some(Background::Page(id,result)) if id == app.request => {
                        app.busy = false;
                        match result {
                            Ok(page) => {
                                app.next = page.next;
                                if let Rows::Tracks(tracks) = &page.rows { for track in tracks { app.cache.insert(track.id.clone(),track.clone()); } }
                                if page.offset == 0 { app.rows = page.rows; app.selected = 0; }
                                else { match (&mut app.rows,page.rows) { (Rows::Tracks(a),Rows::Tracks(b))=>a.extend(b), (Rows::Playlists(a),Rows::Playlists(b))=>a.extend(b), _=>() } }
                                app.status = if app.len() == 0 { "No tracks or playlists available here. / starts a search.".into() } else { format!("{} loaded | Enter play/open | a enqueue{}",app.len(),if app.next.is_some() {" | PgDn load more"} else {""}) };
                            },
                            Err(e) => app.status = format!("{e:#}"),
                        }
                    },
                    Some(Background::Metadata(track)) => { app.cache.insert(track.id.clone(),track); },
                    Some(Background::MetadataError(e)) | Some(Background::SaveError(e)) => app.status = e,
                    _ => (),
                },
                _ = tick.tick() => tasks.metadata(&app),
                _ = save_tick.tick() => { save_tx.send_replace((app.config.clone(),app.queue.clone())); },
                _ = tokio::signal::ctrl_c() => break,
            }
            if app.quit { break; }
        }
        Ok(())
    }.await;
    app.send(&playback.commands, Command::Stop);
    save_tx.send_replace((app.config.clone(), app.queue.clone()));
    drop(save_tx);
    let saved = persistence.await;
    drop(terminal);
    saved?;
    // Surface a final disk failure even after the terminal has been restored.
    while let Ok(event) = bg_rx.try_recv() {
        if let Background::SaveError(e) = event {
            eprintln!("{e}");
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
