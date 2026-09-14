use crate::{
    auth::{TokenManager, http_client, retry_delay},
    model::{Playlist, Track, track_id, valid_id},
};
use anyhow::{Context, Result, bail};
use futures_util::{Stream, StreamExt};
use serde_json::Value;
use std::sync::atomic::{AtomicU8, Ordering};
use std::{sync::Arc, time::Instant};
use tokio::sync::Mutex;

mod discovery;
mod similarity;
pub(crate) use discovery::recording as recording_key;
#[cfg(test)]
mod discovery_tests;

#[derive(Debug)]
pub struct MissingItem;
impl std::fmt::Display for MissingItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Track or playlist not available to this account. Choose another item.")
    }
}
impl std::error::Error for MissingItem {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Browse {
    Search(String),
    Playlists,
    Liked,
    Playlist(String),
    #[allow(dead_code)]
    Album(String),
    #[allow(dead_code)]
    Artist(String),
}
#[derive(Clone, Debug)]
pub enum Rows {
    Tracks(Vec<Track>),
    Playlists(Vec<Playlist>),
}
#[derive(Clone, Debug)]
pub struct Page {
    pub rows: Rows,
    pub offset: usize,
    pub next: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecommendationSource {
    Spotify,
    ArtistSearch,
    SimilarArtists,
}

impl RecommendationSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify recommendations",
            Self::ArtistSearch => "Artist-connected suggestions",
            Self::SimilarArtists => "Similar artists • Deezer",
        }
    }
}

#[derive(Debug)]
pub struct Recommendations {
    pub tracks: Vec<Track>,
    pub source: RecommendationSource,
}

/// Shared across catalog clones; transient UI messages cannot erase service health.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Health {
    #[default]
    Unknown,
    Ready,
    Unavailable,
    AuthenticationRequired,
}

#[derive(Clone)]
pub struct Catalog {
    client: reqwest::Client,
    tokens: TokenManager,
    base: String,
    cooldown: Arc<Mutex<Option<Instant>>>,
    health: Arc<AtomicU8>,
    offline: bool,
    discovery: Arc<Mutex<std::collections::HashMap<String, discovery::Profile>>>,
    similarity: Option<similarity::Similarity>,
}

impl Catalog {
    pub(crate) fn offline() -> Result<Self> {
        Ok(Self {
            offline: true,
            similarity: None,
            ..Self::new(TokenManager::offline()?)?
        })
    }
    #[cfg(test)]
    pub fn mock(base: &str) -> Self {
        let mut catalog = Self::new(TokenManager::mock(format!("{base}/token"), false)).unwrap();
        catalog.base = base.to_owned();
        catalog.similarity = None;
        catalog
    }

    #[cfg(test)]
    pub(crate) fn mock_with_similarity(base: &str, similarity_base: &str) -> Self {
        let mut catalog = Self::mock(base);
        catalog.similarity = Some(similarity::Similarity::mock(similarity_base));
        catalog
    }

    pub fn new(tokens: TokenManager) -> Result<Self> {
        Ok(Self {
            client: http_client()?,
            tokens,
            base: "https://api.spotify.com/v1".into(),
            cooldown: Arc::new(Mutex::new(None)),
            health: Arc::new(AtomicU8::new(0)),
            offline: false,
            discovery: Arc::new(Mutex::new(std::collections::HashMap::new())),
            similarity: Some(similarity::Similarity::new()?),
        })
    }
    pub fn health(&self) -> Health {
        match self.health.load(Ordering::Relaxed) {
            1 => Health::Ready,
            2 => Health::Unavailable,
            3 => Health::AuthenticationRequired,
            _ => Health::Unknown,
        }
    }

