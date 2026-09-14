use super::*;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{path, query_param},
    };

    fn item(id: usize, title: &str, artist: usize, name: &str) -> Value {
        serde_json::json!({"id":format!("{id:022}"),"name":title,"type":"track","is_playable":true,
            "artists":[{"id":format!("{artist:022}"),"name":name}]})
    }

    async fn provider() -> (MockServer, Similarity) {
        let server = MockServer::start().await;
        let provider = Similarity::mock(&server.uri());
        (server, provider)
    }

    async fn root_and_related(server: &MockServer) {
        Mock::given(path("/search/artist"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data":[{"id":1,"name":"Root"}]})),
            )
            .expect(1)
            .mount(server)
            .await;
        let artists: Vec<_> = (0..5)
            .map(|i| serde_json::json!({"id":20+i,"name":format!("Related {i}")}))
            .collect();
        Mock::given(path("/artist/1/related"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"data":artists})),
            )
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn diverse_radio_rejects_noise_and_smart_reuses_cached_similarity() {
        let spotify = MockServer::start().await;
        let (external, similarity) = provider().await;
        root_and_related(&external).await;
        let mut catalog = Catalog::mock(&spotify.uri());
        catalog.similarity = Some(similarity);
        let seed = parse_track(&item(1, "Seed", 1, "Root")).unwrap();
        Mock::given(path("/search"))
            .respond_with(|request: &Request| {
                let q: HashMap<_, _> = request.url.query_pairs().into_owned().collect();
                let name = q["q"].trim_start_matches("artist:").trim_matches('"');
                let artist = if name == "Root" {
                    1
                } else {
                    20 + name
                        .trim_start_matches("Related ")
                        .parse::<usize>()
                        .unwrap()
                };
                let offset: usize = q["offset"].parse().unwrap();
                let mut tracks: Vec<_> = (0..10)
                    .map(|i| {
                        item(
                            artist * 1000 + offset + i,
                            &format!("Song {artist} {}", offset + i),
                            artist,
                            name,
                        )
                    })
                    .collect();
                tracks.push(item(999, "Unrelated chart hit", 999, "Taylor Swift"));
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"tracks":{"items":tracks}}))
            })
            .mount(&spotify)
            .await;
        let first = catalog.radio_recommendations(&seed, &[], 0).await.unwrap();
        assert_eq!(first.source, RecommendationSource::SimilarArtists);
        assert_eq!(first.tracks.len(), 15);
        let mut counts = HashMap::new();
        for track in &first.tracks {
            *counts.entry(&track.artists).or_insert(0) += 1;
        }
        assert!(counts.len() >= 5 && counts.values().all(|n| *n <= 3));
        assert!(
            first
                .tracks
                .iter()
                .all(|track| track.artists != "Taylor Swift")
        );
        let before = spotify.received_requests().await.unwrap().len();
        let smart = catalog
            .smart_recommendations(&seed, &first.tracks, &[])
            .await
            .unwrap();
        assert!(
            smart
                .tracks
                .iter()
                .filter(|track| track.artists != "Root")
                .count()
                >= 10
        );
        assert_eq!(spotify.received_requests().await.unwrap().len(), before);
        let second = catalog
            .radio_recommendations(&seed, &first.tracks, 1)
            .await
            .unwrap();
        assert_eq!(second.tracks.len(), 15);
        assert!(second.tracks.iter().all(|track| {
            !first
                .tracks
                .iter()
                .any(|old| recording_key(old) == recording_key(track))
        }));
        assert_eq!(external.received_requests().await.unwrap().len(), 2);
        assert!(
            external
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| !request.headers.contains_key("authorization"))
        );
    }

    #[tokio::test]
    async fn ambiguous_artist_is_resolved_by_recording_not_first_search_hit() {
        let (server, similarity) = provider().await;
        Mock::given(path("/search/artist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"data":[{"id":1,"name":"Eve"},{"id":2,"name":"Eve"}]}),
            ))
            .mount(&server)
            .await;
        Mock::given(path("/search")).and(query_param("q","artist:\"Eve\" track:\"kaikai kitan\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data":[{"title":"Kaikai Kitan","artist":{"id":2,"name":"Eve"}}]})))
            .mount(&server).await;
        Mock::given(path("/artist/2/related"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"data":[{"name":"yama"},{"name":"Yorushika"}]}),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;
        let seed = parse_track(&item(1, "Kaikai Kitan", 1, "Eve")).unwrap();
        assert_eq!(
            similarity.artists(&seed).await.unwrap(),
            vec!["yama", "Yorushika"]
        );
    }

    #[tokio::test]
    async fn exact_suis_name_wins_over_parenthetical_catalog_entry() {
        let (server, similarity) = provider().await;
        Mock::given(path("/search/artist"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"data":[
                {"id":1,"name":"suis from Yorushika"},{"id":2,"name":"suis (from Yorushika)"}]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/artist/1/related"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"data":[{"name":"aimyon"},{"name":"Masaki Suda"}]}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let seed = parse_track(&item(1, "猫日", 1, "suis from Yorushika")).unwrap();
        assert_eq!(
            similarity.artists(&seed).await.unwrap(),
            vec!["aimyon", "Masaki Suda"]
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn same_name_spotify_artists_do_not_share_identity_cache() {
        let (server, similarity) = provider().await;
        Mock::given(path("/search/artist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"data":[{"id":1,"name":"Eve"},{"id":2,"name":"Eve"}]}),
            ))
            .expect(2)
            .mount(&server)
            .await;
        for (song, id, related) in [
            ("japanese song", 1, vec!["yama", "Yorushika"]),
            ("rap song", 2, vec!["Missy Elliott", "Foxy Brown"]),
        ] {
            Mock::given(path("/search"))
                .and(query_param("q", format!("artist:\"Eve\" track:\"{song}\"")))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"data":[{"title":song,"artist":{"id":id,"name":"Eve"}}]}),
                ))
                .mount(&server)
                .await;
            let artists: Vec<_> = related
                .iter()
                .map(|name| serde_json::json!({"name":name}))
                .collect();
            Mock::given(path(format!("/artist/{id}/related")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"data":artists})),
                )
                .expect(1)
                .mount(&server)
                .await;
            let seed = parse_track(&item(id, song, id, "Eve")).unwrap();
            assert_eq!(similarity.artists(&seed).await.unwrap(), related);
        }
    }
    #[tokio::test]
    async fn similarity_rate_limit_is_cached_and_never_padded_with_root_artist() {
        let spotify = MockServer::start().await;
        let (external, similarity) = provider().await;
        Mock::given(path("/search/artist"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "3600"))
            .expect(1)
            .mount(&external)
            .await;
        let mut catalog = Catalog::mock(&spotify.uri());
        catalog.similarity = Some(similarity);
        let seed = parse_track(&item(1, "Seed", 1, "Root")).unwrap();
        for _ in 0..2 {
            let error = catalog
                .radio_recommendations(&seed, &[], 0)
                .await
                .unwrap_err();
            assert!(format!("{error:#}").contains("rate limit"));
        }
        assert_eq!(external.received_requests().await.unwrap().len(), 1);
        assert!(spotify.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn unrelated_spotify_search_results_do_not_satisfy_similarity() {
        let spotify = MockServer::start().await;
        let (external, similarity) = provider().await;
        root_and_related(&external).await;
        Mock::given(path("/search")).respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"tracks":{"items":[item(999,"Unrelated hit",999,"Taylor Swift")]}})
        )).mount(&spotify).await;
        let mut catalog = Catalog::mock(&spotify.uri());
        catalog.similarity = Some(similarity);
        let seed = parse_track(&item(1, "Seed", 1, "Root")).unwrap();
        assert!(catalog.radio_recommendations(&seed, &[], 0).await.is_err());
        assert_eq!(spotify.received_requests().await.unwrap().len(), 5);
    }
}

