use super::Track;

#[derive(Default)]
pub struct LyricsState {
    pub reading_mode: crate::lyrics::ReadingMode,
    pub scroll: usize,
    /// Manual browsing pauses playback following until `a` resumes it.
    pub manual_scroll: bool,
    pub error: Option<String>,
    pub(super) metadata: Option<Track>,
    pub(super) request: u64,
    pub loading: bool,
    pub track_id: Option<String>,
    pub content: Option<crate::lyrics::Lyrics>,
    /// The track whose lyrics are being fetched ahead of time.
    pub preload_id: Option<String>,
    pub(super) preload_request: u64,
    /// Lyrics fetched ahead of time, ready the moment the track starts.
    pub cached: Option<(String, crate::lyrics::Lyrics)>,
}
