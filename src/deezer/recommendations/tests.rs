use super::*;
use crate::deezer::tests::item;
use serde_json::json;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{path, query_param},
};

#[tokio::test]
async fn local_taste_selects_fresh_recordings_before_the_artist_cap() {
    let server = MockServer::start().await;
    Mock::given(path("/artist/10/related"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .mount(&server)
        .await;
    let songs = tracks(10, "Root", 8);
    Mock::given(path("/artist/10/top"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":songs})))
        .mount(&server)
        .await;
    let seed = parse_track(&song(1, "Seed", 10, "Root")).unwrap();
    let client = Client::mock(&server.uri());
    let baseline = client.recommendations(&seed, &[], 0).await.unwrap();
    assert_eq!(baseline.tracks.len(), ROOT_LIMIT);
    let mut stats = crate::stats::SongStats::default();
    for track in &baseline.tracks {
        for _ in 0..2 {
            stats.record_feedback(
                track,
                crate::stats::FeedbackKind::EarlySkip,
                crate::recommendation::now(),
            );
        }
    }
    let batch = client
        .personalized(&stats)
        .recommendations(&seed, &[], 0)
        .await
        .unwrap();
    assert_eq!(batch.tracks.len(), ROOT_LIMIT);
    assert!(
        batch
            .tracks
            .iter()
            .all(|track| baseline.tracks.iter().all(|old| old.id != track.id))
    );
    assert_batch(&seed, &[], &batch.tracks);
    // Personalization shares public metadata caches and sends no extra calls.
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

fn song(id: u64, title: &str, artist: u64, name: &str) -> Value {
    let mut value = item(id, title);
    value["artist"] = json!({"id":artist,"name":name});
    value
}

fn tracks(artist: u64, name: &str, count: u64) -> Vec<Value> {
    (0..count)
        .map(|i| {
            song(
                artist * 1000 + i,
                &format!("Song {artist} {i}"),
                artist,
                name,
            )
        })
        .collect()
}

fn assert_batch(seed: &Track, excluded: &[Track], tracks: &[Track]) {
    let eligibility = Eligibility::new(seed, excluded);
    assert!(tracks.len() <= BATCH_SIZE);
    let mut ids = HashSet::new();
    let mut recordings = HashSet::new();
    let mut counts = HashMap::<String, usize>::new();
    for track in tracks {
        assert!(
            eligibility.accepts(track),
            "{} / {}",
            track.name,
            track.artists
        );
        assert!(ids.insert(&track.id));
        assert!(recordings.insert(crate::catalog::recording_key(track)));
        for id in &track.artist_ids {
            let count = counts.entry(id.clone()).or_default();
            *count += 1;
            assert!(
                *count
                    <= if seed.artist_ids.contains(id) {
                        ROOT_LIMIT
                    } else {
                        ARTIST_LIMIT
                    }
            );
        }
    }
}

#[test]
fn selection_handles_thousands_of_dense_sparse_and_exhausted_catalogs() {
    use rand::{Rng, SeedableRng, seq::SliceRandom};
    let mut rng = rand::rngs::StdRng::seed_from_u64(20261021);
    let seed = parse_track(&song(1, "Seed", 10, "Root")).unwrap();
    for _ in 0..2000 {
        let mut candidates = Vec::new();
        for artist in 10..rng.gen_range(11..24) {
            candidates.extend(
                tracks(artist, &format!("Artist {artist}"), rng.gen_range(0..25))
                    .iter()
                    .filter_map(parse_track),
            );
        }
        candidates.shuffle(&mut rng);
        let excluded = candidates
            .iter()
            .take(rng.gen_range(0..=candidates.len()))
            .cloned()
            .collect::<Vec<_>>();
        let selected = balanced(&candidates, &seed, &excluded);
        assert_batch(&seed, &excluded, &selected);
        let mut capacity = HashMap::<String, usize>::new();
        let eligibility = Eligibility::new(&seed, &excluded);
        for track in candidates.iter().filter(|track| eligibility.accepts(track)) {
            *capacity.entry(artist_key(track)).or_default() += 1;
        }
        let expected: usize = capacity
            .iter()
            .map(|(artist, count)| {
                (*count).min(if artist == &artist_key(&seed) {
                    ROOT_LIMIT
                } else {
                    ARTIST_LIMIT
                })
            })
            .sum();
        assert_eq!(selected.len(), expected.min(BATCH_SIZE));
        let mut selected_counts = HashMap::<String, usize>::new();
        for track in &selected {
            *selected_counts.entry(artist_key(track)).or_default() += 1;
        }
        let tail = excluded.last().map(artist_key);
        let unavoidable = selected_counts
            .iter()
            .map(|(artist, count)| {
                count.saturating_sub(
                    selected.len() - count + usize::from(tail.as_ref() != Some(artist)),
                )
            })
            .max()
            .unwrap_or(0);
        let repeats = selected
            .windows(2)
            .filter(|pair| artist_key(&pair[0]) == artist_key(&pair[1]))
            .count()
            + usize::from(
                selected
                    .first()
                    .is_some_and(|track| Some(artist_key(track)) == tail),
            );
        assert_eq!(
            repeats,
            unavoidable,
            "capacity={capacity:?}, order={:?}, tail={tail:?}",
            selected.iter().map(artist_key).collect::<Vec<_>>()
        );
    }
}

#[test]
fn selection_filters_versions_duplicates_and_caps_shared_collaborators() {
    let seed = parse_track(&song(1, "Seed", 10, "Root")).unwrap();
    let mut values = tracks(11, "Neighbour", 20);
    values.extend([
        song(2, "Song 11 0 (Deluxe)", 11, "Neighbour"),
        song(3, "Seed (Japanese Version)", 10, "Root"),
        song(4, "Concert - Live", 12, "Other"),
        song(5, "Song (Remix)", 13, "Other"),
        song(6, "Song (Instrumental)", 14, "Other"),
    ]);
    for artist in 20..25 {
        let mut collab = song(artist, "Collaboration", artist, &format!("Artist {artist}"));
        collab["contributors"] = json!([
            {"id":artist,"name":format!("Artist {artist}")},
            {"id":11,"name":"Neighbour"}
        ]);
        values.push(collab);
        values.push(song(
            artist * 100,
            "Solo",
            artist,
            &format!("Artist {artist}"),
        ));
    }
    let candidates = values.iter().filter_map(parse_track).collect::<Vec<_>>();
    let selected = balanced(&candidates, &seed, &[]);
    assert_batch(&seed, &[], &selected);
    assert!(!selected.iter().any(|track| matches!(
        track.id.as_str(),
        "deezer:2" | "deezer:3" | "deezer:4" | "deezer:5" | "deezer:6"
    )));
    assert!(selected.iter().any(|track| track.name == "Solo"));
}

#[tokio::test]
async fn fiesta_sparse_top_pools_use_verified_search_and_cached_smart_refills() {
    let server = MockServer::start().await;
    let names = [
        "IZ*ONE",
        "Red Velvet",
        "OH MY GIRL",
        "Weeekly",
        "fromis_9",
        "Kep1er",
    ];
    let seed = parse_track(&song(1, "FIESTA", 10, names[0])).unwrap();
    Mock::given(path("/artist/10/related"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":
            (11..16).map(|id| json!({"id":id,"name":names[(id-10) as usize]})).collect::<Vec<_>>()
        })))
        .expect(1)
        .mount(&server)
        .await;
    for artist in 10..16 {
        let name = names[(artist - 10) as usize];
        let count = match artist {
            11 => 20,
            10 | 15 => 0,
            _ => 1,
        };
        Mock::given(path(format!("/artist/{artist}/top")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data":tracks(artist,name,count)})),
            )
            .mount(&server)
            .await;
        Mock::given(path("/search"))
            .and(query_param("q", name))
            .respond_with(move |_: &Request| {
                let mut values = tracks(artist, name, 20);
                values.insert(0, song(9999, "Wrong same-name artist", 999, name));
                let mut guest = song(9997, "Guest-credit detour", 999, "Unrelated artist");
                guest["contributors"] = json!([
                    {"id":999,"name":"Unrelated artist"}, {"id":artist,"name":name}
                ]);
                values.insert(0, guest);
                values.push(song(9998, "Unrelated cover", 999, "Cover Artist"));
                ResponseTemplate::new(200).set_body_json(json!({"data":values}))
            })
            .mount(&server)
            .await;
    }
    let client = Client::mock(&server.uri());
    let batch = client.recommendations(&seed, &[], 0).await.unwrap();
    assert_eq!(batch.tracks.len(), 15);
    assert_eq!(
        batch
            .tracks
            .iter()
            .map(artist_key)
            .collect::<HashSet<_>>()
            .len(),
        6
    );
    assert!(
        !batch
            .tracks
            .iter()
            .any(|track| track.artist_ids.contains(&"deezer:999".into()))
    );
    assert_batch(&seed, &[], &batch.tracks);
    let requests = server.received_requests().await.unwrap().len();
    let smart = client
        .recommendations(&seed, &batch.tracks, 0)
        .await
        .unwrap();
    assert_eq!(smart.tracks.len(), 15);
    assert_batch(&seed, &batch.tracks, &smart.tracks);
    // Search is lazily added for the formerly dense artist only after its
    // remaining top pool falls below the cap; cached responses are reused.
    let repeat = client
        .recommendations(&seed, &batch.tracks, 0)
        .await
        .unwrap();
    assert_eq!(
        smart.tracks.iter().map(|t| &t.id).collect::<Vec<_>>(),
        repeat.tracks.iter().map(|t| &t.id).collect::<Vec<_>>()
    );
    assert_eq!(server.received_requests().await.unwrap().len(), requests);
    let mut excluded = batch.tracks;
    excluded.extend(smart.tracks);
    let refill = client.recommendations(&seed, &excluded, 1).await.unwrap();
    assert_eq!(refill.tracks.len(), 15);
    assert_batch(&seed, &excluded, &refill.tracks);
    assert!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| !r.headers.contains_key("authorization"))
    );
}

#[tokio::test]
async fn exhausted_neighbours_are_skipped_without_one_artist_padding() {
    let server = MockServer::start().await;
    let seed = parse_track(&song(1, "Seed", 10, "Root")).unwrap();
    Mock::given(path("/artist/10/related"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":
            ([11,11,10,12,13,14,15,16,17].map(|id| json!({"id":id})))
        })))
        .mount(&server)
        .await;
    for artist in 10..18 {
        Mock::given(path(format!("/artist/{artist}/top")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":
                tracks(artist,&format!("Artist {artist}"),if artist==11 || artist>=16 {20} else {0})
            })))
            .mount(&server)
            .await;
    }
    // Root fallback has no results.
    Mock::given(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .mount(&server)
        .await;
    let batch = Client::mock(&server.uri())
        .recommendations(&seed, &[], 0)
        .await
        .unwrap();
    assert_eq!(batch.tracks.len(), 9);
    assert_batch(&seed, &[], &batch.tracks);
    assert_eq!(
        batch
            .tracks
            .iter()
            .map(artist_key)
            .collect::<HashSet<_>>()
            .len(),
        3
    );
}

