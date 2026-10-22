use super::{Lyrics, parse_lyrics};
use crate::model::Track;
use anyhow::{Result, bail};
use futures_util::{StreamExt, stream::FuturesUnordered};
use reqwest::{Client, RequestBuilder};
use serde_json::Value;
use std::time::Duration;
use unicode_normalization::UnicodeNormalization;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);
const LRCLIB_TIMEOUT: Duration = Duration::from_secs(14);
const OVH_TIMEOUT: Duration = Duration::from_secs(8);
const CATALOG_TIMEOUT: Duration = Duration::from_secs(8);

mod chinese;

struct Endpoints<'a> {
    lrclib: &'a str,
    ovh: &'a str,
    netease: &'a str,
    qq_search: &'a str,
    qq_lyrics: &'a str,
}

const ENDPOINTS: Endpoints<'static> = Endpoints {
    lrclib: "https://lrclib.net",
    ovh: "https://api.lyrics.ovh",
    netease: "https://music.163.com",
    qq_search: "https://u.y.qq.com",
    qq_lyrics: "https://c.y.qq.com",
};

#[derive(Clone, Copy)]
enum Provider {
    Lrclib,
    Netease,
    Qq,
}

impl Provider {
    fn label(self) -> &'static str {
        match self {
            Self::Lrclib => "LRCLIB",
            Self::Netease => "NetEase",
            Self::Qq => "QQ Music",
        }
    }
}

pub(super) async fn fetch(client: &Client, track: &Track) -> Result<Option<Lyrics>> {
    fetch_from(client, &ENDPOINTS, track).await
}

async fn fetch_from(
    client: &Client,
    endpoints: &Endpoints<'_>,
    track: &Track,
) -> Result<Option<Lyrics>> {
    let mut best = None;
    let mut errors = Vec::new();
    let mut pending = FuturesUnordered::new();
    pending.push(lookup(client, endpoints, track, Provider::Lrclib));
    // Start Chinese catalogs immediately for Han metadata. Other tracks keep
    // the fast LRCLIB path and consult them only if timed lyrics are missing.
    let mut catalogs_started = chinese::has_han(&track.name) || chinese::has_han(&track.artists);
    if catalogs_started {
        pending.push(lookup(client, endpoints, track, Provider::Netease));
        pending.push(lookup(client, endpoints, track, Provider::Qq));
    }
    while let Some((provider, result)) = pending.next().await {
        match result {
            Ok(Some(lyrics)) if !lyrics.lines.is_empty() => return Ok(Some(lyrics)),
            Ok(Some(lyrics)) if best.is_none() => best = Some(lyrics),
            Err(error) => errors.push(format!("{}: {error}", provider.label())),
            _ => {}
        }
        if !catalogs_started {
            pending.push(lookup(client, endpoints, track, Provider::Netease));
            pending.push(lookup(client, endpoints, track, Provider::Qq));
            catalogs_started = true;
        }
    }
    if best.is_some() {
        return Ok(best);
    }

    let fallback = tokio::time::timeout(OVH_TIMEOUT, fetch_ovh(client, endpoints.ovh, track))
        .await
        .unwrap_or_else(|_| Err(anyhow::anyhow!("Lookup timed out")));
    match fallback {
        Ok(Some(lyrics)) => return Ok(Some(lyrics)),
        Err(error) => errors.push(format!("lyrics.ovh: {error}")),
        _ => {}
    }
    if errors.is_empty() {
        Ok(None)
    } else {
        bail!("{}", errors.join("; "))
    }
}

async fn lookup(
    client: &Client,
    endpoints: &Endpoints<'_>,
    track: &Track,
    provider: Provider,
) -> (Provider, Result<Option<Lyrics>>) {
    let mut best = None;
    let timeout = match provider {
        Provider::Lrclib => LRCLIB_TIMEOUT,
        _ => CATALOG_TIMEOUT,
    };
    let result = tokio::time::timeout(timeout, async {
        match provider {
            Provider::Lrclib => fetch_lrclib(client, endpoints.lrclib, track, &mut best).await,
            Provider::Netease => {
                chinese::netease(client, endpoints.netease, track, &mut best).await
            }
            Provider::Qq => {
                chinese::qq(
                    client,
                    endpoints.qq_search,
                    endpoints.qq_lyrics,
                    track,
                    &mut best,
                )
                .await
            }
        }
    })
    .await
    .unwrap_or_else(|_| Err(anyhow::anyhow!("Lookup timed out")));
    // A search failure or deadline must not discard an earlier plain result.
    (
        provider,
        best.map_or_else(|| result.map(|()| None), |lyrics| Ok(Some(lyrics))),
    )
}

fn artists(track: &Track) -> Vec<&str> {
    let mut names = vec![track.artists.trim()];
    // Verified credit counts avoid splitting a band such as Earth, Wind & Fire.
    let credits: Vec<_> = track.artists.split(',').map(str::trim).collect();
    if credits.len() > 1 && credits.len() == track.artist_ids.len() && !credits[0].is_empty() {
        names.push(credits[0]);
    }
    names
}

fn key(value: &str) -> String {
    value
        .to_lowercase()
        .nfc()
        .filter(|c| c.is_alphanumeric() || unicode_normalization::char::is_combining_mark(*c))
        .collect()
}

