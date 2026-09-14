use super::*;
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::path};

fn item(id: usize, name: &str, artist: usize) -> Value {
    serde_json::json!({"id":format!("{id:022}"),"name":name,"type":"track","is_playable":true,
        "duration_ms":200000,"artists":[{"id":format!("{artist:022}"),"name":format!("Artist {artist}")}]})
}

#[tokio::test]
async fn smart_japanese_queue_rejects_unrelated_pop_and_keeps_connected_romanized_songs() {
    use wiremock::matchers::query_param;
    let server = MockServer::start().await;
    let catalog = Catalog::mock(&server.uri());
    let seed_value = serde_json::json!({"id":format!("{:022}",1),"name":"猫日","type":"track","is_playable":true,
        "artists":[{"id":format!("{:022}",10),"name":"suis from Yorushika"}]});
    let seed = parse_track(&seed_value).unwrap();
    let duet = parse_track(&serde_json::json!({"id":format!("{:022}",2),"name":"Heikousen","type":"track","is_playable":true,
        "artists":[{"id":format!("{:022}",20),"name":"Eve"},{"id":format!("{:022}",10),"name":"suis from Yorushika"}]})).unwrap();
    let make = |id: usize, title: &str, artist: usize, name: &str| serde_json::json!({"id":format!("{id:022}"),"name":title,"type":"track","is_playable":true,"artists":[{"id":format!("{artist:022}"),"name":name}]});
    let taylor = make(90, "The Fate of Ophelia", 90, "Taylor Swift");
    let olivia = make(91, "stupid song", 91, "Olivia Rodrigo");
    for (name, expected) in [
        (
            "suis from Yorushika",
            make(3, "新しい曲", 10, "suis from Yorushika"),
        ),
        ("Eve", make(4, "English Title", 20, "Eve")),
    ] {
        Mock::given(path("/search"))
            .and(query_param("q", format!("artist:\"{name}\"")))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"tracks":{"items":[seed_value, taylor, olivia, expected]}}),
            ))
            .expect(2)
            .mount(&server)
            .await;
    }
    Mock::given(path(format!("/artists/{:022}/albums", 10)))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"items":[]})))
        .expect(1)
        .mount(&server)
        .await;
    let context = vec![seed.clone(), duet];
    let mut excluded = context.clone();
    excluded.push(parse_track(&taylor).unwrap()); // Old injected suggestion must not become context.
    let batch = catalog
        .smart_recommendations(&seed, &excluded, &context)
        .await
        .unwrap();
    assert_eq!(
        batch
            .tracks
            .iter()
            .map(|track| track.name.as_str())
            .collect::<Vec<_>>(),
        vec!["新しい曲", "English Title"]
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 5);
}

#[tokio::test]
async fn discovery_paginates_reuses_candidates_and_excludes_recordings() {
    let server = MockServer::start().await;
    let catalog = Catalog::mock(&server.uri());
    let seed = parse_track(&item(1, "Seed", 1)).unwrap();
    Mock::given(path("/search"))
        .respond_with(|request: &Request| {
            let query: std::collections::HashMap<_, _> =
                request.url.query_pairs().into_owned().collect();
            assert_eq!(query["q"], "artist:\"Artist 1\"");
            let offset: usize = query["offset"].parse().unwrap();
            let mut items: Vec<_> = (0..10)
                .map(|i| item(100 + offset + i, &format!("Original {}", offset + i), 1))
                .collect();
            items.extend([
                item(2, "Seed - Remastered", 1),
                item(3, "Seed - Remix", 1),
                item(4, "Unrelated chart hit", 90),
            ]);
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"tracks":{"items":items}}))
        })
        .mount(&server)
        .await;
    let first = catalog.radio_recommendations(&seed, &[], 0).await.unwrap();
    assert_eq!(first.source, RecommendationSource::ArtistSearch);
    assert_eq!(first.tracks.len(), 15); // No two-song-per-artist starvation.
    assert!(
        first
            .tracks
            .iter()
            .all(|track| track.artist_ids == seed.artist_ids
                && normalize_title(&track.name) != "seed")
    );
    let second = catalog
        .radio_recommendations(&seed, &first.tracks, 1)
        .await
        .unwrap();
    assert_eq!(second.tracks.len(), 15);
    let keys: std::collections::HashSet<_> = first.tracks.iter().map(recording_key).collect();
    assert!(
        second
            .tracks
            .iter()
            .all(|track| !keys.contains(&recording_key(track))
                && track.artist_ids == seed.artist_ids)
    );
    let before = server.received_requests().await.unwrap().len();
    assert_eq!(before, 4);
    assert_eq!(
        catalog
            .radio_recommendations(&seed, &[], 0)
            .await
            .unwrap()
            .tracks,
        first.tracks
    );
    assert!(
        catalog
            .radio_recommendations(&seed, &[], 50)
            .await
            .unwrap()
            .tracks
            .is_empty()
    );
    assert_eq!(server.received_requests().await.unwrap().len(), before);
}

