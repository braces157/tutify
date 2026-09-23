use super::*;

const GLASS_ORIENTATION_DEBOUNCE: Duration = Duration::from_millis(300);
const GLASS_ORIENTATION_RETRY: Duration = Duration::from_secs(2);

#[derive(Default)]
struct GlassOrientationSync {
    desired: Option<crate::ui::background::Orientation>,
    desired_since: Option<Instant>,
    applied: Option<crate::ui::background::Orientation>,
    in_flight: bool,
    retry_after: Option<Instant>,
}

impl GlassOrientationSync {
    fn observe(&mut self, orientation: crate::ui::background::Orientation, now: Instant) {
        if self.desired != Some(orientation) {
            self.desired = Some(orientation);
            self.desired_since = Some(now);
            self.retry_after = None;
        }
    }

    fn ready(&self, now: Instant) -> Option<crate::ui::background::Orientation> {
        let desired = self.desired?;
        if self.in_flight || self.applied == Some(desired) {
            return None;
        }
        let ready_at = self.desired_since? + GLASS_ORIENTATION_DEBOUNCE;
        let ready_at = self
            .retry_after
            .map_or(ready_at, |retry| retry.max(ready_at));
        (now >= ready_at).then_some(desired)
    }

    fn delay(&self, now: Instant) -> Option<Duration> {
        let desired = self.desired?;
        if self.in_flight || self.applied == Some(desired) {
            return None;
        }
        let ready_at = self.desired_since? + GLASS_ORIENTATION_DEBOUNCE;
        let ready_at = self
            .retry_after
            .map_or(ready_at, |retry| retry.max(ready_at));
        Some(ready_at.saturating_duration_since(now))
    }

    fn complete(
        &mut self,
        orientation: crate::ui::background::Orientation,
        succeeded: bool,
        now: Instant,
    ) {
        self.in_flight = false;
        if succeeded {
            self.applied = Some(orientation);
            if self.desired == Some(orientation) {
                self.retry_after = None;
            }
        } else if self.desired == Some(orientation) {
            self.retry_after = Some(now + GLASS_ORIENTATION_RETRY);
        }
    }
}

pub async fn run(store: Storage, native_glass: bool, glass: bool) -> Result<()> {
    let mut config = store.config()?;
    config.native_glass = native_glass;
    if native_glass || glass {
        config.theme = "glass".into();
    }
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
    let (orientation_tx, mut orientation_rx) = mpsc::unbounded_channel();
    let mut orientation_sync = GlassOrientationSync::default();
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
                if app.config.native_glass {
                    crossterm::execute!(std::io::stdout(), crossterm::terminal::BeginSynchronizedUpdate)?;
                }
                let draw_result = ui::draw_terminal(&mut terminal.terminal, &app);
                if app.config.native_glass {
                    crossterm::execute!(std::io::stdout(), crossterm::terminal::EndSynchronizedUpdate)?;
                }
                draw_result?;
                dirty = false; last_draw = Instant::now();
                let viewport = (app.queue.revision, app.queue.cursor, app.ui.render.borrow().queue_scroll, app.ui.render.borrow().queue_height, app.catalog.view);
                if last_metadata_view != Some(viewport) { metadata_dirty = true; last_metadata_view = Some(viewport); }
                if app.config.native_glass
                    && crate::ui::Theme::from_str(&app.config.theme) == crate::ui::Theme::Glass
                    && let Some(orientation) = app.ui.render.borrow().background.orientation()
                {
                    orientation_sync.observe(orientation, Instant::now());
                }
            }
            if let Some(orientation) = orientation_sync.ready(Instant::now()) {
                orientation_sync.in_flight = true;
                let config = app.config.clone();
                let tx = orientation_tx.clone();
                tokio::spawn(async move {
                    let result = tokio::task::spawn_blocking(move || {
                        crate::terminal_profile::switch_orientation(orientation, &config)
                    })
                    .await;
                    let result = match result {
                        Ok(Ok(())) => Ok(()),
                        Ok(Err(error)) => Err(format!("{error:#}")),
                        Err(error) => Err(format!("Glass profile worker failed: {error}")),
                    };
                    let _ = tx.send((orientation, result));
                });
            }
            if metadata_dirty { tasks.metadata(&app); metadata_dirty = false; }
            let animation = app.animation_interval();
            let idle_refresh = app.idle_refresh_interval();
            let orientation_delay = orientation_sync.delay(Instant::now());
            let delay = if dirty {
                Duration::from_millis(33)
            } else {
                animation
                    .into_iter()
                    .chain(idle_refresh)
                    .chain(orientation_delay)
                    .min()
                    .unwrap_or(Duration::from_secs(1))
            };
            tokio::select! {
                Some(event) = media_rx.recv() => {
                    app.note_user_interaction();
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
                            app.note_user_interaction();
                            if !route_input(&mut app, event, &mut tasks, &playback.commands) {
                                dirty = true;
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
                Some((orientation, result)) = orientation_rx.recv() => {
                    let current_request = orientation_sync.desired == Some(orientation);
                    let succeeded = result.is_ok();
                    orientation_sync.complete(orientation, succeeded, Instant::now());
                    if current_request {
                        match result {
                            Ok(()) if app.status.starts_with("Glass orientation update failed:") => app.status.clear(),
                            Ok(()) => (),
                            Err(error) => app.status = format!("Glass orientation update failed: {error}. Retrying shortly."),
                        }
                    }
                    dirty = true;
                }
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(last_draw + delay)), if dirty || animation.is_some() || idle_refresh.is_some() || orientation_delay.is_some() => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::background::Orientation::{Horizontal, Vertical};

    #[test]
    fn native_orientation_waits_for_resize_to_settle_and_coalesces() {
        let now = Instant::now();
        let mut sync = GlassOrientationSync::default();
        sync.observe(Horizontal, now);
        assert_eq!(
            sync.ready(now + GLASS_ORIENTATION_DEBOUNCE),
            Some(Horizontal)
        );

        sync.observe(Vertical, now + Duration::from_millis(100));
        assert_eq!(sync.ready(now + GLASS_ORIENTATION_DEBOUNCE), None);
        assert_eq!(sync.ready(now + Duration::from_millis(400)), Some(Vertical));

        sync.in_flight = true;
        sync.observe(Horizontal, now + Duration::from_millis(450));
        sync.complete(Vertical, true, now + Duration::from_millis(500));
        assert_eq!(sync.applied, Some(Vertical));
        assert_eq!(
            sync.ready(now + Duration::from_millis(750)),
            Some(Horizontal)
        );
    }

    #[test]
    fn native_orientation_failures_retry_after_a_short_backoff() {
        let now = Instant::now();
        let mut sync = GlassOrientationSync::default();
        sync.observe(Vertical, now);
        sync.in_flight = true;
        let failed_at = now + GLASS_ORIENTATION_DEBOUNCE;
        sync.complete(Vertical, false, failed_at);

        assert_eq!(
            sync.ready(failed_at + GLASS_ORIENTATION_RETRY - Duration::from_millis(1)),
            None
        );
        assert_eq!(
            sync.ready(failed_at + GLASS_ORIENTATION_RETRY),
            Some(Vertical)
        );
    }
}
