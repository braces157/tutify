use super::*;
use std::cell::RefCell;

#[derive(Default)]
pub struct QueueFilter {
    pub query: String,
    pub editing: bool,
    previous_query: String,
    cached: RefCell<FilteredQueue>,
}

#[derive(Default)]
struct FilteredQueue {
    stamp: Option<(u64, u64, usize, u64)>,
    query: String,
    indices: Arc<Vec<usize>>,
    missing: usize,
}

impl QueueFilter {
    /// Store queue positions, never track IDs: duplicate occurrences stay distinct.
    pub fn rows(&self, queue: &Queue, cache: &crate::cache::MetadataCache) -> Arc<Vec<usize>> {
        let stamp = (
            queue.epoch,
            queue.revision,
            queue.order.len(),
            if self.query.trim().is_empty() {
                0
            } else {
                cache.revision
            },
        );
        let mut cached = self.cached.borrow_mut();
        if cached.stamp == Some(stamp) && cached.query == self.query {
            return cached.indices.clone();
        }
        let terms: Vec<String> = self
            .query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let mut indices = Vec::new();
        let mut missing = 0;
        for (at, original) in queue.order.iter().enumerate() {
            if terms.is_empty() {
                indices.push(at);
                continue;
            }
            let track = cache.get(&queue.ids[*original]);
            if track.is_none_or(|t| t.name.is_empty()) {
                missing += 1;
            }
            if track.is_some_and(|track| {
                let name = track.name.to_lowercase();
                let artists = track.artists.to_lowercase();
                let album = track.album.as_deref().unwrap_or_default().to_lowercase();
                terms.iter().all(|term| {
                    name.contains(term) || artists.contains(term) || album.contains(term)
                })
            }) {
                indices.push(at);
            }
        }
        *cached = FilteredQueue {
            stamp: Some(stamp),
            query: self.query.clone(),
            indices: Arc::new(indices),
            missing,
        };
        cached.indices.clone()
    }

    pub fn missing(&self, queue: &Queue, cache: &crate::cache::MetadataCache) -> usize {
        self.rows(queue, cache);
        self.cached.borrow().missing
    }
}

impl App {
    pub fn queue_rows(&self) -> Arc<Vec<usize>> {
        self.ui.queue.rows(&self.queue, &self.cache)
    }

    pub fn queue_selection(&self) -> Option<usize> {
        let rows = self.queue_rows();
        if rows.is_empty() {
            return None;
        }
        Some(
            rows.binary_search(&self.queue.selected)
                .unwrap_or_else(|at| at.min(rows.len() - 1)),
        )
    }

    pub fn selected_queue_index(&self) -> Option<usize> {
        // Back navigation may restore a position from a queue that was replaced.
        // Require navigation before acting on that stale, out-of-range selection.
        if self.queue.selected >= self.queue.order.len() {
            return None;
        }
        self.queue_selection().map(|at| self.queue_rows()[at])
    }

    pub(super) fn move_queue_selection(&mut self, delta: isize) {
        if let Some(at) = self.queue_selection() {
            let rows = self.queue_rows();
            let next = at.saturating_add_signed(delta).min(rows.len() - 1);
            self.queue.selected = rows[next];
        }
    }

    pub(super) fn start_queue_filter(&mut self) {
        self.ui.overlay = Overlay::None;
        self.context_menu = None;
        self.catalog.sidebar = false;
        self.catalog.editing = false;
        self.catalog.filtering = false;
        self.ui.queue.editing = true;
        self.status = "Filter queue by title, artist or album. Enter finishes; Esc clears.".into();
    }

    pub(super) fn reset_queue_filter_selection(&mut self) {
        if let Some(&first) = self.queue_rows().first() {
            self.queue.selected = first;
        }
        self.ui.render.borrow_mut().queue_scroll = 0;
        self.ui.render.borrow_mut().queue_filter_metadata_start = None;
    }

    pub(super) fn clear_queue_filter(&mut self) {
        if !self.ui.queue.query.trim().is_empty() {
            self.ui.queue.previous_query = self.ui.queue.query.clone();
        }
        self.ui.queue.query.clear();
        self.ui.queue.editing = false;
        self.ui.render.borrow_mut().queue_scroll = 0;
        self.ui.render.borrow_mut().queue_filter_metadata_start = None;
    }

    pub(super) fn restore_queue_filter(&mut self) {
        if self.ui.queue.previous_query.is_empty() {
            self.status = "No cleared queue filter to restore yet.".into();
            return;
        }
        self.ui.queue.query = self.ui.queue.previous_query.clone();
        self.ui.queue.editing = false;
        self.reset_queue_filter_selection();
        self.status = "Restored the last queue filter. / edits; Esc clears.".into();
    }

    pub(super) fn active_filter(&self) -> &str {
        if self.catalog.view == View::Queue {
            &self.ui.queue.query
        } else {
            &self.catalog.filter
        }
    }

    /// Filter scroll offsets are presentation indices, not original queue positions.
    pub(super) fn queue_metadata_start(&self) -> usize {
        let scroll = self.ui.render.borrow().queue_scroll;
        if self.catalog.view == View::Queue && !self.ui.queue.query.is_empty() {
            self.ui
                .render
                .borrow()
                .queue_filter_metadata_start
                .unwrap_or_else(|| {
                    self.queue_rows()
                        .get(scroll)
                        .copied()
                        .unwrap_or(self.queue.selected)
                })
        } else {
            scroll
        }
    }
}
