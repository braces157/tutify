use super::*;
use crate::volta::matching::{compatible_duration, select, youtube_recording};

impl Catalog {
    pub(crate) fn with_deezer(mut self, client: crate::deezer::Client) -> Self {
        self.deezer = Some(client);
        self
    }

    pub(crate) fn with_audio_fallback(mut self) -> Self {
        if self.volta.is_some() {
            self.youtube = crate::youtube::Tools::discover().ok();
        }
        self
    }

    pub(super) async fn remember_search_route(&self, query: &str, native: bool) {
        let mut routes = self.search_routes.lock().await;
        if routes.len() >= 32 {
            routes.clear();
        }
        routes.insert(query.into(), native);
    }

    /// Resolve a catalog identity only at playback/preload time. Never persist
    /// a provider's transient audio identity over the selected Deezer recording.
    pub(crate) async fn resolve_playback_track(&self, id: &str) -> Result<(Track, String)> {
        match self.resolve_primary_playback(id).await {
            Ok(recording) => Ok(recording),
            Err(error) if self.volta.is_some() && self.youtube.is_some() => {
                let (track, _) = self
                    .deezer
                    .as_ref()
                    .context("Deezer catalog is unavailable")?
                    .recording(id)
                    .await?;
                let tools = self.youtube.as_ref().unwrap();
                let page = tools
                    .page(
                        &Browse::Search(format!("{} {}", track.name, track.artists)),
                        0,
                    )
                    .await?;
                let Rows::Tracks(candidates) = page.rows else {
                    bail!("YouTube returned no songs")
                };
                let audio = candidates
                    .iter()
                    .filter(|candidate| youtube_recording(&track, candidate))
                    .min_by_key(|candidate| candidate.duration_ms.abs_diff(track.duration_ms))
                    .context(format!(
                        "Volta recording unavailable ({error}); no matching YouTube recording"
                    ))?;
                Ok((track, audio.id.clone()))
            }
            Err(error) => Err(error),
        }
    }

    async fn resolve_primary_playback(&self, id: &str) -> Result<(Track, String)> {
        let client = self
            .deezer
            .as_ref()
            .context("Deezer catalog is unavailable")?;
        let (track, isrc) = client.recording(id).await?;
        if let Some(volta) = &self.volta {
            if let Some(code) = &isrc {
                match volta.get(&format!("/track/{code}"), &[]).await {
                    Ok(value) => {
                        if let Some(candidate) = crate::volta::catalog::parse_track(&value)
                            && candidate.playable
                            && (crate::volta::key(&candidate.id) == Some(code.as_str())
                                || value["isrc"]
                                    .as_str()
                                    .and_then(crate::deezer::isrc)
                                    .as_ref()
                                    == Some(code))
                            && compatible_duration(&track, &candidate, true)
                        {
                            return Ok((track, candidate.id));
                        }
                    }
                    Err(error) if ServiceFailure::is(&error, FailureKind::MissingItem) => (),
                    Err(error) => return Err(error),
                }
            }
        } else if self.youtube.is_none()
            && let Some(code) = &isrc
        {
            let value = self
                .get(
                    "/search",
                    &[
                        ("q", format!("isrc:{code}")),
                        ("type", "track".into()),
                        ("limit", "10".into()),
                    ],
                )
                .await?;
            if let Some(candidate) = value["tracks"]["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|value| {
                    value["external_ids"]["isrc"]
                        .as_str()
                        .and_then(crate::deezer::isrc)
                        .as_ref()
                        == Some(code)
                })
                .filter_map(parse_track)
                .find(|candidate| {
                    candidate.playable && compatible_duration(&track, candidate, true)
                })
            {
                return Ok((track, candidate.id));
            }
        }
        let page = self
            .native_page(
                &Browse::Search(format!("{} {}", track.name, track.artists)),
                0,
                None,
            )
            .await?;
        let Rows::Tracks(candidates) = page.rows else {
            bail!("Audio provider returned no songs")
        };
        let audio_id = if self.volta.is_none() && self.youtube.is_some() {
            candidates
                .iter()
                .filter(|candidate| youtube_recording(&track, candidate))
                .min_by_key(|candidate| candidate.duration_ms.abs_diff(track.duration_ms))
                .map(|candidate| candidate.id.clone())
                .context("No matching YouTube recording is available for this Deezer song")?
        } else {
            select(&track, &candidates)
                .context("No unambiguous audio recording matches this Deezer song")?
        };
        Ok((track, audio_id))
    }
}

#[cfg(test)]
mod tests;
