//! Metadata lookups for Discord never change the playback provider or queue.
use super::*;

impl Catalog {
    pub(crate) fn presence_catalog(&self, config: &crate::storage::Config) -> Option<Self> {
        let mut catalog = if self.offline {
            Self::new(TokenManager::load_optional(config).ok()??).ok()?
        } else {
            let mut isolated = self.clone();
            isolated.cooldown = Arc::new(Mutex::new(None));
            isolated
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
    #[tokio::test]
    async fn presence_throttling_does_not_gate_foreground_catalog_requests() {
        let server = MockServer::start().await;
        Mock::given(path("/search"))
            .respond_with(ResponseTemplate::new(429))
            .expect(1)
            .mount(&server)
            .await;
        let id = "0000000000000000000001";
        Mock::given(path(format!("/tracks/{id}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id":id,"name":"Song","artists":[{"name":"Artist"}],"duration_ms":200000
            })))
            .expect(1)
            .mount(&server)
            .await;
        let catalog = Catalog::mock(&server.uri());
        let before = catalog.health();
        let presence = catalog
            .presence_catalog(&crate::storage::Config::default())
            .unwrap();
        let track = Track {
            name: "Song".into(),
            artists: "Artist".into(),
            duration_ms: 200000,
            ..Track::default()
        };
        assert!(presence.presence_match(&track).await.is_err());
        assert!(presence.presence_match(&track).await.is_err());
        assert!(catalog.cooldown.lock().await.is_none());
        assert_eq!(catalog.health(), before);
        assert_eq!(catalog.track(id).await.unwrap().id, id);
    }
}