    pub(crate) async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let result = self.request(path, query).await;
        match &result {
            Ok(_) => self.health.store(1, Ordering::Relaxed),
            Err(error) if error.is::<crate::auth::AuthenticationRequired>() => {
                self.health.store(3, Ordering::Relaxed);
            }
            Err(_) => {
                // A network outage does not resolve a known authentication failure.
                if self.health() != Health::AuthenticationRequired {
                    self.health.store(2, Ordering::Relaxed);
                }
            }
        }
        result
    }

    async fn request(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        if self.offline {
            bail!("Offline catalog boundary rejected a network request");
        }
        if let Some(until) = *self.cooldown.lock().await {
            if until > Instant::now() {
                bail!(
                    "Spotify rate limit: wait {} seconds, then press F5 to retry",
                    until.saturating_duration_since(Instant::now()).as_secs() + 1
                );
            }
        }
        let mut token = self.tokens.access().await?;
        for attempt in 0..2 {
            let response = self
                .client
                .get(format!("{}{path}", self.base))
                .query(query)
                .bearer_auth(&token)
                .send()
                .await
                .context("Cannot reach Spotify. Check your connection, then press F5 to retry")?;
            match response.status().as_u16() {
                200 => {
                    return response.json().await.context(
                        "Spotify returned an invalid catalog response; press F5 to retry",
                    );
                }
                401 if attempt == 0 => {
                    token = self.tokens.refresh_rejected(&token).await?;
                }
                401 => return Err(crate::auth::AuthenticationRequired.into()),
                403 => bail!(
                    "Spotify denied access. Playlist items require ownership or collaboration in development mode. Also check app user access, scopes, and the app owner's Premium subscription."
                ),
                404 => return Err(MissingItem.into()),
                429 => {
                    let wait = retry_delay(
                        response
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok()),
                    );
                    *self.cooldown.lock().await = Instant::now().checked_add(wait);
                    bail!(
                        "Spotify rate limit: wait {} seconds, then press F5 to retry",
                        wait.as_secs()
                    );
                }
                status => bail!(
                    "Spotify returned HTTP {status}. Retry later with F5; your queue is preserved."
                ),
            }
        }
        unreachable!()
    }
    pub async fn track(&self, id: &str) -> Result<Track> {
        if !valid_id(id) {
            bail!("Invalid track ID");
        }
        parse_track(&self.get(&format!("/tracks/{id}"), &[]).await?)
            .context("Spotify returned no playable track metadata")
    }
    /// Individual endpoints are required for development-mode Spotify apps.
    /// Only five futures exist at once; yield successes AND errors immediately.
    pub fn tracks(
        &self,
        ids: Vec<String>,
    ) -> impl Stream<Item = (String, Result<Track>)> + Send + 'static {
        let catalog = self.clone();
        futures_util::stream::iter(ids)
            .map(move |id| {
                let catalog = catalog.clone();
                async move {
                    let result = catalog.track(&id).await;
                    (id, result)
                }
            })
            .buffer_unordered(5)
    }
    pub async fn recommendations(&self, seed: &Track) -> Result<Recommendations> {
        let normalized_seed = normalize_title(&seed.name);

        // 1. Try official /v1/recommendations endpoint first
        let rec_query = [("seed_tracks", seed.id.clone()), ("limit", "20".into())];
        if let Ok(value) = self.get("/recommendations", &rec_query).await {
            if let Some(items) = value["tracks"].as_array() {
                let mut tracks = Vec::new();
                let mut seen_titles = std::collections::HashSet::new();
                let mut seen_ids = std::collections::HashSet::new();
                seen_titles.insert(normalized_seed.clone());

                for item in items {
                    if let Some(t) = parse_track(item) {
                        let norm = normalize_title(&t.name);
                        if t.playable
                            && t.id != seed.id
                            && !seen_titles.contains(&norm)
                            && seen_ids.insert(t.id.clone())
                        {
                            seen_titles.insert(norm);
                            tracks.push(t);
                        }
                    }
                }
                // Keep Spotify's ordering, including small recommendation pools.
                if !tracks.is_empty() {
                    return Ok(Recommendations {
                        tracks,
                        source: RecommendationSource::Spotify,
                    });
                }
            }
        }

        self.collaboration_recommendations(seed).await
    }

    /// Smart Shuffle uses supported catalog search rather than the deprecated
    /// recommendations endpoint. Search suggestions are not Spotify personalization.
    pub async fn smart_recommendations(
        &self,
        seed: &Track,
        excluded: &[Track],
        context: &[Track],
    ) -> Result<Recommendations> {
        self.queue_context_recommendations(seed, excluded, context)
            .await
    }

    async fn collaboration_recommendations(&self, seed: &Track) -> Result<Recommendations> {
        self.radio_recommendations(seed, &[], 0).await
    }

    pub async fn page(&self, browse: &Browse, offset: usize) -> Result<Page> {
        if let Browse::Search(query) = browse {
            if let Some(id) = track_id(query) {
                return Ok(Page {
                    rows: Rows::Tracks(vec![self.track(&id).await?]),
                    offset: 0,
                    next: None,
                });
            }
            if query.trim().is_empty() {
                return Ok(Page {
                    rows: Rows::Tracks(vec![]),
                    offset: 0,
                    next: None,
                });
            }
        }
        if let Browse::Album(id) = browse {
            if !valid_id(id) {
                bail!("Invalid album ID");
            }
            let query = [("limit", "50".to_string()), ("offset", offset.to_string())];
            let value = self.get(&format!("/albums/{id}/tracks"), &query).await?;
            let items = value["items"]
                .as_array()
                .context("Spotify omitted catalog items; press F5 to retry")?;
            let mut tracks = Vec::new();
            for item in items {
                if let Some(mut track) = parse_track(item) {
                    track.album_id = Some(id.clone());
                    if let Some(num) = item["track_number"].as_u64() {
                        track.track_number = Some(num as u32);
                    }
                    tracks.push(track);
                }
            }
            let next = if !value["next"].is_null()
                && value.get("next").is_some()
                && value["next"].as_str() != Some("")
            {
                Some(offset + 50)
            } else {
                None
            };
            return Ok(Page {
                rows: Rows::Tracks(tracks),
                offset,
                next,
            });
        }
        if let Browse::Artist(id) = browse {
            if !valid_id(id) {
                bail!("Invalid artist ID");
            }
            let query = [("market", "from_token".to_string())];
            match self.get(&format!("/artists/{id}/top-tracks"), &query).await {
                Ok(value) => {
                    let tracks = value["tracks"]
                        .as_array()
                        .context("Spotify omitted catalog items; press F5 to retry")?
                        .iter()
                        .filter_map(parse_track)
                        .collect();
                    return Ok(Page {
                        rows: Rows::Tracks(tracks),
                        offset,
                        next: None,
                    });
                }
                Err(err) if !err.is::<MissingItem>() => {
                    // In Development Mode, Spotify rejects /artists/{id}/top-tracks with 403 Forbidden.
                    // Fall back to resolving the artist's name and searching their popular tracks.
                    if let Ok(info) = self.get(&format!("/artists/{id}"), &[]).await {
                        if let Some(name) = info["name"].as_str() {
                            let search_query = [
                                ("type", "track".to_string()),
                                ("q", format!("artist:\"{name}\"")),
                                ("limit", "10".to_string()),
                                ("offset", offset.to_string()),
                            ];
                            if let Ok(search_val) = self.get("/search", &search_query).await {
                                if let Some(items) = search_val["tracks"]["items"].as_array() {
                                    let tracks: Vec<Track> =
                                        items.iter().filter_map(parse_track).collect();
                                    let next = if !search_val["tracks"]["next"].is_null()
                                        && search_val["tracks"].get("next").is_some()
                                        && search_val["tracks"]["next"].as_str() != Some("")
                                    {
                                        Some(offset + 10)
                                    } else {
                                        None
                                    };
                                    return Ok(Page {
                                        rows: Rows::Tracks(tracks),
                                        offset,
                                        next,
                                    });
                                }
                            }
                        }
                    }
                    return Err(err);
                }
                Err(err) => return Err(err),
            }
        }
        let limit = if matches!(browse, Browse::Search(_)) {
            10
        } else {
            50
        };
        let mut query = vec![("limit", limit.to_string()), ("offset", offset.to_string())];
        let path = match browse {
            Browse::Search(q) => {
                query.extend([("type", "track".into()), ("q", q.clone())]);
                "/search".into()
            }
            Browse::Playlists => "/me/playlists".into(),
            Browse::Liked => "/me/tracks".into(),
            Browse::Playlist(id) => {
                if !valid_id(id) {
                    bail!("Invalid playlist ID");
                }
                format!("/playlists/{id}/items")
            }
            Browse::Album(_) | Browse::Artist(_) => unreachable!("handled above"),
        };
        let value = self.get(&path, &query).await?;
        let page = if matches!(browse, Browse::Search(_)) {
            &value["tracks"]
        } else {
            &value
        };
        let Some(items) = page["items"].as_array() else {
            if matches!(browse, Browse::Playlist(_)) {
                bail!(
                    "Playlist contents are restricted to owners or collaborators in Spotify development mode; this account can only see its metadata."
                );
            }
            bail!("Spotify omitted catalog items; press F5 to retry");
        };
        let next = page["next"]
            .as_str()
            .filter(|s| !s.is_empty())
            .and_then(|_| offset.checked_add(limit));
        let rows = if matches!(browse, Browse::Playlists) {
            Rows::Playlists(
                items
                    .iter()
                    .filter_map(|v| {
                        Some(Playlist {
                            id: v["id"].as_str().filter(|id| valid_id(id))?.into(),
                            name: clean(v["name"].as_str().unwrap_or("Untitled playlist")),
                            owner: clean(
                                v["owner"]["display_name"]
                                    .as_str()
                                    .unwrap_or("Spotify user"),
                            ),
                        })
                    })
                    .collect(),
            )
        } else {
            Rows::Tracks(
                items
                    .iter()
                    .filter_map(|v| {
                        let track = match browse {
                            Browse::Liked => &v["track"],
                            Browse::Playlist(_) => v
                                .get("item")
                                .or_else(|| v.get("track"))
                                .unwrap_or(&Value::Null),
                            _ => v,
                        };
                        parse_track(track)
                    })
                    .collect(),
            )
        };
        Ok(Page { rows, offset, next })
    }
}

