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
}

impl RecommendationSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify recommendations",
            Self::ArtistSearch => "Artist-search suggestions",
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
}

impl Catalog {
    #[cfg(test)]
    pub fn mock(base: &str) -> Self {
        let mut catalog = Self::new(TokenManager::mock(format!("{base}/token"), false)).unwrap();
        catalog.base = base.to_owned();
        catalog
    }

    pub fn new(tokens: TokenManager) -> Result<Self> {
        Ok(Self {
            client: http_client()?,
            tokens,
            base: "https://api.spotify.com/v1".into(),
            cooldown: Arc::new(Mutex::new(None)),
            health: Arc::new(AtomicU8::new(0)),
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

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
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

        // 2. Fallback: Query Spotify Search for related tracks & artist catalog
        let artist_names: Vec<&str> = seed
            .artists
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        let primary_artist = artist_names.first().copied().unwrap_or(&seed.artists);

        let mut queries = Vec::new();
        queries.push(primary_artist.to_string());
        if let Some(&second_artist) = artist_names.get(1) {
            queries.push(second_artist.to_string());
        }

        let mut candidates = Vec::new();
        let mut search_error = None;
        for q in queries {
            for offset in [0, 10] {
                let search_query = [
                    ("limit", "10".into()),
                    ("offset", offset.to_string()),
                    ("type", "track".into()),
                    ("q", q.clone()),
                ];
                match self.get("/search", &search_query).await {
                    Ok(val) => {
                        if let Some(items) = val["tracks"]["items"].as_array() {
                            for item in items {
                                if let Some(t) = parse_track(item) {
                                    candidates.push(t);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        search_error = Some(e);
                        break;
                    }
                }
            }
            if search_error.is_some() {
                break;
            }
        }
        if candidates.is_empty() {
            if let Some(e) = search_error {
                return Err(e);
            }
        }

        let mut final_tracks = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        let mut seen_titles = std::collections::HashSet::new();
        seen_ids.insert(seed.id.clone());
        seen_titles.insert(normalized_seed);

        for t in candidates {
            let norm = normalize_title(&t.name);
            if t.playable && !seen_ids.contains(&t.id) && !seen_titles.contains(&norm) {
                seen_ids.insert(t.id.clone());
                seen_titles.insert(norm);
                final_tracks.push(t);
                if final_tracks.len() >= 15 {
                    break;
                }
            }
        }

        Ok(Recommendations {
            tracks: final_tracks,
            source: RecommendationSource::ArtistSearch,
        })
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
fn parse_track(v: &Value) -> Option<Track> {
    if v["is_local"].as_bool() == Some(true) || v["type"].as_str().is_some_and(|t| t != "track") {
        return None;
    }
    let album = v["album"]["name"].as_str().map(clean);
    let album_art_url = v["album"]["images"]
        .as_array()
        .and_then(|imgs| imgs.first())
        .and_then(|img| img["url"].as_str())
        .map(clean);

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
        duration_ms: v["duration_ms"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32,
        playable: v["is_playable"].as_bool().unwrap_or(true)
            && v.get("restrictions").is_none_or(|r| r.is_null()),
        album,
        album_art_url,
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
        (server, c)
    }
    fn track() -> Value {
        serde_json::json!({"id":"0000000000000000000001","name":"Example","artists":[{"name":"Artist"}],"type":"track","duration_ms":200000})
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
    async fn recommendations_fallback_filters_same_song() {
        let (s, c) = catalog().await;
        // Mock 403 on /recommendations to trigger search fallback
        Mock::given(path("/recommendations"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
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
        let recs = c.recommendations(&seed).await.unwrap();
        // The seed and the acoustic variant MUST be filtered out; only Shape of You remains!
        assert_eq!(recs.source, RecommendationSource::ArtistSearch);
        assert_eq!(recs.tracks.len(), 1);
        assert_eq!(recs.tracks[0].name, "Shape of You");
        assert_eq!(recs.tracks[0].id, "0000000000000000000003");
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
}
