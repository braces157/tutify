use crate::stats::{SongStats, TrackStat, compare_plays};
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
        let query = self.query.trim().to_lowercase();
        let mut rows = Vec::new();
        let mut top: Option<(&TrackStat, String)> = None;
        for row in stats.tracks.values() {
            self.total_plays = self.total_plays.saturating_add(row.play_count);
            self.total_ms = self.total_ms.saturating_add(row.listened_ms);
            let title = row.name.to_lowercase();
            if top.as_ref().is_none_or(|(best, best_title)| {
                compare_plays(row, &title, best, best_title).is_lt()
            }) {
                top = Some((row, title.clone()));
            }
            if query.is_empty()
                || title.contains(&query)
                || row.artists.to_lowercase().contains(&query)
            {
                rows.push((title, row));
            }
        }
        self.top_song = top.map(|(row, _)| row.name.clone()).unwrap_or_default();
        match self.sort {
            StatsSort::Plays => rows.sort_unstable_by(|(a_title, a), (b_title, b)| {
                compare_plays(a, a_title, b, b_title)
            }),
            StatsSort::Time => rows.sort_unstable_by(|(a_title, a), (b_title, b)| {
                b.listened_ms
                    .cmp(&a.listened_ms)
                    .then_with(|| b.play_count.cmp(&a.play_count))
                    .then_with(|| a_title.cmp(b_title))
                    .then_with(|| a.id.cmp(&b.id))
            }),
            StatsSort::Title => rows.sort_unstable_by(|(a_title, a), (b_title, b)| {
                a_title.cmp(b_title).then_with(|| a.id.cmp(&b.id))
            }),
        }
        self.rows = rows.into_iter().map(|(_, row)| row.clone()).collect();
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
    fn stats_order_is_deterministic_for_unicode_and_case_ties() {
        let mut stats = SongStats::default();
        for (i, name) in ["Zulu", "alpha", "ALPHA", "日本語", "éclair", "ÉCLAIR"]
            .into_iter()
            .enumerate()
        {
            stats.add_play(&format!("{i:022}"), name, "Artist");
        }
        let expected: Vec<_> = [1, 2, 0, 4, 5, 3]
            .into_iter()
            .map(|i| format!("{i:022}"))
            .collect();
        let mut view = StatsView::default();
        for sort in [StatsSort::Plays, StatsSort::Time, StatsSort::Title] {
            view.sort = sort;
            view.refresh(&stats);
            assert_eq!(
                view.rows
                    .iter()
                    .map(|row| row.id.clone())
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(view.top_song, "alpha");
        }
        view.query = "éCLAir".into();
        view.refresh(&stats);
        assert_eq!(view.rows.len(), 2);
        assert_eq!(view.total_plays, 6);
        assert_eq!(view.top_song, "alpha");
        stats.tracks.clear();
        stats.revision += 1;
        view.refresh(&stats);
        assert!(view.rows.is_empty());
        assert!(view.top_song.is_empty());
        assert_eq!(view.total_plays, 0);
    }
    #[test]
    #[ignore = "Release microbenchmark; run with --release --ignored --nocapture"]
    fn benchmark_stats_refresh() {
        for count in [5_000, 50_000] {
            let mut stats = SongStats::default();
            for i in 0..count {
                stats.add_play(
                    &format!("{i:022}"),
                    &format!("Song {:05} 日本語", count - i),
                    "Artist",
                );
            }
            for sort in [StatsSort::Plays, StatsSort::Time, StatsSort::Title] {
                let mut view = StatsView {
                    sort,
                    ..Default::default()
                };
                let start = std::time::Instant::now();
                for _ in 0..5 {
                    stats.revision += 1;
                    view.refresh(&stats);
                    std::hint::black_box(&view.rows);
                }
                println!(
                    "stats_rows={count} sort={sort:?} mean_refresh_ms={:.3}",
                    start.elapsed().as_secs_f64() * 200.0
                );
                assert_eq!(view.rows.len(), count);
            }
        }
    }
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