pub fn normalize_title(name: &str) -> String {
    let lower = name.to_lowercase();
    let base = lower
        .split(" - ")
        .next()
        .unwrap_or(&lower)
        .split('(')
        .next()
        .unwrap_or(&lower)
        .split('[')
        .next()
        .unwrap_or(&lower);
    base.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}
pub(crate) fn parse_track(v: &Value) -> Option<Track> {
    if v["is_local"].as_bool() == Some(true) || v["type"].as_str().is_some_and(|t| t != "track") {
        return None;
    }
    let album = v["album"]["name"].as_str().map(clean);
    let album_art_url = v["album"]["images"]
        .as_array()
        .and_then(|imgs| imgs.first())
        .and_then(|img| img["url"].as_str())
        .map(clean);
    let album_id = v["album"]["id"]
        .as_str()
        .filter(|id| valid_id(id))
        .map(str::to_owned);
    let track_number = v["track_number"].as_u64().map(|n| n as u32);

    Some(Track {
        id: v["id"].as_str().filter(|id| valid_id(id))?.into(),
        name: clean(v["name"].as_str().unwrap_or("Unknown track")),
        artists: v["artists"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|a| a["name"].as_str())
                    .map(clean)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
        artist_ids: v["artists"]
            .as_array()
            .map(|artists| {
                artists
                    .iter()
                    .filter_map(|artist| artist["id"].as_str())
                    .filter(|id| valid_id(id))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        duration_ms: v["duration_ms"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32,
        playable: v["is_playable"].as_bool().unwrap_or(true)
            && v.get("restrictions").is_none_or(|r| r.is_null()),
        album,
        album_art_url,
        album_id,
        track_number,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path, query_param},
    };
    async fn catalog() -> (MockServer, Catalog) {
        let server = MockServer::start().await;
        let mut c =
            Catalog::new(TokenManager::mock(format!("{}/token", server.uri()), false)).unwrap();
        c.base = server.uri();
        c.similarity = None;
        (server, c)
    }
    fn track() -> Value {
        serde_json::json!({"id":"0000000000000000000001","name":"Example","artists":[{"name":"Artist"}],"type":"track","duration_ms":200000})
    }

    #[tokio::test]
    #[ignore = "Uses the saved Spotify login and live catalog; manual acceptance test"]
    async fn live_radio_multiple_seeds() -> Result<()> {
        let store = crate::storage::Storage::local()?;
        let catalog = Catalog::new(TokenManager::load(&store.config()?)?)?;
        for query in [
            "track:good 4 u artist:Olivia Rodrigo",
            "track:Blinding Lights artist:The Weeknd",
            "track:Yellow artist:Coldplay",
            "track:One More Time artist:Daft Punk",
        ] {
            let page = catalog.page(&Browse::Search(query.into()), 0).await?;
            let Rows::Tracks(tracks) = page.rows else {
                bail!("Expected tracks")
            };
            let seed = tracks
                .into_iter()
                .find(|track| track.playable)
                .context("No playable seed")?;
            println!("SEED {} - {} | id={}", seed.name, seed.artists, seed.id);
            let batch = catalog.radio_recommendations(&seed, &[], 0).await?;
            assert!(
                batch.tracks.len() >= 12,
                "{}: sparse recommendation pool",
                seed.name
            );
            println!(
                "LIVE count={} source={}",
                batch.tracks.len(),
                batch.source.label()
            );
            for track in &batch.tracks {
                println!("  {} - {}", track.name, track.artists);
            }
        }
        Ok(())
    }

    #[tokio::test]
    #[ignore = "Requires the locally saved Spotify catalog login; manual acceptance test"]
    async fn live_recommendations_from_saved_account() -> Result<()> {
        let store = crate::storage::Storage::local()?;
        let config = store.config()?;
        let catalog = Catalog::new(TokenManager::load(&config)?)?;
        let seed = catalog.track("6PCUP3dWmTjcTtXY02oFdT").await?;
        let recommendations = catalog.recommendations(&seed).await?;

        println!(
            "LIVE RADIO: {} - {} | source={} | count={}",
            seed.name,
            seed.artists,
            recommendations.source.label(),
            recommendations.tracks.len()
        );
        for (index, track) in recommendations.tracks.iter().enumerate() {
            println!("{:>2}. {} - {}", index + 1, track.name, track.artists);
        }
        Ok(())
    }
    #[tokio::test]
    async fn health_tracks_authentication_and_recovery_across_clones() {
        let (server, catalog) = catalog().await;
        let observer = catalog.clone();
        Mock::given(path("/me/tracks"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        Mock::given(path("/token"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&server)
            .await;
        let error = catalog.page(&Browse::Liked, 0).await.unwrap_err();
        assert!(error.is::<crate::auth::AuthenticationRequired>());
        assert_eq!(observer.health(), Health::AuthenticationRequired);

        server.reset().await;
        Mock::given(path("/me/tracks"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        assert!(catalog.page(&Browse::Liked, 0).await.is_err());
        assert_eq!(observer.health(), Health::AuthenticationRequired);
        let fresh = Catalog::mock(&server.uri());
        assert!(fresh.page(&Browse::Liked, 0).await.is_err());
        assert_eq!(fresh.health(), Health::Unavailable);

        server.reset().await;
        Mock::given(path("/me/tracks"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"items": [{"track": track()}]})),
            )
            .mount(&server)
            .await;
        assert!(catalog.page(&Browse::Liked, 0).await.is_ok());
        assert_eq!(observer.health(), Health::Ready);
    }
    #[test]
    fn album_metadata_is_optional_and_old_cached_tracks_still_load() {
        let mut value = track();
        let parsed = parse_track(&value).unwrap();
        assert!(parsed.album.is_none() && parsed.album_art_url.is_none());
        let mut old = serde_json::to_value(parsed).unwrap();
        old.as_object_mut().unwrap().remove("album");
        old.as_object_mut().unwrap().remove("album_art_url");
        assert!(
            serde_json::from_value::<Track>(old)
                .unwrap()
                .album
                .is_none()
        );
        value["album"] = serde_json::json!({"name":"Album", "images":[{"url":"https://i.scdn.co/image/example"}]});
        let parsed = parse_track(&value).unwrap();
        assert_eq!(parsed.album.as_deref(), Some("Album"));
        assert_eq!(
            parsed.album_art_url.as_deref(),
            Some("https://i.scdn.co/image/example")
        );
    }
    #[tokio::test]
    async fn metadata_stream_reports_errors_and_yields_without_waiting_for_slow_tracks() {
        let (server, catalog) = catalog().await;
        Mock::given(path("/tracks/0000000000000000000001"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(track())
                    .set_delay(std::time::Duration::from_secs(2)),
            )
            .mount(&server)
            .await;
        Mock::given(path("/tracks/0000000000000000000002"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let mut stream = catalog.tracks(vec![
            "0000000000000000000001".into(),
            "0000000000000000000002".into(),
        ]);
        let (id, result) = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap();
        assert!(id.ends_with('2'));
        assert!(result.unwrap_err().to_string().contains("503"));
    }
    #[tokio::test]
    async fn search_uses_ten_and_offsets() {
        let (s, c) = catalog().await;
        for offset in [0, 10] {
            Mock::given(method("GET")).and(path("/search")).and(query_param("limit","10")).and(query_param("offset",offset.to_string())).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"tracks":{"items":[track()],"next":if offset == 0 { Some("next") } else { None }}}))).expect(1).mount(&s).await;
        }
        let p = c.page(&Browse::Search("test".into()), 0).await.unwrap();
        assert_eq!(p.next, Some(10));
        assert!(
            c.page(&Browse::Search("test".into()), p.next.unwrap())
                .await
                .unwrap()
                .next
                .is_none()
        );
    }
    #[tokio::test]
    async fn playlist_items_new_shape_and_restriction() {
        let (s, c) = catalog().await;
        Mock::given(path("/playlists/0000000000000000000001/items")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"items":[{"item":track()},{"item":null},{"item":{"type":"episode"}}],"next":null}))).mount(&s).await;
        let p = c
            .page(&Browse::Playlist("0000000000000000000001".into()), 0)
            .await
            .unwrap();
        assert!(matches!(p.rows,Rows::Tracks(t) if t.len()==1));
        Mock::given(path("/playlists/0000000000000000000002/items"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"name":"restricted"})),
            )
            .mount(&s)
            .await;
        assert!(
            c.page(&Browse::Playlist("0000000000000000000002".into()), 0)
                .await
                .unwrap_err()
                .to_string()
                .contains("collaborators")
        );
    }
    #[tokio::test]
    async fn rate_limit_blocks_further_requests() {
        let (s, c) = catalog().await;
        Mock::given(path("/me/tracks"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "120"))
            .expect(1)
            .mount(&s)
            .await;
        assert!(
            c.page(&Browse::Liked, 0)
                .await
                .unwrap_err()
                .to_string()
                .contains("120")
        );
        assert!(c.page(&Browse::Liked, 0).await.is_err());
    }
    #[tokio::test]
    async fn unauthorized_refreshes_once() {
        let (s, c) = catalog().await;
        Mock::given(path("/me/tracks"))
            .and(header("authorization", "Bearer old"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"access_token":"new","expires_in":3600})),
            )
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(path("/me/tracks"))
            .and(header("authorization", "Bearer new"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"items":[{"track":track()}]})),
            )
            .expect(1)
            .mount(&s)
            .await;
        assert!(c.page(&Browse::Liked, 0).await.is_ok());
    }
    #[tokio::test]
    async fn forbidden_and_server_errors_do_not_retry() {
        let (s, c) = catalog().await;
        Mock::given(path("/me/playlists"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&s)
            .await;
        assert!(
            c.page(&Browse::Playlists, 0)
                .await
                .unwrap_err()
                .to_string()
                .contains("Premium")
        );
        Mock::given(path("/me/tracks"))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&s)
            .await;
        assert!(
            c.page(&Browse::Liked, 0)
                .await
                .unwrap_err()
                .to_string()
                .contains("503")
        );
    }
    #[test]
    fn test_normalize_title() {
        assert_eq!(normalize_title("Castle on the Hill"), "castle on the hill");
        assert_eq!(
            normalize_title("Castle on the Hill - Acoustic"),
            "castle on the hill"
        );
        assert_eq!(
            normalize_title("Castle on the Hill (Vic Carnes)"),
            "castle on the hill"
        );
        assert_eq!(
            normalize_title("Castle on the Hill [Live]"),
            "castle on the hill"
        );
        assert_eq!(normalize_title("Shape of You"), "shape of you");
        assert_ne!(
            normalize_title("Castle on the Hill"),
            normalize_title("Shape of You")
        );
    }
    #[tokio::test]
    async fn smart_recommendations_filter_same_song_without_deprecated_endpoint() {
        let (s, c) = catalog().await;
        // Smart Shuffle must not probe the deprecated recommendations endpoint.
        Mock::given(path("/recommendations"))
            .respond_with(ResponseTemplate::new(403))
            .expect(0)
            .mount(&s)
            .await;
        // Mock search returning candidate tracks including seed, acoustic duplicate, and new related track
        let item1 = serde_json::json!({
            "id": "0000000000000000000001",
            "name": "Castle on the Hill",
            "artists": [{"name": "Ed Sheeran"}],
            "duration_ms": 261000,
            "is_playable": true,
            "type": "track"
        });
        let item2 = serde_json::json!({
            "id": "0000000000000000000002",
            "name": "Castle on the Hill - Acoustic",
            "artists": [{"name": "Ed Sheeran"}],
            "duration_ms": 220000,
            "is_playable": true,
            "type": "track"
        });
        let item3 = serde_json::json!({
            "id": "0000000000000000000003",
            "name": "Shape of You",
            "artists": [{"name": "Ed Sheeran"}],
            "duration_ms": 233000,
            "is_playable": true,
            "type": "track"
        });
        Mock::given(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tracks": {
                    "items": [item1, item2, item3]
                }
            })))
            .mount(&s)
            .await;

        let seed = Track {
            id: "0000000000000000000001".into(),
            name: "Castle on the Hill".into(),
            artists: "Ed Sheeran".into(),
            duration_ms: 261000,
            playable: true,
            ..Default::default()
        };
        let recs = c.smart_recommendations(&seed, &[], &[]).await.unwrap();
        // The seed and the acoustic variant MUST be filtered out; only Shape of You remains!
        assert_eq!(recs.source, RecommendationSource::ArtistSearch);
        assert_eq!(recs.tracks.len(), 1);
        assert_eq!(recs.tracks[0].name, "Shape of You");
        assert_eq!(recs.tracks[0].id, "0000000000000000000003");
    }

    #[tokio::test]
    async fn recommendations_fallback_uses_collaborators_without_genre_search() {
        let (server, catalog) = catalog().await;
        Mock::given(path("/recommendations"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;

        let track = |id: usize, name: &str, artists: Vec<(&str, usize)>| {
            serde_json::json!({
                "id": format!("{id:022}"),
                "name": name,
                "artists": artists
                    .into_iter()
                    .map(|(artist, artist_id)| serde_json::json!({
                        "name": artist,
                        "id": format!("{artist_id:022}")
                    }))
                    .collect::<Vec<_>>(),
                "duration_ms": 200000,
                "is_playable": true,
                "type": "track"
            })
        };

        Mock::given(path("/search"))
            .and(query_param("q", "artist:\"Seed Artist\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tracks": {"items": [
                    track(2, "Seed with B", vec![("Seed Artist", 100), ("Artist B", 200)]),
                    track(3, "Seed with C", vec![("Seed Artist", 100), ("Artist C", 300)]),
                    track(4, "Seed Solo", vec![("Seed Artist", 100)]),
                    track(13, "Second B duet", vec![("Seed Artist", 100), ("Artist B", 200)]),
                    track(14, "Second C duet", vec![("Seed Artist", 100), ("Artist C", 300)])
                ]}
            })))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(path("/search"))
            .and(query_param("q", "artist:\"Artist B\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tracks": {"items": [
                    track(5, "B with D", vec![("Artist B", 200), ("Artist D", 400)]),
                    track(6, "B with E", vec![("Artist B", 200), ("Artist E", 500)])
                ]}
            })))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(path("/search"))
            .and(query_param("q", "artist:\"Artist C\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tracks": {"items": [
                    track(7, "C with F", vec![("Artist C", 300), ("Artist F", 600)]),
                    track(8, "C with G", vec![("Artist C", 300), ("Artist G", 700)])
                ]}
            })))
            .expect(2)
            .mount(&server)
            .await;
        for (artist, artist_id, track_id) in [
            ("Artist D", 400, 9),
            ("Artist E", 500, 10),
            ("Artist F", 600, 11),
            ("Artist G", 700, 12),
        ] {
            Mock::given(path("/search"))
                .and(query_param("q", format!("artist:\"{artist}\"")))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "tracks": {"items": [track(
                        track_id,
                        &format!("{artist} Solo"),
                        vec![(artist, artist_id)]
                    )]}
                })))
                .expect(0)
                .mount(&server)
                .await;
        }
        Mock::given(path("/search"))
            .and(query_param("q", "genre:\"pop\""))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;

        Mock::given(path(format!("/artists/{:022}/albums", 100)))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"items":[]})))
            .expect(1)
            .mount(&server)
            .await;
        let recs = catalog
            .recommendations(&Track {
                id: "0000000000000000000001".into(),
                name: "Seed Song".into(),
                artists: "Seed Artist".into(),
                artist_ids: vec!["0000000000000000000100".into()],
                duration_ms: 200000,
                playable: true,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(recs.source, RecommendationSource::ArtistSearch);
        assert!(
            recs.tracks
                .iter()
                .any(|track| track.artists == "Artist B, Artist D")
        );
        assert!(
            recs.tracks
                .iter()
                .any(|track| track.artists == "Artist C, Artist F")
        );
        assert!(
            recs.tracks.len() > 5,
            "repeated direct collaborations should add candidates without a second hop"
        );
    }

    #[tokio::test]
    async fn recommendations_preserve_small_spotify_pool_and_order() {
        let (server, catalog) = catalog().await;
        let item = |id: usize, name: &str| {
            serde_json::json!({
                "id": format!("{id:022}"), "name": name,
                "artists": [{"name": "Artist"}], "is_playable": true,
                "duration_ms": 200000, "type": "track"
            })
        };
        Mock::given(path("/recommendations"))
            .and(query_param("seed_tracks", format!("{:022}", 1)))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tracks": [item(3, "Zulu"), item(2, "Alpha"), item(3, "Duplicate"), item(1, "Seed")]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/search"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let result = catalog
            .recommendations(&Track {
                id: format!("{:022}", 1),
                name: "Seed".into(),
                artists: "Artist".into(),
                playable: true,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(result.source, RecommendationSource::Spotify);
        assert_eq!(
            result
                .tracks
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Zulu", "Alpha"]
        );
    }
    #[test]
    fn spotify_links() {
        assert_eq!(
            track_id("https://open.spotify.com/intl-en/track/0000000000000000000001?si=x").unwrap(),
            "0000000000000000000001"
        );
        assert!(track_id("https://evil.com/track/0000000000000000000001").is_none());
        assert!(track_id("https://open.spotify.com/track/bad").is_none());
    }

    #[tokio::test]
    async fn album_tracks_pagination_and_deserialization() {
        let (server, catalog) = catalog().await;
        let album_id = "0000000000000000000001";

        let track1 = serde_json::json!({
            "id": "1111111111111111111111",
            "name": "Track 1",
            "artists": [{"id": "9999999999999999999999", "name": "Artist 1"}],
            "track_number": 1,
            "duration_ms": 180000,
            "type": "track",
            "is_playable": true
        });
        let track2 = serde_json::json!({
            "id": "2222222222222222222222",
            "name": "Track 2",
            "artists": [{"id": "9999999999999999999999", "name": "Artist 1"}],
            "track_number": 2,
            "duration_ms": 200000,
            "type": "track",
            "is_playable": true
        });
        let track3 = serde_json::json!({
            "id": "3333333333333333333333",
            "name": "Track 3",
            "artists": [{"id": "9999999999999999999999", "name": "Artist 1"}],
            "track_number": 3,
            "duration_ms": 220000,
            "type": "track",
            "is_playable": true
        });

        Mock::given(method("GET"))
            .and(path(format!("/albums/{album_id}/tracks")))
            .and(query_param("limit", "50"))
            .and(query_param("offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [track1, track2],
                "limit": 50,
                "offset": 0,
                "total": 3,
                "next": format!("{}/albums/{album_id}/tracks?limit=50&offset=50", server.uri())
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/albums/{album_id}/tracks")))
            .and(query_param("limit", "50"))
            .and(query_param("offset", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [track3],
                "limit": 50,
                "offset": 50,
                "total": 3,
                "next": null
            })))
            .expect(1)
            .mount(&server)
            .await;

        let page1 = catalog
            .page(&Browse::Album(album_id.into()), 0)
            .await
            .unwrap();
        assert_eq!(page1.offset, 0);
        assert_eq!(page1.next, Some(50));
        match page1.rows {
            Rows::Tracks(tracks) => {
                assert_eq!(tracks.len(), 2);
                assert_eq!(tracks[0].id, "1111111111111111111111");
                assert_eq!(tracks[0].name, "Track 1");
                assert_eq!(tracks[0].album_id.as_deref(), Some(album_id));
                assert_eq!(tracks[0].track_number, Some(1));
                assert_eq!(tracks[1].id, "2222222222222222222222");
                assert_eq!(tracks[1].album_id.as_deref(), Some(album_id));
                assert_eq!(tracks[1].track_number, Some(2));
            }
            Rows::Playlists(_) => panic!("expected tracks"),
        }

        let page2 = catalog
            .page(&Browse::Album(album_id.into()), 50)
            .await
            .unwrap();
        assert_eq!(page2.offset, 50);
        assert_eq!(page2.next, None);
        match page2.rows {
            Rows::Tracks(tracks) => {
                assert_eq!(tracks.len(), 1);
                assert_eq!(tracks[0].id, "3333333333333333333333");
                assert_eq!(tracks[0].album_id.as_deref(), Some(album_id));
                assert_eq!(tracks[0].track_number, Some(3));
            }
            Rows::Playlists(_) => panic!("expected tracks"),
        }
    }

    #[tokio::test]
    async fn artist_top_tracks_deserialization() {
        let (server, catalog) = catalog().await;
        let artist_id = "0000000000000000000002";
        let album_id = "0000000000000000000003";

        let full_track = serde_json::json!({
            "id": "4444444444444444444444",
            "name": "Hit Song",
            "artists": [{"id": artist_id, "name": "Top Artist"}],
            "album": {
                "id": album_id,
                "name": "Greatest Hits",
                "images": [{"url": "https://i.scdn.co/image/hit"}]
            },
            "track_number": 5,
            "duration_ms": 210000,
            "type": "track",
            "is_playable": true
        });

        Mock::given(method("GET"))
            .and(path(format!("/artists/{artist_id}/top-tracks")))
            .and(query_param("market", "from_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tracks": [full_track]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let page = catalog
            .page(&Browse::Artist(artist_id.into()), 0)
            .await
            .unwrap();
        assert_eq!(page.offset, 0);
        assert_eq!(page.next, None);
        match page.rows {
            Rows::Tracks(tracks) => {
                assert_eq!(tracks.len(), 1);
                assert_eq!(tracks[0].id, "4444444444444444444444");
                assert_eq!(tracks[0].name, "Hit Song");
                assert_eq!(tracks[0].album.as_deref(), Some("Greatest Hits"));
                assert_eq!(tracks[0].album_id.as_deref(), Some(album_id));
                assert_eq!(tracks[0].track_number, Some(5));
                assert_eq!(
                    tracks[0].album_art_url.as_deref(),
                    Some("https://i.scdn.co/image/hit")
                );
            }
            Rows::Playlists(_) => panic!("expected tracks"),
        }
    }

    #[tokio::test]
    async fn album_tracks_rate_limit_backoff() {
        let (server, catalog) = catalog().await;
        let album_id = "0000000000000000000001";

        Mock::given(method("GET"))
            .and(path(format!("/albums/{album_id}/tracks")))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "45"))
            .expect(1)
            .mount(&server)
            .await;

        let err = catalog
            .page(&Browse::Album(album_id.into()), 0)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("45"));

        // Subsequent request immediately rejected by cooldown
        let cooldown_err = catalog
            .page(&Browse::Album(album_id.into()), 0)
            .await
            .unwrap_err();
        assert!(cooldown_err.to_string().contains("Spotify rate limit"));
    }

    #[tokio::test]
    async fn artist_top_tracks_missing_item_404() {
        let (server, catalog) = catalog().await;
        let artist_id = "0000000000000000000002";

        Mock::given(method("GET"))
            .and(path(format!("/artists/{artist_id}/top-tracks")))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;

        let err = catalog
            .page(&Browse::Artist(artist_id.into()), 0)
            .await
            .unwrap_err();
        assert!(err.is::<MissingItem>());
    }

    #[tokio::test]
    async fn album_and_artist_invalid_id_rejected() {
        let (server, catalog) = catalog().await;

        // Invalid album IDs
        let err_album = catalog
            .page(&Browse::Album("too_short".into()), 0)
            .await
            .unwrap_err();
        assert!(err_album.to_string().contains("Invalid album ID"));

        // Invalid artist IDs
        let err_artist = catalog
            .page(&Browse::Artist("invalid_chars_here!!".into()), 0)
            .await
            .unwrap_err();
        assert!(err_artist.to_string().contains("Invalid artist ID"));

        // Verify zero mock server requests were made
        assert_eq!(server.received_requests().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn artist_top_tracks_dev_mode_403_falls_back_to_search() {
        let (server, catalog) = catalog().await;
        let artist_id = "0000000000000000000003";

        // /artists/{id}/top-tracks returns 403 in Development Mode
        Mock::given(method("GET"))
            .and(path(format!("/artists/{artist_id}/top-tracks")))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;

        // Fallback queries /artists/{id} for the artist name
        Mock::given(method("GET"))
            .and(path(format!("/artists/{artist_id}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "Radiohead"
            })))
            .expect(1)
            .mount(&server)
            .await;

        // Fallback then queries /search for artist's tracks
        Mock::given(method("GET"))
            .and(path("/search"))
            .and(query_param("type", "track"))
            .and(query_param("q", "artist:\"Radiohead\""))
            .and(query_param("limit", "10"))
            .and(query_param("offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tracks": {
                    "items": [{
                        "id": "3333333333333333333333",
                        "name": "Creep",
                        "is_playable": true,
                        "artists": [{"id": artist_id, "name": "Radiohead"}],
                        "album": {
                            "id": "1111111111111111111111",
                            "name": "Pablo Honey",
                            "images": []
                        },
                        "duration_ms": 238640,
                        "track_number": 2
                    }],
                    "next": null
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let page = catalog
            .page(&Browse::Artist(artist_id.into()), 0)
            .await
            .unwrap();
        assert_eq!(page.offset, 0);
        assert_eq!(page.next, None);
        match page.rows {
            Rows::Tracks(tracks) => {
                assert_eq!(tracks.len(), 1);
                assert_eq!(tracks[0].name, "Creep");
                assert_eq!(tracks[0].artists, "Radiohead");
            }
            Rows::Playlists(_) => panic!("expected tracks"),
        }
    }
}
