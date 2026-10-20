use super::*;

fn playing_queue(count: usize) -> App {
    let mut queue = Queue::default();
    queue.replace((1..=count).map(|i| test_track(i).id).collect(), 0, false);
    let mut app = App::new(Config::default(), queue);
    for i in 1..=count {
        app.cache.insert(test_track(i).id, test_track(i));
    }
    app.loaded = true;
    app.state = State::Playing;
    app
}

fn demo_tasks(app: &App) -> (Tasks, mpsc::UnboundedReceiver<Background>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut tasks = Tasks::demo(tx).unwrap();
    tasks.sync_queue_epoch(app.queue.epoch);
    (tasks, rx)
}

fn recommendations(epoch: u64, tracks: Vec<Track>) -> Background {
    Background::Autoplay(
        epoch,
        1,
        Ok(Recommendations {
            tracks,
            source: crate::catalog::RecommendationSource::ArtistSearch,
        }),
    )
}

#[tokio::test]
async fn prefetch_preserves_chosen_order_and_manual_additions_take_priority() {
    let mut app = playing_queue(5);
    let (mut tasks, mut bg) = demo_tasks(&app);
    let (tx, _) = mpsc::unbounded_channel();
    tasks.update_autoplay(&mut app, &tx);
    assert!(!tasks.autoplay.pending); // Four songs still remain.
    app.queue.select(1);
    tasks.update_autoplay(&mut app, &tx);
    assert!(tasks.autoplay.pending);
    tasks.update_autoplay(&mut app, &tx);
    assert!(matches!(
        bg.try_recv().unwrap(),
        Background::Autoplay(_, _, _)
    ));
    assert!(bg.try_recv().is_err()); // Only one request while pending.
    let epoch = app.queue.epoch;
    let mut duplicate_recording = test_track(2);
    duplicate_recording.id = test_track(98).id;
    let mut unavailable = test_track(99);
    unavailable.playable = false;
    background(
        &mut app,
        &mut tasks,
        recommendations(
            epoch,
            vec![
                test_track(2),
                duplicate_recording,
                unavailable,
                test_track(6),
                test_track(7),
            ],
        ),
    );
    assert_eq!(
        app.queue.ids,
        (1..=7).map(|i| test_track(i).id).collect::<Vec<_>>()
    );
    assert_eq!(app.queue.current(), Some(test_track(2).id.as_str()));
    assert!(app.enqueue_manual(test_track(8).id));
    let order: Vec<_> = app
        .queue
        .order
        .iter()
        .map(|&i| app.queue.ids[i].clone())
        .collect();
    assert_eq!(order, [1, 2, 3, 4, 5, 8, 6, 7].map(|i| test_track(i).id));
    app.queue.validate().unwrap();
}

#[tokio::test]
async fn last_song_waits_for_slow_recommendations_and_loads_the_first_new_song() {
    let mut app = playing_queue(1);
    let (mut tasks, mut bg) = demo_tasks(&app);
    let (tx, mut commands) = mpsc::unbounded_channel();
    tasks.update_autoplay(&mut app, &tx);
    app.playback_event(Event::Completed(app.generation), &tx);
    let stopped_generation = app.generation;
    assert!(matches!(commands.try_recv().unwrap(), Command::Stop));
    tasks.update_autoplay(&mut app, &tx);
    assert!(app.autoplay_waiting.is_some());
    assert!(commands.try_recv().is_err());
    background(&mut app, &mut tasks, bg.try_recv().unwrap());
    let expected = app.queue.ids[1].clone();
    tasks.update_autoplay(&mut app, &tx);
    assert_eq!(app.state, State::Loading);
    assert_eq!(app.queue.current(), Some(expected.as_str()));
    assert_eq!(app.queue.position_ms, 0);
    assert!(app.autoplay_waiting.is_none());
    assert!(
        matches!(commands.try_recv().unwrap(), Command::Load { id, generation, .. }
        if id == expected && generation > stopped_generation)
    );
    assert!(commands.try_recv().is_err());
}

