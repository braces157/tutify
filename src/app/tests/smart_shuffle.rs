use super::*;

fn app() -> App {
    let mut queue = Queue::default();
    queue.replace((0..9).map(|i| test_track(i).id).collect(), 0, false);
    let mut app = App::new(Config::default(), queue);
    for i in 0..9 {
        app.cache.insert(test_track(i).id, test_track(i));
    }
    app
}

fn batch() -> Result<Recommendations> {
    let mut unavailable = test_track(99);
    unavailable.playable = false;
    let distinct = |i: usize| {
        let mut track = test_track(i);
        track.artists = format!("Artist {i}");
        track.artist_ids = vec![format!("{:022}", 1000 + i)];
        track
    };
    Ok(Recommendations {
        tracks: vec![
            test_track(0),
            distinct(20),
            distinct(20),
            unavailable,
            distinct(21),
            distinct(22),
        ],
        source: crate::catalog::RecommendationSource::ArtistSearch,
    })
}

#[test]
fn smart_candidates_spread_artists_and_avoid_nearby_artist_when_possible() {
    let mut app = app();
    app.queue.smart_shuffle = true;
    let mut same_artist = test_track(20);
    same_artist.artists = "Artist".into();
    let mut alt_one = test_track(21);
    alt_one.artists = "Different One".into();
    let mut alt_two = test_track(22);
    alt_two.artists = "Different Two".into();
    let selected =
        super::super::smart_shuffle::diverse_candidates(&app, &[same_artist, alt_one, alt_two]);
    assert_eq!(
        selected
            .iter()
            .map(|track| track.id.clone())
            .collect::<Vec<_>>(),
        vec![test_track(21).id, test_track(22).id, test_track(20).id]
    );
}

#[test]
fn smart_candidates_fill_spaced_slots_from_a_narrow_relevant_pool() {
    let mut app = app();
    app.queue.smart_shuffle = true;
    let tracks = [20, 21, 22]
        .into_iter()
        .map(|i| {
            let mut track = test_track(i);
            track.artists = "Repeated Artist".into();
            track
        })
        .collect::<Vec<_>>();
    let selected = super::super::smart_shuffle::diverse_candidates(&app, &tracks);
    assert_eq!(selected.len(), 3);
}

#[test]
fn smart_candidates_exclude_alternate_releases_but_allow_unrelated_same_titles() {
    let mut app = app();
    app.queue.smart_shuffle = true;
    let mut original = test_track(0);
    original.name = "My Song".into();
    app.cache.insert(original.id.clone(), original.clone());
    let mut alternate = original.clone();
    alternate.id = test_track(20).id;
    alternate.name = "My Song - Remastered".into();
    let mut unrelated = test_track(21);
    unrelated.name = "My Song".into();
    unrelated.artists = "Other Artist".into();
    unrelated.artist_ids.clear();
    let selected =
        super::super::smart_shuffle::diverse_candidates(&app, &[alternate, unrelated.clone()]);
    assert_eq!(selected, vec![unrelated]);
}

#[test]
fn radio_suggestions_never_become_smart_seeds() {
    let mut app = app();
    let root = test_track(0);
    app.radio_suggestions
        .extend(app.queue.ids.iter().skip(1).cloned());
    let attempted = HashSet::from([root.id]);
    assert!(super::super::smart_shuffle::smart_seed(&app, &attempted).is_none());
}

#[test]
fn smart_seed_tries_another_artist_before_repeating_with_legacy_metadata() {
    let mut app = app();
    let mut original = test_track(0);
    original.artist_ids = vec!["0000000000000000001000".into()];
    app.cache.insert(original.id.clone(), original.clone());
    let mut alternative = test_track(8);
    alternative.artists = "Another Artist".into();
    alternative.artist_ids.clear();
    app.cache
        .insert(alternative.id.clone(), alternative.clone());
    let attempted = HashSet::from([original.id]);
    let seed = super::super::smart_shuffle::smart_seed(&app, &attempted).unwrap();
    assert_eq!(seed.id, alternative.id);
    app.queue.suggestions.insert(8);
    let seed = super::super::smart_shuffle::smart_seed(&app, &attempted).unwrap();
    assert_eq!(seed.id, test_track(1).id);
}

