use super::*;
use anyhow::{Context, bail};

async fn pump_until(
    app: &mut App,
    tasks: &mut Tasks,
    bg: &mut mpsc::UnboundedReceiver<Background>,
    player: &mut playback::Playback,
    label: &str,
    done: impl Fn(&App, &Tasks) -> bool,
) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(55), async {
        loop {
            tasks.refill_radio(app);
            tasks.refill_smart_shuffle(app);
            if app.state == State::Failed { bail!("{label}: {}", app.status); }
            if app.status.contains("rate limit") { bail!("{label}: {}", app.status); }
            if let Some(error) = &app.radio_error { bail!("{label}: {error}"); }
            if done(app, tasks) { return Ok(()); }
            tokio::select! {
                event = bg.recv() => { background(app, tasks, event.context("Background channel closed")?); }
                event = player.events.recv() => { app.playback_event(event.context("Playback channel closed")?, &player.commands); }
            }
        }
    }).await.with_context(|| format!("Timed out: {label}; {}", app.status))?
}

#[tokio::test]
#[ignore = "Live Tuitify search, radio, refill, Smart Shuffle and Windows audio; uses saved logins"]
async fn live_tuitify_multiple_song_acceptance() -> Result<()> {
    crate::diagnostics::init();
    let store = Storage::local()?;
    let _lock = store.lock()?;
    let mut config = store.config()?;
    config.volume = 10;
    config.shuffle = false;
    config.repeat = crate::model::Repeat::Off;
    config.discord_rpc = false;
    let catalog = Catalog::new(TokenManager::load(&config)?)?;
    let mut player = playback::Playback::spawn(
        TokenManager::load_streaming()?,
        config.client_id.clone(),
        10,
    );
    let (bg_tx, mut bg) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(catalog, bg_tx)?;
    let mut app = App::new(config, Queue::default());
    let mut exercised = 0;
    for query in [
        "track:good 4 u artist:Olivia Rodrigo",
        "track:月面着陸計画 - Live artist:tuki.",
        "track:猫日 artist:suis from Yorushika",
    ] {
        if std::env::var("TUITIFY_LIVE_ARTIST")
            .is_ok_and(|artist| !query.ends_with(&format!("artist:{artist}")))
        {
            continue;
        }
        exercised += 1;
        // Production input routing, real HTTP, real background handlers and
        // librespot/Windows audio. App state stays in memory; no saved queue edits.
        route_input(
            &mut app,
            Input::Key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE)),
            &mut tasks,
            &player.commands,
        );
        if !app.catalog.editing {
            route_input(
                &mut app,
                Input::Key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)),
                &mut tasks,
                &player.commands,
            );
        }
        assert!(app.catalog.editing);
        app.catalog.query.clear();
        route_input(
            &mut app,
            Input::Paste(query.into()),
            &mut tasks,
            &player.commands,
        );
        route_input(
            &mut app,
            Input::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            &mut tasks,
            &player.commands,
        );
        pump_until(
            &mut app,
            &mut tasks,
            &mut bg,
            &mut player,
            "search",
            |app, _| !app.catalog.busy,
        )
        .await?;
        assert!(matches!(&app.catalog.rows, Rows::Tracks(rows) if !rows.is_empty()));
        route_input(
            &mut app,
            Input::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            &mut tasks,
            &player.commands,
        );
        pump_until(
            &mut app,
            &mut tasks,
            &mut bg,
            &mut player,
            "seed and initial radio",
            |app, tasks| {
                app.state == State::Playing
                    && app.queue.position_ms >= 1500
                    && tasks.recommendations.is_none()
            },
        )
        .await?;
        let seed = app.current_track().context("Missing current seed")?.clone();
        let expected = query
            .strip_prefix("track:")
            .unwrap()
            .split(" artist:")
            .next()
            .unwrap();
        assert!(
            seed.name.eq_ignore_ascii_case(expected),
            "Expected {expected}, played {}",
            seed.name
        );
        let initial = app.queue.ids.len();
        assert!(
            initial >= 4,
            "{}: only {initial} queue entries; {}",
            seed.name,
            app.status
        );
        let artists: HashSet<_> = app
            .queue
            .ids
            .iter()
            .filter_map(|id| app.cache.get(id))
            .map(|track| track.artists.clone())
            .collect();
        println!(
            "LIVE {}: initial={initial}, artists={}, seed streaming advanced",
            seed.name,
            artists.len()
        );
        let mut counts = std::collections::HashMap::<String, usize>::new();
        for track in app
            .queue
            .ids
            .iter()
            .skip(1)
            .filter_map(|id| app.cache.get(id))
        {
            println!("  CANDIDATE {} / {}", track.name, track.artists);
            *counts
                .entry(
                    track
                        .artist_ids
                        .first()
                        .cloned()
                        .unwrap_or_else(|| track.artists.clone()),
                )
                .or_default() += 1;
            if seed.artists != "Olivia Rodrigo" {
                assert!(
                    !track.artists.contains("Taylor Swift")
                        && !track.artists.contains("Olivia Rodrigo")
                );
            }
        }
        assert!(
            counts.len() >= 4,
            "not enough different artists: {counts:?}"
        );
        assert!(
            counts.values().all(|n| *n <= 3),
            "one artist dominates: {counts:?}"
        );
        assert_eq!(
            app.radio_source,
            Some(crate::catalog::RecommendationSource::SimilarArtists)
        );

        for _ in 0..2 {
            route_input(
                &mut app,
                Input::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
                &mut tasks,
                &player.commands,
            );
            pump_until(
                &mut app,
                &mut tasks,
                &mut bg,
                &mut player,
                "suggested track playback",
                |app, _| app.state == State::Playing && app.queue.position_ms >= 1500,
            )
            .await?;
            println!(
                "  STREAMED {} - {}",
                app.current_track().unwrap().name,
                app.current_track().unwrap().artists
            );
        }
        app.queue.select(initial - 3);
        app.load(&player.commands);
        pump_until(
            &mut app,
            &mut tasks,
            &mut bg,
            &mut player,
            "anchored refill",
            |app, tasks| {
                app.state == State::Playing
                    && tasks.recommendations.is_none()
                    && tasks.radio_round >= 2
            },
        )
        .await?;
        let refilled = app.queue.ids.len();
        assert!(
            refilled >= initial,
            "{}: refill removed queue entries",
            seed.name
        );
        assert_eq!(tasks.radio_seed.as_ref().unwrap().id, seed.id);
        let recordings: HashSet<_> = app
            .queue
            .ids
            .iter()
            .filter_map(|id| app.cache.get(id))
            .map(crate::catalog::recording_key)
            .collect();
        assert_eq!(
            recordings.len(),
            refilled,
            "duplicate recordings after refill"
        );
        println!(
            "  REFILL added={}, total={refilled}, original seed retained, no duplicate recordings",
            refilled - initial
        );

        // Completion must advance to another playable recommendation itself.
        let old_generation = app.generation;
        let end = app
            .current_track()
            .unwrap()
            .duration_ms
            .saturating_sub(1200);
        app.control(Control::Seek(Seek::Position(end)), &player.commands);
        pump_until(
            &mut app,
            &mut tasks,
            &mut bg,
            &mut player,
            "automatic completion",
            |app, _| {
                app.generation > old_generation
                    && app.state == State::Playing
                    && app.queue.position_ms >= 500
            },
        )
        .await?;

        // Exercise the separate Smart Shuffle mode on the real fetched queue.
        let originals = app.queue.ids.iter().take(7).cloned().collect();
        tasks.cancel_radio(&mut app);
        app.queue.replace(originals, 0, false);
        app.load(&player.commands);
        tasks.cycle_shuffle(&mut app);
        tasks.cycle_shuffle(&mut app);
        pump_until(
            &mut app,
            &mut tasks,
            &mut bg,
            &mut player,
            "Smart Shuffle",
            |app, tasks| !app.queue.suggestions.is_empty() && tasks.smart.handle.is_none(),
        )
        .await?;
        app.queue.validate()?;
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 32))?;
        terminal.draw(|frame| ui::draw(frame, &app, &mut app.ui.render.borrow_mut()))?;
        let screen: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(screen.contains("SHUF:SMART") && screen.contains('✦'));
        let mut smart_artists = HashSet::new();
        for index in &app.queue.suggestions {
            let track = app.cache.get(&app.queue.ids[*index]).unwrap();
            println!("  SMART {} / {}", track.name, track.artists);
            assert!(
                !track
                    .artist_ids
                    .iter()
                    .any(|id| seed.artist_ids.contains(id)),
                "Smart returned the seed artist"
            );
            smart_artists.insert(
                track
                    .artist_ids
                    .first()
                    .cloned()
                    .unwrap_or_else(|| track.artists.clone()),
            );
        }
        assert!(
            smart_artists.len() >= 2,
            "Smart must discover multiple other artists"
        );
        println!(
            "  SMART suggestions={}, native TUI render and automatic track advance passed",
            app.queue.suggestions.len()
        );
        tasks.cancel_smart_shuffle();
        tasks.cancel_radio(&mut app);
        app.stop(&player.commands);
        app.config.shuffle = false;
    }
    player.commands.send(Command::Stop)?;
    assert!(
        exercised > 0,
        "TUITIFY_LIVE_ARTIST did not match any acceptance seed"
    );
    Ok(())
}