#[tokio::test]
async fn no_related_artists_returns_a_short_seed_pool_and_never_fifteen_of_one_artist() {
    let server = MockServer::start().await;
    let seed = parse_track(&song(1, "Seed", 10, "Root")).unwrap();
    Mock::given(path("/artist/10/related"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .mount(&server)
        .await;
    Mock::given(path("/artist/10/top"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data":tracks(10,"Root",20)})),
        )
        .mount(&server)
        .await;
    let batch = Client::mock(&server.uri())
        .recommendations(&seed, &[], 0)
        .await
        .unwrap();
    assert_eq!(batch.tracks.len(), ROOT_LIMIT);
    assert_batch(&seed, &[], &batch.tracks);
}

#[tokio::test]
async fn empty_catalog_has_a_bounded_request_budget() {
    let server = MockServer::start().await;
    let seed = parse_track(&song(1, "Seed", 10, "Root")).unwrap();
    Mock::given(path("/artist/10/related"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":
            (11..41).map(|id| json!({"id":id})).collect::<Vec<_>>()
        })))
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::path_regex(r"^/artist/\d+/top$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .expect((1 + MAX_NEIGHBOURS) as u64)
        .mount(&server)
        .await;
    Mock::given(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .expect(1)
        .mount(&server)
        .await;
    let batch = Client::mock(&server.uri())
        .recommendations(&seed, &[], 0)
        .await
        .unwrap();
    assert!(batch.tracks.is_empty());
}

#[tokio::test]
#[ignore = "Read-only live recommendation quality matrix; public metadata only, no audio or saved-state changes"]
async fn live_deezer_recommendation_quality_matrix() -> Result<()> {
    let client = Client::new()?;
    for (query, artist) in [
        ("FIESTA IZ*ONE", "IZ*ONE"),
        ("Psycho Red Velvet", "Red Velvet"),
        ("Radiohead Creep", "Radiohead"),
        ("Castle on the Hill Ed Sheeran", "Ed Sheeran"),
        ("Mưa Tháng Sáu Văn Mai Hương", "Van Mai Huong"),
        ("ヨルシカ だから僕は音楽を辞めた", "ヨルシカ"),
        ("tuki. 晩餐歌", "tuki."),
        ("Olivia Rodrigo good 4 u", "Olivia Rodrigo"),
        ("Kendrick Lamar HUMBLE.", "Kendrick Lamar"),
        ("Metallica Enter Sandman", "Metallica"),
        ("Daft Punk One More Time", "Daft Punk"),
        ("Miles Davis So What", "Miles Davis"),
    ] {
        let Rows::Tracks(search) = client.page(&Browse::Search(query.into()), 0).await?.rows else {
            unreachable!()
        };
        let seed = search
            .iter()
            .find(|track| {
                track
                    .artists
                    .split(',')
                    .any(|name| name.trim().eq_ignore_ascii_case(artist))
            })
            .with_context(|| {
                format!(
                    "No verified seed for {query}; found {:?}",
                    search.iter().map(|t| &t.artists).collect::<Vec<_>>()
                )
            })?;
        let batch = client.recommendations(seed, &[], 0).await?;
        assert_batch(seed, &[], &batch.tracks);
        ensure!(
            batch.tracks.len() >= 8,
            "Sparse initial batch for {query}: {}",
            batch.tracks.len()
        );
        let artists = batch
            .tracks
            .iter()
            .map(artist_key)
            .collect::<HashSet<_>>()
            .len();
        ensure!(artists >= 4, "Too few artists for {query}: {artists}");
        println!(
            "SEED {} / {}: {} tracks, {} artists",
            seed.name,
            seed.artists,
            batch.tracks.len(),
            artists
        );
        for track in &batch.tracks {
            println!("  {} / {}", track.name, track.artists);
        }
        let at = crate::recommendation::now();
        let mut taste = crate::stats::SongStats::default();
        for track in batch.tracks.iter().take(2) {
            for _ in 0..2 {
                taste.record_feedback(track, crate::stats::FeedbackKind::EarlySkip, at);
            }
        }
        for track in batch.tracks.iter().skip(2).take(2) {
            taste.record_feedback(track, crate::stats::FeedbackKind::Enjoyed, at);
        }
        let personalized = client
            .personalized(&taste)
            .recommendations(seed, &[], 0)
            .await?;
        assert_batch(seed, &[], &personalized.tracks);
        ensure!(
            personalized.tracks.len() >= 8,
            "Sparse personalized batch for {query}"
        );
        ensure!(
            personalized.tracks.iter().all(|track| {
                batch.tracks.iter().take(2).all(|skipped| {
                    crate::catalog::recording_key(skipped) != crate::catalog::recording_key(track)
                })
            }),
            "Repeated skip reappeared for {query}"
        );
        println!(
            "  PERSONALIZED: {} tracks; repeated skips excluded",
            personalized.tracks.len()
        );
        let mut excluded = batch.tracks;
        for round in 1..=2 {
            let refill = client.recommendations(seed, &excluded, round).await?;
            assert_batch(seed, &excluded, &refill.tracks);
            ensure!(
                !refill.tracks.is_empty(),
                "Empty refill {round} for {query}"
            );
            let artists = refill
                .tracks
                .iter()
                .map(artist_key)
                .collect::<HashSet<_>>()
                .len();
            ensure!(
                artists >= 3,
                "Concentrated refill {round} for {query}: {artists}"
            );
            println!(
                "  REFILL {round}: {} tracks, {artists} artists",
                refill.tracks.len()
            );
            excluded.extend(refill.tracks);
        }
        let smart = client.recommendations(seed, &excluded, 0).await?;
        assert_batch(seed, &excluded, &smart.tracks);
        println!("  SMART after radio: {} fresh tracks", smart.tracks.len());
    }
    Ok(())
}