/// Public artist similarity metadata only. Spotify credentials are never sent here.
#[derive(Clone)]
pub(super) struct Similarity {
    client: reqwest::Client,
    base: String,
    state: Arc<Mutex<SimilarityState>>,
}

#[derive(Default)]
struct SimilarityState {
    cache: HashMap<String, (Instant, std::result::Result<Vec<String>, String>)>,
    next_request: Option<Instant>,
    cooldown: Option<Instant>,
}

fn name_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

impl Similarity {
    pub(super) fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .user_agent("Tuitify/0.2.8 (https://github.com/braces157/tutify)")
                .connect_timeout(Duration::from_secs(4))
                .timeout(Duration::from_secs(10))
                .build()?,
            base: "https://api.deezer.com".into(),
            state: Arc::new(Mutex::new(SimilarityState::default())),
        })
    }

    #[cfg(test)]
    pub(super) fn mock(base: &str) -> Self {
        Self {
            base: base.into(),
            ..Self::new().unwrap()
        }
    }

    async fn get(
        &self,
        state: &mut SimilarityState,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Value> {
        if state.cooldown.is_some_and(|until| until > Instant::now()) {
            bail!("Similar-artist service is cooling down; retry later");
        }
        if let Some(next) = state.next_request {
            tokio::time::sleep(next.saturating_duration_since(Instant::now())).await;
        }
        state.next_request = Some(Instant::now() + Duration::from_secs(1));
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .query(query)
            .send()
            .await?;
        if response.status().as_u16() == 429 {
            let wait = retry_delay(
                response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok()),
            );
            state.cooldown = Instant::now().checked_add(wait);
            bail!(
                "Similar-artist service rate limit; wait {} seconds",
                wait.as_secs()
            );
        }
        let value: Value = response.error_for_status()?.json().await?;
        if value.get("error").is_some() {
            // Deezer also reports quota/service errors inside HTTP 200 responses.
            state.cooldown = Some(Instant::now() + Duration::from_secs(60));
            bail!("Similar-artist service returned an error");
        }
        Ok(value)
    }

    pub(super) async fn artists(&self, seed: &Track) -> Result<Vec<String>> {
        let name = seed.artists.split(',').next().unwrap_or("").trim();
        if name.is_empty() {
            bail!("Missing seed artist for similarity lookup");
        }
        let identity = seed
            .artist_ids
            .first()
            .cloned()
            .unwrap_or_else(|| normalize_title(&seed.name));
        let key = format!("{}:{identity}", name_key(name));
        let mut state = self.state.lock().await;
        if let Some((until, result)) = state.cache.get(&key) {
            if *until > Instant::now() {
                return result.clone().map_err(anyhow::Error::msg);
            }
        }
        let result = self.lookup(&mut state, name, &seed.name).await;
        let cached = result
            .as_ref()
            .map(Clone::clone)
            .map_err(ToString::to_string);
        let ttl = if cached.is_ok() { 1800 } else { 60 };
        if state.cache.len() >= 64 {
            state.cache.clear();
        }
        state
            .cache
            .insert(key, (Instant::now() + Duration::from_secs(ttl), cached));
        result
    }

    async fn lookup(
        &self,
        state: &mut SimilarityState,
        name: &str,
        title: &str,
    ) -> Result<Vec<String>> {
        let value = self
            .get(
                state,
                "/search/artist",
                &[("q", name.into()), ("limit", "10".into())],
            )
            .await?;
        let mut ids: HashSet<u64> = value["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|artist| {
                artist["name"]
                    .as_str()
                    .is_some_and(|found| name_key(found) == name_key(name))
            })
            .filter_map(|artist| artist["id"].as_u64())
            .collect();
        let exact_ids: HashSet<u64> = value["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|artist| {
                artist["name"]
                    .as_str()
                    .is_some_and(|found| found.trim().eq_ignore_ascii_case(name))
            })
            .filter_map(|artist| artist["id"].as_u64())
            .collect();
        if !exact_ids.is_empty() {
            ids = exact_ids;
        }
        if ids.len() > 1 {
            // Resolve same-name artists using a matching recording, not popularity.
            let q = format!(
                "artist:\"{}\" track:\"{}\"",
                name.replace('"', " "),
                normalize_title(title)
            );
            let value = self
                .get(state, "/search", &[("q", q), ("limit", "10".into())])
                .await?;
            let matched: HashSet<_> = value["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|track| {
                    track["title"]
                        .as_str()
                        .is_some_and(|found| normalize_title(found) == normalize_title(title))
                })
                .filter(|track| {
                    track["artist"]["name"]
                        .as_str()
                        .is_some_and(|found| name_key(found) == name_key(name))
                })
                .filter_map(|track| track["artist"]["id"].as_u64())
                .collect();
            ids.retain(|id| matched.contains(id));
        }
        if ids.len() != 1 {
            bail!("Could not identify {name} unambiguously in the similar-artist catalog");
        }
        let id = ids.into_iter().next().unwrap();
        let value = self
            .get(
                state,
                &format!("/artist/{id}/related"),
                &[("limit", "10".into())],
            )
            .await?;
        let mut seen = HashSet::from([name_key(name)]);
        let names: Vec<_> = value["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|artist| artist["name"].as_str())
            .filter(|name| !name.trim().is_empty() && seen.insert(name_key(name)))
            .map(clean)
            .take(8)
            .collect();
        if names.len() < 2 {
            bail!("Not enough similar artists available for {name}");
        }
        Ok(names)
    }
}

