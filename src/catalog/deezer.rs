use super::*;
use crate::volta::matching::{compatible_duration, select, youtube_recording};

impl Catalog {
    /// Read the original recording without consulting an audio provider's match.
    pub(crate) async fn recording(&self, id: &str) -> Result<(Track, Option<String>)> {
        if crate::deezer::key(id).is_some() {
            return self
                .deezer
                .as_ref()
                .context("Deezer catalog is unavailable")?
                .recording(id)
                .await;
        }
        if let Some(client) = &self.volta {
            let key = crate::volta::key(id).context("Invalid Volta track ID")?;
            let value = client.get(&format!("/track/{key}"), &[]).await?;
            let track = crate::volta::catalog::parse_track(&value)
                .context("Volta returned incomplete track metadata")?;
            return Ok((
                track,
                value["isrc"]
                    .as_str()
                    .and_then(crate::deezer::isrc)
                    .or_else(|| crate::deezer::isrc(key)),
            ));
        }
        if self.youtube.is_some() {
            return Ok((self.track(id).await?, None));
        }
        anyhow::ensure!(crate::model::valid_id(id), "Invalid Spotify track ID");
        let value = self.get(&format!("/tracks/{id}"), &[]).await?;
        let track = parse_track(&value).context("Spotify returned incomplete track metadata")?;
        Ok((
            track,
            value["external_ids"]["isrc"]
                .as_str()
                .and_then(crate::deezer::isrc),
        ))
    }

    pub(crate) fn native_audio_identity(&self, id: &str) -> bool {
        if self.volta.is_some() {
            crate::volta::key(id).is_some()
        } else if self.youtube.is_some() {
            crate::youtube::video_key(id).is_some()
        } else {
            crate::model::valid_id(id)
        }
    }

    pub(crate) fn with_deezer(mut self, client: crate::deezer::Client) -> Self {
        self.deezer = Some(client);
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
        self.resolve_audio_track(self, id).await
    }

    pub(crate) async fn resolve_audio_track(
        &self,
        origin: &Catalog,
        id: &str,
    ) -> Result<(Track, String)> {
        let (track, isrc) = origin.recording(id).await?;
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
                .context("No matching YouTube recording is available for this song")?
        } else {
            select(&track, &candidates)
                .context("No unambiguous audio recording matches this song")?
        };
        Ok((track, audio_id))
    }
}

#[cfg(test)]
mod tests;
