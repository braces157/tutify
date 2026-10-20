use super::{Client, key};
use crate::{
    catalog::{ArtistResultSource, Browse, Page, RecommendationSource, Recommendations, Rows},
    model::{Playlist, Track},
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;

const PAGE_SIZE: usize = 20;
fn text(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        v[*key]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    })
}
fn identity(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| {
        let raw = v[*k]
            .as_str()
            .map(str::to_owned)
            .or_else(|| v[*k].as_i64().map(|n| n.to_string()))?;
        super::valid_key(&raw).then(|| format!("volta:{raw}"))
    })
}
pub(crate) fn parse_track(value: &Value) -> Option<Track> {
    let v = value
        .get("track")
        .filter(|v| v.is_object())
        .unwrap_or(value);
    if v["type"]
        .as_str()
        .is_some_and(|kind| !matches!(kind, "track" | "song"))
    {
        return None;
    }
    let artist = text(v, &["artist", "artist_name"])
        .or_else(|| {
            v["artists"].as_array().map(|artists| {
                artists
                    .iter()
                    .filter_map(|a| a.as_str().or_else(|| a["name"].as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
        })
        .unwrap_or_default();
    let duration_ms = v["duration_ms"]
        .as_u64()
        .or_else(|| v["durationMs"].as_u64())
        .or_else(|| v["duration"].as_u64().map(|s| s.saturating_mul(1000)))
        .unwrap_or(0)
        .min(u64::from(u32::MAX)) as u32;
    Some(Track {
        id: identity(v, &["id", "track_id", "isrc"])?,
        name: text(v, &["name", "title"])?,
        artists: artist,
        artist_ids: identity(v, &["artist_id", "artistId"])
            .into_iter()
            .collect(),
        duration_ms,
        playable: v["playable"].as_bool().unwrap_or(true),
        album: text(v, &["album", "album_name"]),
        album_id: identity(v, &["album_id", "albumId"]),
        album_art_url: text(v, &["cover_url", "coverUrl", "image_url", "cover", "image"]),
        track_number: v["track_number"]
            .as_u64()
            .map(|n| n.min(u64::from(u32::MAX)) as u32),
        music_metadata: true,
    })
}
fn items<'a>(value: &'a Value, keys: &[&str]) -> &'a [Value] {
    if let Some(items) = value.as_array() {
        return items;
    }
    for key in keys {
        if let Some(items) = value[*key].as_array() {
            return items;
        }
        if let Some(items) = value[*key]["items"].as_array() {
            return items;
        }
    }
    &[]
}
impl Client {
    pub(crate) async fn track(&self, id: &str) -> Result<Track> {
        let id = key(id).context("Invalid Volta track ID")?;
        let value = self.get(&format!("/track/{id}"), &[]).await?;
        parse_track(&value).context("Volta returned incomplete track metadata")
    }
    pub(crate) async fn page(&self, browse: &Browse, offset: usize) -> Result<Page> {
        let (path, query) = match browse {
            Browse::Search(q) if key(q.trim()).is_some() => {
                return Ok(Page {
                    rows: Rows::Tracks(vec![self.track(q.trim()).await?]),
                    offset: 0,
                    next: None,
                    artist_source: None,
                });
            }
            Browse::Search(q) => {
                if q.trim().is_empty() {
                    return Ok(Page {
                        rows: Rows::Tracks(Vec::new()),
                        offset: 0,
                        next: None,
                        artist_source: None,
                    });
                }
                (
                    "/search".into(),
                    vec![("q", q.clone()), ("limit", "100".into())],
                )
            }
            Browse::Liked => ("/library/tracks".into(), Vec::new()),
            Browse::Playlists => ("/library/playlists".into(), Vec::new()),
            Browse::Playlist(id) => (
                format!(
                    "/library/playlists/{}",
                    key(id).context("Invalid Volta playlist ID")?
                ),
                Vec::new(),
            ),
            Browse::Album(id) => (
                format!("/album/{}", key(id).context("Invalid Volta album ID")?),
                Vec::new(),
            ),
            Browse::Artist(id) => (
                format!("/artists/{}", key(id).context("Invalid Volta artist ID")?),
                Vec::new(),
            ),
        };
        let value = self.get(&path, &query).await?;
        // The documented API has no offset contract. Slice its bounded result locally.
        if matches!(browse, Browse::Playlists) {
            let all = items(&value, &["playlists", "items"]);
            let rows = all
                .iter()
                .filter_map(|p| {
                    Some(Playlist {
                        id: identity(p, &["id"])?,
                        name: text(p, &["name", "title"])?,
                        owner: text(p, &["owner", "owner_name"]).unwrap_or_else(|| "Volta".into()),
                    })
                })
                .collect::<Vec<_>>();
            return Ok(Page {
                next: (offset.saturating_add(PAGE_SIZE) < rows.len())
                    .then_some(offset.saturating_add(PAGE_SIZE)),
                rows: Rows::Playlists(rows.into_iter().skip(offset).take(PAGE_SIZE).collect()),
                offset,
                artist_source: None,
            });
        }
        let root = if value["results"].is_object() {
            &value["results"]
        } else {
            &value
        };
        let rows = items(
            root,
            &[
                "tracks",
                "songs",
                "items",
                "results",
                "top_tracks",
                "topTracks",
            ],
        )
        .iter()
        .filter_map(parse_track)
        .collect::<Vec<_>>();
        ensure!(
            rows.len() <= crate::queue::MAX_TRACKS,
            "Volta returned too many tracks"
        );
        Ok(Page {
            next: (offset.saturating_add(PAGE_SIZE) < rows.len())
                .then_some(offset.saturating_add(PAGE_SIZE)),
            rows: Rows::Tracks(rows.into_iter().skip(offset).take(PAGE_SIZE).collect()),
            offset,
            artist_source: matches!(browse, Browse::Artist(_)).then_some(ArtistResultSource::Volta),
        })
    }
    pub(crate) async fn recommendations(
        &self,
        seed: &Track,
        excluded: &[Track],
        round: usize,
    ) -> Result<Recommendations> {
        // Personal applications cannot reach the private radio routes.
        let page = self
            .page(
                &Browse::Search(seed.artists.clone()),
                round.saturating_mul(PAGE_SIZE),
            )
            .await?;
        let Rows::Tracks(tracks) = page.rows else {
            unreachable!()
        };
        Ok(Recommendations {
            tracks: tracks
                .into_iter()
                .filter(|t| t.id != seed.id && !excluded.iter().any(|e| e.id == t.id))
                .collect(),
            source: RecommendationSource::Volta,
        })
    }
}