impl Catalog {
    /// Fetch one Spotify page per neighbour, validating its actual artist credit.
    /// Cache by artist and round so Smart mode changes reuse the Radio requests.
    async fn similar_artist_page(
        &self,
        name: &str,
        known_ids: &[String],
        round: usize,
    ) -> Result<Vec<Track>> {
        let key = format!("similar:{}:{}", name_key(name), known_ids.join(","));
        let mut profile = self
            .discovery
            .lock()
            .await
            .get(&key)
            .cloned()
            .unwrap_or_default();
        if let Some(batch) = profile.batches.get(&round) {
            return Ok(batch.clone());
        }
        let value = self
            .get(
                "/search",
                &[
                    (
                        "q",
                        format!("artist:\"{}\"", name.replace(['"', '\\'], " ")),
                    ),
                    ("type", "track".into()),
                    ("limit", "10".into()),
                    ("offset", (round * 10).to_string()),
                ],
            )
            .await?;
        let items: Vec<_> = value["tracks"]["items"]
            .as_array()
            .into_iter()
            .flatten()
            .collect();
        let mut artist_ids: HashSet<String> = known_ids.iter().cloned().collect();
        if artist_ids.is_empty() {
            artist_ids = items
                .iter()
                .flat_map(|track| track["artists"].as_array().into_iter().flatten())
                .filter(|artist| {
                    artist["name"]
                        .as_str()
                        .is_some_and(|found| name_key(found) == name_key(name))
                })
                .filter_map(|artist| artist["id"].as_str())
                .filter(|id| valid_id(id))
                .map(str::to_owned)
                .collect();
            // Multiple same-name Spotify artists cannot safely share one pool.
            if artist_ids.len() != 1 {
                return Ok(vec![]);
            }
        }
        let mut candidates: Vec<_> = items
            .into_iter()
            .filter_map(parse_track)
            .filter(|track| track.artist_ids.iter().any(|id| artist_ids.contains(id)))
            .collect();
        if round > 0 {
            if let Some(previous) = profile.batches.get(&(round - 1)) {
                candidates.extend(previous.clone());
            }
        }
        candidates = discovery::unique_tracks(candidates);
        if profile.batches.len() >= 2 {
            profile.batches.clear();
        }
        profile.batches.insert(round, candidates.clone());
        let mut cache = self.discovery.lock().await;
        if cache.len() >= 64 && !cache.contains_key(&key) {
            cache.clear();
        }
        cache.insert(key, profile);
        Ok(candidates)
    }

    pub(super) async fn similarity_candidates(
        &self,
        seed: &Track,
        round: usize,
    ) -> Result<Vec<Track>> {
        if round >= 50 {
            return Ok(vec![]);
        }
        let provider = self
            .similarity
            .as_ref()
            .context("Similar-artist service is disabled")?;
        let names = provider
            .artists(seed)
            .await
            .context("Similar-artist discovery unavailable")?;
        let mut candidates = Vec::new();
        let mut found = 0;
        // At most six artist pages, including a spare when one is absent in Spotify.
        for name in names.iter().take(6) {
            let tracks = self.similar_artist_page(name, &[], round).await?;
            if !tracks.is_empty() {
                found += 1;
                candidates.extend(tracks);
            }
            if found >= 5 {
                break;
            }
        }
        if found < 2 {
            bail!(
                "Too few similar artists could be matched in Spotify. Your queue is preserved; retry later."
            );
        }
        let primary = seed.artists.split(',').next().unwrap_or("").trim();
        let root_ids = seed
            .artist_ids
            .first()
            .cloned()
            .into_iter()
            .collect::<Vec<_>>();
        candidates.extend(self.similar_artist_page(primary, &root_ids, round).await?);
        Ok(discovery::unique_tracks(candidates))
    }
}
