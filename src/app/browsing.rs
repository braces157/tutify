use super::{App, Browse, Rows, SearchScope, View};
use std::{cell::RefCell, sync::Arc};

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub view: View,
    pub browse: Browse,
    pub title: String,
    pub selected: usize,
    pub scroll: usize,
    pub rows: Rows,
    pub next: Option<usize>,
    pub query: String,
    pub search_scope: SearchScope,
    pub filter: String,
    pub filtering: bool,
    #[allow(dead_code)]
    pub breadcrumb: String,
    pub queue_selected: usize,
    pub queue_scroll: usize,
}

#[derive(Default)]
struct FilterCache {
    revision: u64,
    query: String,
    view: Option<View>,
    len: usize,
    indices: Arc<Vec<usize>>,
}

/// Catalog navigation, search and filtering share a revision-aware row owner.
pub struct BrowseState {
    pub view: View,
    pub sidebar: bool,
    pub nav: usize,
    pub rows: Rows,
    pub selected: usize,
    pub query: String,
    pub search_scope: SearchScope,
    pub library_scanned: usize,
    pub editing: bool,
    pub busy: bool,
    pub title: String,
    pub next: Option<usize>,
    pub browse: Browse,
    pub history: Vec<HistoryEntry>,
    pub(super) request: u64,
    pub filter: String,
    pub filtering: bool,
    pub(super) rows_revision: u64,
    filtered: RefCell<FilterCache>,
}
impl BrowseState {
    pub(super) fn new(restored: bool) -> Self {
        Self {
            view: if restored { View::Queue } else { View::Search },
            sidebar: false,
            nav: if restored { 3 } else { 0 },
            rows: Rows::Tracks(vec![]),
            selected: 0,
            query: String::new(),
            search_scope: SearchScope::Spotify,
            library_scanned: 0,
            editing: false,
            busy: false,
            title: "Search".into(),
            next: None,
            browse: Browse::Search(String::new()),
            history: Vec::new(),
            request: 0,
            filter: String::new(),
            filtering: false,
            rows_revision: 0,
            filtered: RefCell::new(FilterCache::default()),
        }
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
    pub(super) fn reset_rows(&mut self) {
        self.rows = Rows::Tracks(Vec::new());
        self.rows_revision = self.rows_revision.wrapping_add(1);
    }

    pub(super) fn append_tracks(&mut self, tracks: Vec<super::Track>) {
        if tracks.is_empty() {
            return;
        }
        if let Rows::Tracks(rows) = &mut self.rows {
            rows.extend(tracks);
            self.rows_revision = self.rows_revision.wrapping_add(1);
        }
    }

    pub(super) fn apply_page(&mut self, page: super::Page) {
        self.next = page.next;
        self.rows_revision = self.rows_revision.wrapping_add(1);
        if page.offset == 0 {
            self.rows = page.rows;
            self.selected = 0;
        } else {
            match (&mut self.rows, page.rows) {
                (Rows::Tracks(a), Rows::Tracks(b)) => a.extend(b),
                (Rows::Playlists(a), Rows::Playlists(b)) => a.extend(b),
                _ => (),
            }
        }
    }

    pub fn push_history(&mut self, entry: HistoryEntry) {
        if self.history.len() >= 20 {
            self.history.remove(0);
        }
        self.history.push(entry);
    }

    pub fn pop_history(&mut self) -> Option<HistoryEntry> {
        self.history.pop()
    }
}

impl App {
    pub fn push_navigation(&mut self, breadcrumb: String) {
        let entry = HistoryEntry {
            view: self.catalog.view,
            browse: self.catalog.browse.clone(),
            title: self.catalog.title.clone(),
            selected: self.catalog.selected,
            scroll: self.ui.render.borrow().catalog_scroll,
            rows: self.catalog.rows.clone(),
            next: self.catalog.next,
            query: self.catalog.query.clone(),
            search_scope: self.catalog.search_scope,
            filter: self.catalog.filter.clone(),
            filtering: self.catalog.filtering,
            breadcrumb,
            queue_selected: self.queue.selected,
            queue_scroll: self.ui.render.borrow().queue_scroll,
        };
        self.catalog.push_history(entry);
    }

    pub fn pop_navigation(&mut self) -> bool {
        if let Some(entry) = self.catalog.pop_history() {
            self.catalog.view = entry.view;
            self.catalog.nav = entry.view.index();
            self.catalog.browse = entry.browse;
            self.catalog.title = entry.title;
            self.catalog.selected = entry.selected;
            self.catalog.rows = entry.rows;
            self.catalog.next = entry.next;
            self.catalog.query = entry.query;
            self.catalog.search_scope = entry.search_scope;
            self.catalog.filter = entry.filter;
            self.catalog.filtering = entry.filtering;
            self.catalog.editing = false;
            self.catalog.request = self.catalog.request.wrapping_add(1);
            self.catalog.busy = false;
            self.catalog.rows_revision = self.catalog.rows_revision.wrapping_add(1);

            self.ui.render.borrow_mut().catalog_scroll = entry.scroll;
            self.queue.selected = entry.queue_selected;
            self.ui.render.borrow_mut().queue_scroll = entry.queue_scroll;
            true
        } else {
            false
        }
    }
}