#[tokio::test]
async fn tuki_live_seed_uses_albums_when_search_is_sparse_without_chart_padding() {
    let server = MockServer::start().await;
    let catalog = Catalog::mock(&server.uri());
    let mut seed_value = item(1, "月面着陸計画 - Live", 10);
    seed_value["artists"][0]["name"] = "tuki.".into();
    let seed = parse_track(&seed_value).unwrap();
    Mock::given(path("/search")).respond_with(ResponseTemplate::new(200).set_body_json(
        serde_json::json!({"tracks":{"items":[seed_value, item(90,"The Fate of Ophelia",90), item(91,"stupid song",91)],"next":null}})
    )).expect(1).mount(&server).await;
    Mock::given(path(format!("/artists/{:022}/albums",10))).respond_with(ResponseTemplate::new(200).set_body_json(
        serde_json::json!({"items":[{"id":format!("{:022}",500),"name":"15","album_type":"album"}]})
    )).expect(1).mount(&server).await;
    let mut items: Vec<_> = (0..16)
        .map(|i| {
            let mut track = item(100 + i, &format!("Original {i}"), 10);
            track["artists"][0]["name"] = "tuki.".into();
            track
        })
        .collect();
    let mut alternate = item(93, "Original 1 - Remastered", 10);
    alternate["artists"][0]["name"] = "tuki.".into();
    items.extend([item(92, "Unrelated soundtrack song", 92), alternate]);
    Mock::given(path(format!("/albums/{:022}/tracks", 500)))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"items":items})))
        .expect(1)
        .mount(&server)
        .await;
    let first = catalog.radio_recommendations(&seed, &[], 0).await.unwrap();
    assert_eq!(first.tracks.len(), 15);
    assert!(
        first
            .tracks
            .iter()
            .all(|track| track.artist_ids == seed.artist_ids
                && track.album_id.as_deref() == Some(&format!("{:022}", 500)))
    );
    // Smart Shuffle uses the SAME vetted pool; switching modes needs no new HTTP.
    let smart = catalog
        .smart_recommendations(&seed, &[], &[])
        .await
        .unwrap();
    assert_eq!(smart.tracks.len(), 16);
    assert!(
        smart
            .tracks
            .iter()
            .all(|track| track.artist_ids == seed.artist_ids)
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn one_off_guest_cannot_redirect_radio_or_smart_shuffle() {
    let server = MockServer::start().await;
    let catalog = Catalog::mock(&server.uri());
    let seed = parse_track(&item(1, "Seed", 1)).unwrap();
    Mock::given(path("/search"))
        .respond_with(|request: &Request| {
            let query: std::collections::HashMap<_, _> =
                request.url.query_pairs().into_owned().collect();
            assert_eq!(
                query["q"], "artist:\"Artist 1\"",
                "one-off guest was queried"
            );
            let offset: usize = query["offset"].parse().unwrap();
            let mut tracks: Vec<_> = (0..10)
                .map(|i| item(100 + offset + i, &format!("Original {}", offset + i), 1))
                .collect();
            let mut duet = item(999, "One guest appearance", 1);
            duet["artists"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({"id":format!("{:022}",90),"name":"Guest"}));
            tracks.push(duet);
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"tracks":{"items":tracks}}))
        })
        .expect(2)
        .mount(&server)
        .await;
    let radio = catalog.radio_recommendations(&seed, &[], 0).await.unwrap();
    let smart = catalog
        .smart_recommendations(&seed, &[], &[])
        .await
        .unwrap();
    assert!(
        radio
            .tracks
            .iter()
            .chain(&smart.tracks)
            .all(|track| track.artist_ids.contains(&format!("{:022}", 1)))
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn discovery_stops_at_rate_limit_without_search_fallback_or_retry_storm() {
    let server = MockServer::start().await;
    let catalog = Catalog::mock(&server.uri());
    let seed = parse_track(&item(1, "Seed", 1)).unwrap();
    Mock::given(path("/search"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "60"))
        .expect(1)
        .mount(&server)
        .await;
    assert!(
        catalog
            .radio_recommendations(&seed, &[], 0)
            .await
            .unwrap_err()
            .to_string()
            .contains("rate limit")
    );
    assert!(catalog.radio_recommendations(&seed, &[], 0).await.is_err());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
