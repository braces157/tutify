use super::*;

pub async fn run(store: Storage) -> Result<()> {
    let config = store.config()?;
    let queue = store.queue()?;
    let catalog = Catalog::new(TokenManager::load(&config)?)?;
    let mut app = App::new(config, queue);
    let mut playback = playback::Playback::spawn_with_visualizer(
        TokenManager::load_streaming()?,
        app.config.client_id.clone(),
        app.config.volume,
        app.visualizer.clone(),
    );
    match store.cache() {
        Ok(cache) => app.cache = cache,
        Err(_) => app.status = "Old or invalid metadata cache ignored; names will reload. Use clear-cache to remove it.".into(),
    }
    match store.stats() {
        Ok(stats) => app.stats = stats,
        Err(_) => app.status = "Old or invalid song statistics ignored; starting fresh.".into(),
    }
    match store.mix_recipes() {
        Ok(recipes) => app.mix_recipes = recipes,
        Err(_) => app.status = "Old or invalid mix recipes ignored; starting with none.".into(),
    }
    app.stats.refresh_metadata(&app.cache);
    let (bg_tx, mut bg_rx) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(catalog, bg_tx.clone())?;
    let (config_tx, config_rx) = watch::channel(None);
    let (queue_tx, queue_rx) = watch::channel(None);
    let (cache_tx, cache_rx) = watch::channel(None);
    let (stats_tx, stats_rx) = watch::channel(None);
    let (recipes_tx, recipes_rx) = watch::channel(None);
    let config_store = store.clone();
    let queue_store = store.clone();
    let cache_store = store.clone();
    let stats_store = store.clone();
    let recipes_store = store.clone();
    let config_writer = writer(config_rx, bg_tx.clone(), move |config| {
        config_store.save_config(&config)
    });
    let queue_writer = writer(queue_rx, bg_tx.clone(), move |queue| {
        queue_store.save_queue(&queue)
    });
    let cache_writer = writer(cache_rx, bg_tx.clone(), move |cache| {
        cache_store.save_cache(&cache)
    });
    let stats_writer = writer(stats_rx, bg_tx, move |stats| stats_store.save_stats(&stats));
    let recipes_writer = writer(recipes_rx, tasks.tx.clone(), move |recipes| {
        recipes_store.save_mix_recipes(&recipes)
    });
    let mut checkpoints = Checkpoints {
        config: app.config.clone(),
        queue: queue_stamp(&app.queue),
        cache: 0,
        stats: 0,
        recipes: app.mix_recipes.revision,
        retry: false,
        config_tx,
        queue_tx,
        cache_tx,
        stats_tx,
        recipes_tx,
    };
    let mut terminal = ui::TerminalGuard::enter()?;
    let (media_tx, mut media_rx) = mpsc::unbounded_channel();
    let mut media_controls = match media_controls::MediaControls::spawn(media_tx) {
        Ok(controls) => Some(controls),
        Err(_) => {
            app.status = "Windows media controls unavailable; terminal controls still work.".into();
            None
        }
    };
    let mut discord_presence = crate::discord::DiscordPresence::spawn(
        app.config.discord_rpc,
        app.config.discord_client_id.clone(),
    );
    let mut current_window_title = String::new();
    let mut keys = EventStream::new();
    let mut save_tick = tokio::time::interval(Duration::from_secs(2));
    save_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut playback_open = true;
    let mut dirty = true;
    let mut metadata_dirty = true;
    let mut lyrics_dirty = true;
    let mut last_metadata_view = None;
    let mut last_draw = Instant::now() - Duration::from_millis(33);
    let result: Result<()> = async {
        loop {
            app.catalog_health = tasks.catalog.health();
            if let Some(controls) = &mut media_controls {
                controls.update(media_controls::Snapshot {
                    track: app.current_track(), state: app.state, position_ms: app.queue.position_ms,
                });
            }
            discord_presence.update(app.discord_snapshot());
            tasks.sync_queue_epoch(app.queue.epoch);
            tasks.refill_radio(&app);
            tasks.refill_smart_shuffle(&app);
            if lyrics_dirty { tasks.update_lyrics(&mut app); lyrics_dirty = false; }
            if dirty && last_draw.elapsed() >= Duration::from_millis(33) {
                app.interpolate_position();
                app.account_playback_time(Instant::now());
                let title = app.window_title();
                if title != current_window_title { ui::set_title(&title); current_window_title = title; }
                terminal.terminal.draw(|frame| ui::draw(frame, &app, &mut app.ui.render.borrow_mut()))?;
                dirty = false; last_draw = Instant::now();
                let viewport = (app.queue.revision, app.queue.cursor, app.ui.render.borrow().queue_scroll, app.ui.render.borrow().queue_height, app.catalog.view);
                if last_metadata_view != Some(viewport) { metadata_dirty = true; last_metadata_view = Some(viewport); }
            }
            if metadata_dirty { tasks.metadata(&app); metadata_dirty = false; }
            let animation = app.animation_interval();
            let delay = if dirty { Duration::from_millis(33) } else { animation.unwrap_or(Duration::from_secs(1)) };
            tokio::select! {
                Some(event) = media_rx.recv() => {
                    match event {
                        media_controls::Event::Action(action) => app.media_action(action, &playback.commands),
                        media_controls::Event::Unavailable => {
                            media_controls = None;
                            app.status = "Windows media controls unavailable; terminal controls still work.".into();
                        }
                    }
                    app.ui.render.borrow_mut().mouse_hits.clear();
                    dirty = true; metadata_dirty = true; lyrics_dirty = true;
                }
                key_event = keys.next() => {
                    match key_event {
                        Some(Ok(event)) => {
                            if !route_input(&mut app, event, &mut tasks, &playback.commands) {
                                continue;
                            }
                        }
                        Some(Err(e)) => return Err(e.into()), None => break,
                    }
                    app.ui.render.borrow_mut().mouse_hits.clear();
                    dirty = true; metadata_dirty = true; lyrics_dirty = true;
                }
                event = playback.events.recv(), if playback_open => {
                    if let Some(event) = event { app.playback_event(event, &playback.commands); }
                    else {
                        playback_open = false;
                        if app.state == State::Playing {
                            app.finalize_playback_accounting();
                        }
                        app.loaded = false;
                        app.state = State::Failed;
                        app.status = "Playback worker exited; restart Tuitify.".into();
                    }
                    app.ui.render.borrow_mut().mouse_hits.clear();
                    dirty = true; metadata_dirty = true; lyrics_dirty = true;
                }
                Some(event) = bg_rx.recv() => {
                    checkpoints.retry |= background(&mut app, &mut tasks, event);
                    // Process bursts together; bound the batch so keyboard input stays fair.
                    for _ in 0..63 { match bg_rx.try_recv() { Ok(event) => checkpoints.retry |= background(&mut app, &mut tasks, event), Err(_) => break } }
                    app.check_preload(&playback.commands);
                    app.ui.render.borrow_mut().mouse_hits.clear();
                    dirty = true; metadata_dirty = true; lyrics_dirty = true;
                }
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(last_draw + delay)), if dirty || animation.is_some() => {
                    if animation.is_some() { app.animation_frame = app.animation_frame.wrapping_add(1); }
                    dirty = true;
                }
                _ = save_tick.tick() => {
                    app.interpolate_position();
                    app.account_playback_time(Instant::now());
                    if app.cache.prune_expired() { dirty = true; metadata_dirty = true; lyrics_dirty = true; }
                    checkpoints.send(&app);
                }
                _ = tokio::signal::ctrl_c() => break,
            }
            if app.quit { break; }
        }
        Ok(())
    }.await;
    app.interpolate_position();
    app.finalize_playback_accounting();
    app.send(&playback.commands, Command::Stop);
    checkpoints.send(&app);
    drop(checkpoints);
    drop(tasks);
    drop(terminal);
    drop(media_controls);
    discord_presence.close().await;
    let (config_saved, queue_saved, cache_saved, stats_saved, recipes_saved) = tokio::join!(
        config_writer,
        queue_writer,
        cache_writer,
        stats_writer,
        recipes_writer
    );
    config_saved??;
    queue_saved??;
    cache_saved??;
    stats_saved??;
    recipes_saved??;
    result
}
