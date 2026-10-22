//! Metadata lookups for Discord never change the playback provider or queue.
use super::*;

impl Catalog {
    pub(crate) fn presence_catalog(&self, config: &crate::storage::Config) -> Option<Self> {
        let mut catalog = if self.offline {
            Self::new(TokenManager::load_optional(config).ok()??).ok()?
        } else {
            self.clone()
        };
        catalog.volta = None;
        catalog.youtube = None;
        catalog.deezer = None;
        Some(catalog)
    }

    pub(crate) async fn presence_match(&self, track: &Track) -> Result<Track> {
        let quoted = |text: &str| {
            text.chars()
                .filter(|ch| !ch.is_control())
                .take(128)
                .collect::<String>()
                .replace(['"', '\\'], " ")
        };
        let value = self
            .request(
                "/search",
                &[
                    (
                        "q",
                        format!(
                            "track:\"{}\" artist:\"{}\"",
                            quoted(&track.name),
                            quoted(track.artists.split(',').next().unwrap_or("").trim())
                        ),
                    ),
                    ("type", "track".into()),
                    ("limit", "10".into()),
                ],
            )
            .await?;
        let candidates: Vec<_> = value["tracks"]["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(parse_track)
            .map(|mut track| {
                // A regional audio restriction does not invalidate a metadata link.
                track.playable = true;
                track
            })
            .collect();
        let id = crate::volta::matching::select(track, &candidates)?;
        candidates
            .into_iter()
            .find(|track| track.id == id)
            .context("Spotify returned no matching recording")
    }
}
