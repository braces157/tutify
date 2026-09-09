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
    Ok(Recommendations {
        tracks: vec![
            test_track(0),
            test_track(20),
            test_track(20),
            unavailable,
            test_track(21),
            test_track(22),
        ],
        source: crate::catalog::RecommendationSource::ArtistSearch,
    })
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
    assert!(app.status.contains("Artist-search suggestions"));
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