#[tokio::test]
async fn completion_can_start_discovery_even_without_an_earlier_prefetch() {
    let mut app = playing_queue(1);
    let (mut tasks, mut bg) = demo_tasks(&app);
    let (tx, _commands) = mpsc::unbounded_channel();
    app.playback_event(Event::Completed(app.generation), &tx);
    tasks.update_autoplay(&mut app, &tx);
    assert!(tasks.autoplay.pending);
    background(&mut app, &mut tasks, bg.try_recv().unwrap());
    tasks.update_autoplay(&mut app, &tx);
    assert_eq!(app.queue.cursor, Some(1));
    assert_eq!(app.state, State::Loading);
}

#[tokio::test]
async fn pause_or_stop_cancels_delayed_restart_but_keeps_fetched_songs() {
    for explicit_stop in [false, true] {
        let mut app = playing_queue(1);
        let (mut tasks, mut bg) = demo_tasks(&app);
        let (tx, mut commands) = mpsc::unbounded_channel();
        tasks.update_autoplay(&mut app, &tx);
        app.playback_event(Event::Completed(app.generation), &tx);
        if explicit_stop {
            app.stop(&tx);
        } else {
            app.media_action(MediaAction::Pause, &tx);
        }
        while commands.try_recv().is_ok() {}
        background(&mut app, &mut tasks, bg.try_recv().unwrap());
        tasks.update_autoplay(&mut app, &tx);
        assert!(app.queue.ids.len() > 1);
        assert_eq!(app.queue.cursor, Some(0));
        assert_eq!(app.state, State::Paused);
        assert!(commands.try_recv().is_err());
    }
}

#[tokio::test]
async fn repeat_and_sleep_timers_take_priority_over_autoplay() {
    use crate::app::listening::SleepTimer;
    for repeat in [crate::model::Repeat::Queue, crate::model::Repeat::Track] {
        let mut app = playing_queue(1);
        let (mut tasks, _) = demo_tasks(&app);
        let (tx, _commands) = mpsc::unbounded_channel();
        app.config.repeat = repeat;
        tasks.update_autoplay(&mut app, &tx);
        assert!(!tasks.autoplay.pending);
        app.playback_event(Event::Completed(app.generation), &tx);
        assert_eq!(app.queue.current(), Some(test_track(1).id.as_str()));
        assert!(app.autoplay_waiting.is_none());
    }
    for timed in [false, true] {
        let mut app = playing_queue(1);
        let (mut tasks, mut bg) = demo_tasks(&app);
        let (tx, mut commands) = mpsc::unbounded_channel();
        tasks.update_autoplay(&mut app, &tx);
        app.ui.listening.sleep = if timed {
            SleepTimer::Deadline(Instant::now() - Duration::from_secs(1))
        } else {
            SleepTimer::EndOfTrack(app.generation)
        };
        app.playback_event(Event::Completed(app.generation), &tx);
        while commands.try_recv().is_ok() {}
        background(&mut app, &mut tasks, bg.try_recv().unwrap());
        tasks.update_autoplay(&mut app, &tx);
        assert_eq!(app.state, State::Paused);
        assert_eq!(app.queue.cursor, Some(0));
        assert!(app.autoplay_waiting.is_none());
        assert!(commands.try_recv().is_err());
    }
}

#[tokio::test]
async fn changing_or_clearing_queue_rejects_old_results_and_restart_intent() {
    for clear in [false, true] {
        let mut app = playing_queue(1);
        let (mut tasks, mut bg) = demo_tasks(&app);
        let (tx, _commands) = mpsc::unbounded_channel();
        tasks.update_autoplay(&mut app, &tx);
        app.playback_event(Event::Completed(app.generation), &tx);
        let stale = bg.try_recv().unwrap();
        if clear {
            app.queue.clear();
        } else {
            app.queue.replace(vec![test_track(9).id], 0, false);
        }
        tasks.sync_queue_epoch(app.queue.epoch);
        background(&mut app, &mut tasks, stale);
        tasks.update_autoplay(&mut app, &tx);
        assert!(!tasks.autoplay.pending);
        assert_eq!(app.queue.ids.len(), usize::from(!clear));
        assert!(app.autoplay_waiting.is_none());
        assert!(app.autoplay_epoch.is_none());
        assert_eq!(app.state, State::Paused);
    }
}