fn same_recording(json: &Value, track: &Track, artist: &str, tolerance: f64) -> bool {
    json["trackName"]
        .as_str()
        .is_some_and(|name| !key(name).is_empty() && key(name) == key(&track.name))
        && json["artistName"]
            .as_str()
            .is_some_and(|name| !key(name).is_empty() && key(name) == key(artist))
        && json["duration"].as_f64().is_some_and(|seconds| {
            seconds.is_finite()
                && seconds > 0.0
                && (seconds - f64::from(track.duration_ms) / 1000.0).abs() <= tolerance
        })
}

fn consider(best: &mut Option<Lyrics>, json: &Value) {
    let lyrics = parse_lyrics(json);
    if !lyrics.is_empty()
        && (best.is_none()
            || (best.as_ref().is_some_and(|saved| saved.lines.is_empty())
                && !lyrics.lines.is_empty()))
    {
        *best = Some(lyrics);
    }
}

fn has_synced(best: &Option<Lyrics>) -> bool {
    best.as_ref().is_some_and(|lyrics| !lyrics.lines.is_empty())
}

async fn fetch_lrclib(
    client: &Client,
    base: &str,
    track: &Track,
    best: &mut Option<Lyrics>,
) -> Result<()> {
    let title: String = track.name.nfc().collect();
    let artists = artists(track);
    let duration = (f64::from(track.duration_ms) / 1000.0).to_string();
    for artist in &artists {
        let artist: String = artist.nfc().collect();
        let query = [
            ("track_name", title.as_str()),
            ("artist_name", artist.as_str()),
            ("duration", duration.as_str()),
        ];
        if let Some(json) = request(client.get(format!("{base}/api/get")).query(&query)).await?
            && same_recording(&json, track, &artist, 2.0)
        {
            consider(best, &json);
            if has_synced(best) {
                return Ok(());
            }
        }
    }

    for artist in &artists {
        let artist: String = artist.nfc().collect();
        let query = [
            ("track_name", title.as_str()),
            ("artist_name", artist.as_str()),
        ];
        let Some(json) = request(client.get(format!("{base}/api/search")).query(&query)).await?
        else {
            continue;
        };
        let results = json
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid LRCLIB search response"))?;
        // Search can return covers and different versions. Require title and
        // artist agreement, retain version words, and allow only 5s of drift.
        let mut matches: Vec<_> = results
            .iter()
            .filter(|json| {
                artists
                    .iter()
                    .any(|name| same_recording(json, track, name, 5.0))
            })
            .collect();
        matches.sort_by(|a, b| {
            let distance = |json: &Value| {
                (json["duration"].as_f64().unwrap_or_default()
                    - f64::from(track.duration_ms) / 1000.0)
                    .abs()
            };
            distance(a).total_cmp(&distance(b)).then_with(|| {
                let album_matches = |json: &Value| {
                    track.album.as_deref().is_some_and(|album| {
                        json["albumName"]
                            .as_str()
                            .is_some_and(|name| key(name) == key(album))
                    })
                };
                album_matches(b).cmp(&album_matches(a))
            })
        });
        for json in matches {
            consider(best, json);
            if has_synced(best) {
                return Ok(());
            }
        }
    }
    Ok(())
}

async fn fetch_ovh(client: &Client, base: &str, track: &Track) -> Result<Option<Lyrics>> {
    for artist in artists(track) {
        let mut url = reqwest::Url::parse(base)?;
        // Encode each segment, including slashes, ? and # in artist/song names.
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Invalid lyrics.ovh service URL"))?
            .pop_if_empty()
            .push("v1")
            .push(&artist.nfc().collect::<String>())
            .push(&track.name.nfc().collect::<String>());
        if let Some(json) = request(client.get(url)).await? {
            let text = json["lyrics"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid lyrics.ovh response"))?;
            // This API returns no timing/recording metadata: display plain text
            // only and never manufacture synchronized timestamps.
            let lyrics = parse_lyrics(&serde_json::json!({"plainLyrics": text}));
            if !lyrics.is_empty() {
                return Ok(Some(lyrics));
            }
        }
    }
    Ok(None)
}

async fn request(builder: RequestBuilder) -> Result<Option<Value>> {
    for attempt in 0..2 {
        let response = builder
            .try_clone()
            .ok_or_else(|| anyhow::anyhow!("Could not prepare lyrics request"))?
            .timeout(REQUEST_TIMEOUT)
            .header(
                "User-Agent",
                concat!(
                    "Tuitify/",
                    env!("CARGO_PKG_VERSION"),
                    " (terminal music player)"
                ),
            )
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Could not reach lyrics service: {}", e.without_url()))?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if matches!(status.as_u16(), 502..=504) {
            let delay = response
                .headers()
                .get("retry-after")
                .map(|value| crate::auth::retry_delay(value.to_str().ok()))
                .unwrap_or(Duration::from_secs(1));
            if attempt == 0 && delay <= Duration::from_secs(2) {
                drop(response);
                tokio::time::sleep(delay).await;
                continue;
            }
            bail!(
                "Lyrics service temporarily unavailable (HTTP {}). Try again shortly",
                status.as_u16()
            );
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            bail!("Lyrics service is busy (HTTP 429). Wait before retrying");
        }
        let response = response
            .error_for_status()
            .map_err(|e| anyhow::anyhow!("Lyrics service request failed: {}", e.without_url()))?;
        return Ok(Some(response.json().await.map_err(|e| {
            anyhow::anyhow!("Invalid lyrics service response: {}", e.without_url())
        })?));
    }
    unreachable!("each request returns or retries once")
}

#[cfg(test)]
mod tests;