#[test]
fn smart_seed_stays_anchored_to_queue_context_when_cursor_reaches_an_outlier() {
    let mut app = app();
    app.queue.smart_shuffle = true;
    app.queue.cursor = Some(6);
    app.queue.selected = 6;

    let mut anchor = test_track(0);
    anchor.artists = "Anchor Artist".into();
    app.cache.insert(anchor.id.clone(), anchor.clone());

    let mut outlier = test_track(6);
    outlier.artists = "Unrelated Outlier".into();
    app.cache.insert(outlier.id.clone(), outlier.clone());

    let mut attempted = HashSet::new();
    let first = super::super::smart_shuffle::smart_seed(&app, &attempted).unwrap();
    assert_eq!(first.id, anchor.id);
    assert_ne!(first.id, outlier.id);

    attempted.insert(first.id.clone());
    let second = super::super::smart_shuffle::smart_seed(&app, &attempted).unwrap();
    assert_eq!(second.id, test_track(1).id);
}

#[tokio::test]
#[ignore = "Requires the locally saved Spotify catalog login and queue; manual acceptance test"]
async fn live_smart_shuffle_seed_context_from_saved_queue() -> Result<()> {
    let store = Storage::local()?;
    let config = store.config()?;
    let mut app = App::new(config.clone(), store.queue()?);
    app.cache = store.cache()?;
    let catalog = Catalog::new(TokenManager::load(&config)?)?;
    let mut attempted = HashSet::new();

    for round in 1..=2 {
        let seed = super::super::smart_shuffle::smart_seed(&app, &attempted)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("saved queue has no eligible Smart Shuffle seed"))?;
        attempted.insert(seed.id.clone());
        let recommendations = catalog.recommendations(&seed).await?;
        println!(
            "LIVE SMART {round}: seed={} - {} | source={} | count={}",
            seed.name,
            seed.artists,
            recommendations.source.label(),
            recommendations.tracks.len()
        );
        for track in recommendations.tracks.iter().take(8) {
            println!("  {} - {}", track.name, track.artists);
        }
    }
    Ok(())
}

#[test]
fn smart_order_breaks_up_repeated_artists_in_the_upcoming_original_queue() {
    let mut app = app();
    let artists = [
        "Ed Sheeran",
        "Ed Sheeran",
        "Ed Sheeran",
        "Ed Sheeran",
        "Maroon 5",
        "Charlie Puth",
        "Avicii",
        "Wiz Khalifa",
        "Bruno Mars",
    ];
    for (i, artist) in artists.iter().enumerate() {
        let mut track = test_track(i);
        track.artists = (*artist).into();
        track.artist_ids = vec![format!(
            "{:022}",
            5000 + artists[..=i].iter().position(|a| a == artist).unwrap_or(i)
        )];
        if *artist == "Ed Sheeran" {
            track.artist_ids = vec!["0000000000000000005000".into()];
        }
        app.cache.insert(track.id.clone(), track);
    }
    app.queue.order = (0..artists.len()).collect();
    app.queue.cursor = Some(0);
    app.queue.selected = 3;
    let current = app.queue.current().unwrap().to_owned();
    let selected_entry = app.queue.order[app.queue.selected];

    super::super::smart_shuffle::diversify_smart_order(&mut app);

    assert_eq!(app.queue.current(), Some(current.as_str()));
    assert_eq!(app.queue.order[app.queue.selected], selected_entry);
    let upcoming_artists = app.queue.order[1..]
        .iter()
        .filter_map(|&index| app.cache.get(&app.queue.ids[index]))
        .map(|track| track.artists.as_str())
        .collect::<Vec<_>>();
    assert!(
        upcoming_artists.windows(2).all(|pair| pair[0] != pair[1]),
        "Smart Shuffle left adjacent same-artist tracks: {upcoming_artists:?}"
    );
}

#[test]
fn smart_order_preserves_played_history_and_current_track() {
    let mut app = app();
    for i in 0..9 {
        let mut track = test_track(i);
        track.artists = if i < 5 { "Repeated" } else { "Different" }.into();
        track.artist_ids = vec![if i < 5 {
            "0000000000000000006000".into()
        } else {
            format!("{:022}", 6000 + i)
        }];
        app.cache.insert(track.id.clone(), track);
    }
    app.queue.order = (0..9).collect();
    app.queue.cursor = Some(2);
    app.queue.selected = 2;
    let history = app.queue.order[..=2].to_vec();
    let current = app.queue.current().unwrap().to_owned();

    super::super::smart_shuffle::diversify_smart_order(&mut app);

    assert_eq!(&app.queue.order[..=2], history.as_slice());
    assert_eq!(app.queue.current(), Some(current.as_str()));
}