#[tokio::test]
async fn empty_or_failed_discovery_stops_without_a_request_loop() {
    for failed in [false, true] {
        let mut app = playing_queue(1);
        let (mut tasks, mut bg) = demo_tasks(&app);
        let (tx, _commands) = mpsc::unbounded_channel();
        tasks.update_autoplay(&mut app, &tx);
        let _ = bg.try_recv().unwrap();
        app.playback_event(Event::Completed(app.generation), &tx);
        let event = if failed {
            Background::Autoplay(
                app.queue.epoch,
                1,
                Err(anyhow::anyhow!("provider unavailable")),
            )
        } else {
            recommendations(app.queue.epoch, vec![test_track(1)])
        };
        background(&mut app, &mut tasks, event);
        for _ in 0..5 {
            tasks.update_autoplay(&mut app, &tx);
        }
        assert!(!tasks.autoplay.pending);
        assert!(bg.try_recv().is_err());
        assert!(app.autoplay_waiting.is_none());
        assert_eq!(app.state, State::Paused);
        assert_eq!(app.queue.ids.len(), 1);
        assert!(app.status.contains(if failed {
            "provider unavailable"
        } else {
            "No more"
        }));
    }
}

#[tokio::test]
async fn smart_shuffle_tail_continues_and_restored_queue_waits_until_played() {
    let mut app = playing_queue(1);
    app.queue.smart_shuffle = true;
    app.config.shuffle = true;
    app.state = State::Paused;
    app.loaded = false;
    let (mut tasks, mut bg) = demo_tasks(&app);
    let (tx, _commands) = mpsc::unbounded_channel();
    tasks.update_autoplay(&mut app, &tx);
    assert!(!tasks.autoplay.pending);
    app.load(&tx);
    tasks.update_autoplay(&mut app, &tx);
    background(&mut app, &mut tasks, bg.try_recv().unwrap());
    app.playback_event(Event::Completed(app.generation), &tx);
    assert_eq!(app.queue.cursor, Some(1));
    assert!(app.queue.smart_shuffle);
    app.queue.validate().unwrap();
}

#[tokio::test]
async fn slow_track_radio_batch_also_resumes_after_completion() {
    let mut app = playing_queue(1);
    let (mut tasks, _bg) = demo_tasks(&app);
    let (tx, _commands) = mpsc::unbounded_channel();
    tasks.start_radio(&mut app, test_track(1), &tx);
    // Use a pending handle as in the real network path.
    tasks.recommendations = Some(tokio::spawn(std::future::pending()));
    app.playback_event(Event::Completed(app.generation), &tx);
    tasks.update_autoplay(&mut app, &tx);
    assert!(app.autoplay_waiting.is_some());
    assert!(!tasks.autoplay.pending);
    tasks.recommendations.take().unwrap().abort();
    let epoch = app.queue.epoch;
    background(
        &mut app,
        &mut tasks,
        Background::Recommendations(
            epoch,
            Ok(Recommendations {
                tracks: vec![test_track(2)],
                source: crate::catalog::RecommendationSource::Spotify,
            }),
        ),
    );
    tasks.update_autoplay(&mut app, &tx);
    assert_eq!(app.queue.current(), Some(test_track(2).id.as_str()));
    assert_eq!(app.state, State::Loading);
}

