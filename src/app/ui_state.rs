use crate::stats::{SongStats, TrackStat};
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overlay {
    #[default]
    None,
    Lyrics,
    Visualizer,
    Stats,
    MixBuilder,
}

#[derive(Default)]
pub struct UiState {
    pub overlay: Overlay,
    pub render: RefCell<RenderState>,
    pub stats: RefCell<StatsView>,
}

impl UiState {
    pub fn close(&mut self, overlay: Overlay) {
        if self.overlay == overlay {
            self.overlay = Overlay::None;
        }
    }
    pub fn toggle(&mut self, overlay: Overlay) {
        self.overlay = if self.overlay == overlay {
            Overlay::None
        } else {
            overlay
        };
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatsSort {
    #[default]
    Plays,
    Time,
    Title,
}

impl StatsSort {
    pub fn next(self) -> Self {
        match self {
            Self::Plays => Self::Time,
            Self::Time => Self::Title,
            Self::Title => Self::Plays,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Plays => "Plays",
            Self::Time => "Time",
            Self::Title => "Title",
        }
    }
}

/// Session-only presentation state; never changes the persisted statistics schema.
#[derive(Default)]
pub struct StatsView {
    pub selected: usize,
    pub query: String,
    pub editing: bool,
    pub sort: StatsSort,
    revision: Option<u64>,
    cached_query: String,
    cached_sort: StatsSort,
    pub rows: Vec<TrackStat>,
    pub total_plays: u64,
    pub total_ms: u64,
    pub unique_tracks: usize,
    pub top_song: String,
}

impl StatsView {
    pub fn refresh(&mut self, stats: &SongStats) {
        if self.revision == Some(stats.revision)
            && self.cached_query == self.query
            && self.cached_sort == self.sort
        {
            return;
        }
        self.total_plays = 0;
        self.total_ms = 0;
        self.unique_tracks = stats.len();
        let mut rows = stats.sorted_tracks();
        self.top_song = rows.first().map(|s| s.name.clone()).unwrap_or_default();
        for row in &rows {
            self.total_plays = self.total_plays.saturating_add(row.play_count);
            self.total_ms = self.total_ms.saturating_add(row.listened_ms);
        }
        let query = self.query.trim().to_lowercase();
        rows.retain(|s| {
            s.name.to_lowercase().contains(&query) || s.artists.to_lowercase().contains(&query)
        });
        match self.sort {
            StatsSort::Plays => {}
            StatsSort::Time => rows.sort_by(|a, b| {
                b.listened_ms
                    .cmp(&a.listened_ms)
                    .then_with(|| b.play_count.cmp(&a.play_count))
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    .then_with(|| a.id.cmp(&b.id))
            }),
            StatsSort::Title => rows.sort_by(|a, b| {
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then_with(|| a.id.cmp(&b.id))
            }),
        }
        self.rows = rows;
        self.revision = Some(stats.revision);
        self.cached_query.clone_from(&self.query);
        self.cached_sort = self.sort;
    }
}

/// Layout feedback consumed by input and metadata scheduling after a frame.
/// Rendering receives this state explicitly, separately from the application.
pub struct RenderState {
    pub mouse_hits: Vec<(ratatui::layout::Rect, super::MouseTarget)>,
    pub catalog_scroll: usize,
    pub stats_scroll: usize,
    pub queue_scroll: usize,
    pub queue_height: usize,
    pub terminal_size: (u16, u16),
    pub help_length: usize,
    pub lyrics_length: usize,
    pub background: crate::ui::BackgroundState,
}
impl Default for RenderState {
    fn default() -> Self {
        Self {
            mouse_hits: Vec::new(),
            catalog_scroll: 0,
            stats_scroll: 0,
            queue_scroll: 0,
            queue_height: 40,
            terminal_size: (120, 35),
            help_length: 1,
            lyrics_length: 1,
            background: crate::ui::BackgroundState::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stats_view_filters_sorts_and_keeps_all_time_totals() {
        let mut stats = SongStats::default();
        let a = "1".repeat(22);
        let b = "2".repeat(22);
        stats.add_play(&a, "Zulu", "First artist");
        stats.add_play(&a, "Zulu", "First artist");
        stats.add_play(&b, "Alpha", "Second artist");
        stats.add_listened_ms(&a, 10_000, "Zulu", "First artist");
        stats.add_listened_ms(&b, 40_000, "Alpha", "Second artist");
        let mut view = StatsView::default();
        view.refresh(&stats);
        assert_eq!(view.rows[0].id, a);
        assert_eq!(
            (view.total_plays, view.total_ms, view.unique_tracks),
            (3, 50_000, 2)
        );
        view.sort = StatsSort::Time;
        view.refresh(&stats);
        assert_eq!(view.rows[0].id, b);
        view.sort = StatsSort::Title;
        view.refresh(&stats);
        assert_eq!(view.rows[0].id, b);
        view.query = " FIRST ".into();
        view.refresh(&stats);
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.rows[0].id, a);
        assert_eq!(view.total_ms, 50_000);
        view.query = "missing".into();
        view.refresh(&stats);
        assert!(view.rows.is_empty());
        stats.add_listened_ms(&b, 5_000, "Alpha", "Second artist");
        view.refresh(&stats);
        assert_eq!(view.total_ms, 55_000);
        assert_eq!(view.top_song, "Zulu");
    }
}
