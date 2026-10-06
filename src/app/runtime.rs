use super::*;

const GLASS_ORIENTATION_DEBOUNCE: Duration = Duration::from_millis(300);
const GLASS_ORIENTATION_RETRY: Duration = Duration::from_secs(2);

/// Frame periods are relative to the last frame; remaining debounce/fade delays
/// are relative to now. Mixing those origins makes an expired timer spin.
fn refresh_deadline(
    now: Instant,
    last_draw: Instant,
    dirty: bool,
    animation: Option<Duration>,
    idle_refresh: Option<Duration>,
    orientation_delay: Option<Duration>,
) -> Option<Instant> {
    let frame = if dirty {
        Some(Duration::from_millis(33))
    } else {
        animation
    };
    frame
        .map(|delay| last_draw + delay)
        .into_iter()
        .chain(idle_refresh.map(|delay| now + delay))
        .chain(orientation_delay.map(|delay| now + delay))
        .min()
}

fn load_history(app: &mut App, store: &Storage) -> Result<()> {
    // Unlike disposable metadata, history must never be silently replaced by
    // an empty snapshot after a parse, version, or filesystem error.
    app.stats = store.stats().map_err(|error| {
        anyhow::anyhow!("Could not load statistics; stats.json has been preserved: {error:#}")
    })?;
    app.mix_recipes = store.mix_recipes().map_err(|error| {
        anyhow::anyhow!(
            "Could not load mix recipes; mix-recipes.json has been preserved: {error:#}"
        )
    })?;
    Ok(())
}

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

pub async fn run_session(
    store: Storage,
    native_glass: bool,
    glass: bool,
    source: crate::model::MusicSource,
    notice: &str,
) -> Result<crate::model::SessionRequest> {
    let mut config = store.config()?;
    config.source = source;
    config.native_glass = native_glass;
    if native_glass || glass {
        config.theme = "glass".into();
    }
    let queue = store.queue()?;
    let mut app = App::new(config, queue);
    load_history(&mut app, &store)?;
    let services = crate::providers::open(source, &app.config, app.visualizer.clone())?;
    app.config.youtube_music = services.music_catalog;
    app.config.youtube_connected =
        source == crate::model::MusicSource::Youtube && services.library_connected;
    if source == crate::model::MusicSource::Youtube {
        app.catalog.search_scope = SearchScope::Youtube;
        app.catalog.title = if services.music_catalog {
            "YouTube Music search"
        } else {
            "YouTube search"
        }
        .into();
        app.status =
            "Music is ready. / searches; F6 connects accounts and repairs playback.".into();
    }
    let catalog = services.catalog;
    let mut playback = services.playback;
    if !notice.is_empty() {
        app.status = notice.into();
        if notice.contains("unavailable")
            || notice.contains("timed out")
            || notice.contains("could not confirm")
        {
            app.ui
                .diagnostics
                .history
                .record_text(Subsystem::Catalog, notice);
        }
    }
    match store.cache() {
        Ok(cache) => app.cache = cache,
        Err(_) => app.status = "Metadata cache unavailable and preserved; names will reload. Use state inspect cache or clear-cache.".into(),
    }
    app.stats.refresh_metadata(&app.cache);
    let (bg_tx, mut bg_rx) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(catalog, bg_tx.clone())?;
    let (config_tx, config_rx) = watch::channel(None);
    let (queue_tx, queue_rx) = watch::channel::<Option<Arc<Queue>>>(None);
    let (cache_tx, cache_rx) = watch::channel::<Option<Arc<crate::cache::MetadataCache>>>(None);
    let (stats_tx, stats_rx) = watch::channel::<Option<Arc<SongStats>>>(None);
    let (recipes_tx, recipes_rx) = watch::channel::<Option<Arc<MixRecipes>>>(None);
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
    let mut published_media = None;
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
            if app.check_sleep(Instant::now(), &playback.commands) { dirty = true; }
            app.catalog_health = tasks.catalog.health();
            let media_stamp = (app.generation, app.state, app.queue.position_ms / 1000, app.cache.revision);
            if published_media != Some(media_stamp) {
                if let Some(controls) = &mut media_controls {
                    controls.update(media_controls::Snapshot {
                        track: app.current_track(), state: app.state, position_ms: app.queue.position_ms,
                    });
                }
                discord_presence.update(app.discord_snapshot());
                published_media = Some(media_stamp);
            }
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
            let now = Instant::now();
            let deadline = refresh_deadline(now, last_draw, dirty, animation, idle_refresh, orientation_delay)
                .into_iter().chain(app.ui.listening.sleep.refresh_at(now)).min();
            // The branch is disabled when no refresh is scheduled.
            let wake_at = tokio::time::Instant::from_std(deadline.unwrap_or(last_draw));
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
                        app.ui.diagnostics.history.record_text(Subsystem::Playback, "Playback worker exited");
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
                            Err(error) => {
                                app.ui.diagnostics.history.record_text(Subsystem::Terminal, &error);
                                app.status = format!("Glass orientation update failed: {error}. Retrying shortly.");
                            },
                        }
                    }
                    dirty = true;
                }
                _ = tokio::time::sleep_until(wake_at), if deadline.is_some() => {
                    if animation.is_some() { app.animation_frame = app.animation_frame.wrapping_add(1); }
                    dirty = true;
                }
                _ = save_tick.tick() => {
                    tasks.catalog.release_idle_helpers();
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
    result?;
    Ok(app.session_request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::background::Orientation::{Horizontal, Vertical};

    #[test]
    fn refresh_timers_use_their_own_time_origin() {
        let now = Instant::now();
        let old_frame = now - Duration::from_secs(10);
        let delay = Duration::from_millis(300);
        assert_eq!(
            refresh_deadline(now, old_frame, false, None, None, Some(delay)),
            Some(now + delay)
        );
        assert_eq!(
            refresh_deadline(now, old_frame, false, None, Some(delay), None),
            Some(now + delay)
        );
        assert_eq!(
            refresh_deadline(now, old_frame, false, None, None, None),
            None
        );
        let last_frame = now - Duration::from_millis(10);
        assert_eq!(
            refresh_deadline(now, last_frame, true, None, None, Some(delay)),
            Some(last_frame + Duration::from_millis(33))
        );
        assert_eq!(
            refresh_deadline(
                now,
                last_frame,
                false,
                Some(Duration::from_millis(33)),
                Some(delay),
                None
            ),
            Some(last_frame + Duration::from_millis(33))
        );
    }

    #[test]
    fn startup_preserves_invalid_history_and_accepts_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage {
            root: dir.path().to_owned(),
        };
        let mut app = App::new(Config::default(), Queue::default());
        load_history(&mut app, &store).unwrap();
        for (name, contents) in [
            ("stats.json", "broken"),
            ("stats.json", r#"{"version":2,"tracks":{}}"#),
            ("mix-recipes.json", "broken"),
            ("mix-recipes.json", r#"{"version":2,"recipes":[]}"#),
        ] {
            let path = store.root.join(name);
            std::fs::write(&path, contents).unwrap();
            let error = load_history(&mut app, &store).unwrap_err().to_string();
            assert!(error.contains(name), "{error}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
            std::fs::remove_file(path).unwrap();
        }
    }

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