#[tokio::test]
async fn next_at_the_tail_waits_for_autoplay() {
    let mut app = playing_queue(1);
    let (mut tasks, mut bg) = demo_tasks(&app);
    let (tx, _commands) = mpsc::unbounded_channel();
    app.media_action(MediaAction::Next, &tx);
    tasks.update_autoplay(&mut app, &tx);
    background(&mut app, &mut tasks, bg.try_recv().unwrap());
    tasks.update_autoplay(&mut app, &tx);
    assert_eq!(app.queue.cursor, Some(1));
    assert_eq!(app.state, State::Loading);
}

#[tokio::test]
async fn network_refills_stay_anchored_across_multiple_completed_songs() {
    use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::path};
    let server = MockServer::start().await;
    Mock::given(path("/search"))
        .respond_with(|request: &Request| {
            let query: std::collections::HashMap<_, _> =
                request.url.query_pairs().into_owned().collect();
            assert_eq!(query["q"], "artist:\"Artist\"");
            let offset: usize = query["offset"].parse().unwrap();
            let id = 2 + offset / 20;
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(20))
                .set_body_json(serde_json::json!({"tracks":{"next":null,"items":[{
                    "id":format!("{id:022}"), "name":format!("Song {id}"),
                    "type":"track", "is_playable":true, "duration_ms":200000,
                    "artists":[
                        {"id":format!("{:022}",100 + offset),"name":format!("Guest {offset}")},
                        {"id":format!("{:022}",1),"name":"Artist"}
                    ]
                }]}}))
        })
        .expect(2)
        .mount(&server)
        .await;
    let (bg_tx, mut bg) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(Catalog::mock(&server.uri()), bg_tx).unwrap();
    let mut app = playing_queue(1);
    tasks.sync_queue_epoch(app.queue.epoch);
    let (tx, _commands) = mpsc::unbounded_channel();
    for expected in [2, 3] {
        tasks.update_autoplay(&mut app, &tx);
        app.playback_event(Event::Completed(app.generation), &tx);
        tasks.update_autoplay(&mut app, &tx);
        assert!(app.autoplay_waiting.is_some());
        let event = tokio::time::timeout(Duration::from_secs(3), bg.recv())
            .await
            .unwrap()
            .unwrap();
        background(&mut app, &mut tasks, event);
        tasks.update_autoplay(&mut app, &tx);
        assert_eq!(app.queue.current(), Some(test_track(expected).id.as_str()));
        assert_eq!(app.state, State::Loading);
        app.playback_event(
            Event::Playing {
                generation: app.generation,
                position_ms: 0,
            },
            &tx,
        );
    }
    assert_eq!(app.queue.ids.len(), 3);
    app.queue.validate().unwrap();
}

#[tokio::test]
async fn restored_queue_skips_discovery_pages_it_already_contains() {
    use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::path};
    let server = MockServer::start().await;
    Mock::given(path("/search"))
        .respond_with(|request: &Request| {
            let query: std::collections::HashMap<_, _> =
                request.url.query_pairs().into_owned().collect();
            let id = if query["offset"] == "0" { 1 } else { 2 };
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tracks":{"next":null,"items":[{
                    "id":format!("{id:022}"),"name":format!("Song {id}"),
                    "type":"track","is_playable":true,"duration_ms":200000,
                    "artists":[{"name":"Artist"}]
                }]}
            }))
        })
        .expect(2)
        .mount(&server)
        .await;
    let (bg_tx, mut bg) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(Catalog::mock(&server.uri()), bg_tx).unwrap();
    let mut app = playing_queue(1);
    tasks.sync_queue_epoch(app.queue.epoch);
    let (tx, _commands) = mpsc::unbounded_channel();
    tasks.update_autoplay(&mut app, &tx);
    let event = tokio::time::timeout(Duration::from_secs(3), bg.recv())
        .await
        .unwrap()
        .unwrap();
    background(&mut app, &mut tasks, event);
    assert_eq!(app.queue.ids, [test_track(1).id, test_track(2).id]);
}
