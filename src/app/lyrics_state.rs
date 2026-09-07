use super::Track;

#[derive(Default)]
pub struct LyricsState {
    pub scroll: usize,
    pub error: Option<String>,
    pub(super) metadata: Option<Track>,
    pub(super) request: u64,
    pub loading: bool,
    pub track_id: Option<String>,
    pub content: Option<crate::lyrics::Lyrics>,
}