#[tokio::test]
async fn key_cycles_modes_adds_labeled_suggestions_and_undo_restores_smart() {
    let mut app = app();
    app.queue.position_ms = 5000;
    let (mut tasks, _) = tasks();
    let (tx, _rx) = mpsc::unbounded_channel();
    let shuffle = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
    key(&mut app, shuffle, &mut tasks, &tx);
    assert!(app.config.shuffle);
    assert!(!app.queue.smart_shuffle);
    key(&mut app, shuffle, &mut tasks, &tx);
    assert!(app.queue.smart_shuffle);
    assert!(tasks.smart.handle.is_some());
    let epoch = app.queue.epoch;
    let request = tasks.smart.request;
    background(
        &mut app,
        &mut tasks,
        Background::SmartRecommendations(epoch, request, batch()),
    );
    assert_eq!(app.queue.suggestions.len(), 3);
    assert!(app.status.contains("Artist-connected suggestions"));
    assert!(!app.queue.ids.contains(&test_track(99).id));
    assert_eq!(app.queue.current(), Some(test_track(0).id.as_str()));
    assert_eq!(app.queue.position_ms, 5000);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| ui::draw(frame, &app, &mut app.ui.render.borrow_mut()))
        .unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(screen.contains("SHUF:SMART"));
    assert!(screen.contains("✦"));
    key(&mut app, shuffle, &mut tasks, &tx);
    assert!(!app.config.shuffle);
    assert!(!app.queue.smart_shuffle);
    assert_eq!(app.queue.order, (0..9).collect::<Vec<_>>());
    key(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
        &mut tasks,
        &tx,
    );
    assert!(app.queue.smart_shuffle);
    assert_eq!(app.queue.suggestions.len(), 3);
    assert_eq!(app.state, State::Paused);
    app.queue.validate().unwrap();
}

#[tokio::test]
async fn stale_batches_are_rejected_after_mode_cycle_and_queue_replacement() {
    let mut app = app();
    let (mut tasks, _) = tasks();
    tasks.cycle_shuffle(&mut app);
    tasks.cycle_shuffle(&mut app);
    let epoch = app.queue.epoch;
    let request = tasks.smart.request;
    tasks.cycle_shuffle(&mut app);
    tasks.cycle_shuffle(&mut app);
    tasks.cycle_shuffle(&mut app);
    background(
        &mut app,
        &mut tasks,
        Background::SmartRecommendations(epoch, request, batch()),
    );
    assert_eq!(app.queue.ids.len(), 9);
    assert!(tasks.smart.handle.is_some());
    let request = tasks.smart.request;
    app.queue.clear();
    background(
        &mut app,
        &mut tasks,
        Background::SmartRecommendations(epoch, request, batch()),
    );
    assert!(app.queue.ids.is_empty());
    tasks.sync_queue_epoch(app.queue.epoch);
    assert!(tasks.smart.handle.is_none());
}

#[tokio::test]
async fn metadata_delays_requests_and_errors_stop_automatic_retries() {
    let mut app = app();
    app.cache = crate::cache::MetadataCache::default();
    let (mut tasks, _) = tasks();
    tasks.cycle_shuffle(&mut app);
    tasks.cycle_shuffle(&mut app);
    assert!(tasks.smart.handle.is_none());
    app.cache.insert(test_track(0).id, test_track(0));
    tasks.refill_smart_shuffle(&app);
    assert!(tasks.smart.handle.is_some());
    let request = tasks.smart.request;
    let epoch = app.queue.epoch;
    background(
        &mut app,
        &mut tasks,
        Background::SmartRecommendations(epoch, request, Err(anyhow::anyhow!("HTTP 503"))),
    );
    tasks.refill_smart_shuffle(&app);
    assert!(tasks.smart.handle.is_none());
    assert!(app.status.contains("503"));
    assert_eq!(app.queue.ids.len(), 9);
}

#[tokio::test]
async fn smart_cancels_radio_and_late_radio_response_cannot_refill_after_disable() {
    let mut app = app();
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    tasks.start_radio(&mut app, test_track(0), &tx);
    let epoch = app.queue.epoch;
    tasks.cycle_shuffle(&mut app);
    tasks.cycle_shuffle(&mut app);
    assert!(!tasks.radio_active);
    assert!(tasks.recommendations.is_none());
    tasks.cycle_shuffle(&mut app);
    background(
        &mut app,
        &mut tasks,
        Background::Recommendations(epoch, batch()),
    );
    assert_eq!(app.queue.ids.len(), 1);
}
