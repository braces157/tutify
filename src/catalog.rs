use crate::{
    auth::{TokenManager, http_client, retry_delay},
    model::{Playlist, Track, track_id, valid_id},
};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{sync::Arc, time::Instant};
use tokio::sync::Mutex;

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

#[derive(Clone)]
pub struct Catalog {
    client: reqwest::Client,
    tokens: TokenManager,
    base: String,
    cooldown: Arc<Mutex<Option<Instant>>>,
}

impl Catalog {
    pub fn new(tokens: TokenManager) -> Result<Self> {
        Ok(Self {
            client: http_client()?,
            tokens,
            base: "https://api.spotify.com/v1".into(),
            cooldown: Arc::new(Mutex::new(None)),
        })
    }
    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        if let Some(until) = *self.cooldown.lock().await {
            if until > Instant::now() {
                bail!(
                    "Spotify rate limit: wait {} seconds, then press F5 to retry",
                    until.duration_since(Instant::now()).as_secs() + 1
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
                401 => bail!("Spotify login expired; exit and run tuitify auth --force"),
                403 => bail!(
                    "Spotify denied access. Playlist items require ownership or collaboration in development mode. Also check app user access, scopes, and the app owner's Premium subscription."
                ),
                404 => {
                    bail!("Track or playlist not available to this account. Choose another item.")
                }
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
    pub async fn tracks(&self, ids: &[String]) -> Result<Vec<Track>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let valid: Vec<String> = ids.iter().filter(|id| valid_id(id)).cloned().collect();
        if valid.is_empty() {
            return Ok(vec![]);
        }

        // Fetch tracks concurrently using individual /tracks/{id} endpoints.
        // Spotify's batch /v1/tracks?ids=... endpoint returns HTTP 403 Forbidden for apps in Development Mode,
        // but individual /v1/tracks/{id} endpoints are fully supported (HTTP 200 OK).
        let mut set = tokio::task::JoinSet::new();
        for id in valid {
            let cat = self.clone();
            set.spawn(async move { cat.track(&id).await });
        }
        let mut tracks = Vec::new();
        while let Some(res) = set.join_next().await {
            if let Ok(Ok(track)) = res {
                tracks.push(track);
            }
        }
        Ok(tracks)
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

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}
fn parse_track(v: &Value) -> Option<Track> {
    if v["is_local"].as_bool() == Some(true) || v["type"].as_str().is_some_and(|t| t != "track") {
        return None;
    }
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
    fn spotify_links() {
        assert_eq!(
            track_id("https://open.spotify.com/intl-en/track/0000000000000000000001?si=x").unwrap(),
            "0000000000000000000001"
        );
        assert!(track_id("https://evil.com/track/0000000000000000000001").is_none());
        assert!(track_id("https://open.spotify.com/track/bad").is_none());
    }
}
