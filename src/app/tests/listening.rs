use super::*;
use crate::{app::listening::SleepTimer, model::Repeat};

fn fixture() -> App {
    let mut app = App::new(Config::default(), Queue::default());
    for i in 0..5 {
        let track = test_track(i);
        app.cache.insert(track.id.clone(), track);
    }
    app.queue.replace(
        vec![
            test_track(0).id,
            test_track(1).id,
            test_track(2).id,
            test_track(1).id,
            test_track(3).id,
            test_track(3).id,
            test_track(4).id,
        ],
        1,
        false,
    );
    app.queue.position_ms = 23_000;
    app.loaded = true;
    app.state = State::Playing;
    app.catalog.view = View::Queue;
    app
}

fn apply(app: &mut App, tasks: &mut Tasks, tx: &mpsc::UnboundedSender<Command>, at: usize) {
    app.open_listening_tools();
    app.ui.listening.selected = at;
    key(
        app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        tasks,
        tx,
    );
}

#[tokio::test]
async fn sleep_presets_and_cancel_leave_music_and_saved_settings_unchanged() {
    let mut app = fixture();
    let config = app.config.clone();
    let queue = serde_json::to_string(&app.queue).unwrap();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    for (at, minutes) in [15, 30, 45, 60].into_iter().enumerate() {
        let before = Instant::now();
        apply(&mut app, &mut tasks, &tx, at);
        let SleepTimer::Deadline(deadline) = app.ui.listening.sleep else {
            panic!("missing deadline")
        };
        assert!(deadline >= before + Duration::from_secs(minutes * 60));
        assert!(deadline <= Instant::now() + Duration::from_secs(minutes * 60));
        assert_eq!(app.ui.overlay, Overlay::None);
    }
    apply(&mut app, &mut tasks, &tx, 5);
    assert_eq!(app.ui.listening.sleep, SleepTimer::Off);
    assert_eq!(app.config, config);
    assert_eq!(serde_json::to_string(&app.queue).unwrap(), queue);
    assert!(rx.try_recv().is_err());
}

#[test]
fn sleep_expiry_is_one_shot_and_retains_queue_position_and_volume() {
    let mut app = fixture();
    let ids = app.queue.ids.clone();
    let order = app.queue.order.clone();
    let now = Instant::now();
    app.ui.listening.sleep = SleepTimer::Deadline(now + Duration::from_secs(15));
    let (tx, mut rx) = mpsc::unbounded_channel();
    assert!(!app.check_sleep(now + Duration::from_secs(14), &tx));
    assert!(rx.try_recv().is_err());
    assert!(app.check_sleep(now + Duration::from_secs(15), &tx));
    assert_eq!(app.state, State::Paused);
    assert!(!app.loaded);
    assert_eq!(app.queue.position_ms, 23_000);
    assert_eq!(app.queue.ids, ids);
    assert_eq!(app.queue.order, order);
    assert_eq!(app.config.volume, 50);
    assert!(matches!(rx.try_recv().unwrap(), Command::Stop));
    assert!(!app.check_sleep(now + Duration::from_secs(16), &tx));
    assert!(rx.try_recv().is_err());
    app.media_action(MediaAction::Play, &tx);
    assert!(matches!(
        rx.try_recv().unwrap(),
        Command::Load {
            position_ms: 23_000,
            ..
        }
    ));
}

#[test]
fn sleep_expiry_rejects_delayed_playing_events_and_works_while_paused() {
    for state in [State::Playing, State::Paused, State::Loading] {
        let mut app = fixture();
        app.state = state;
        let old_generation = app.generation;
        app.ui.listening.sleep = SleepTimer::Deadline(Instant::now());
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.playback_event(
            Event::Playing {
                generation: old_generation,
                position_ms: 20_000,
            },
            &tx,
        );
        assert_eq!(app.state, State::Paused);
        assert_eq!(app.queue.position_ms, 23_000);
        assert!(matches!(rx.try_recv().unwrap(), Command::Stop));
        assert!(rx.try_recv().is_err());
    }
}

#[tokio::test]
async fn end_track_sleep_overrides_repeat_and_cancels_when_track_changes() {
    for repeat in [Repeat::Off, Repeat::Queue, Repeat::Track] {
        let mut app = fixture();
        app.config.repeat = repeat;
        let current = app.queue.current().unwrap().to_owned();
        let (mut tasks, _) = tasks();
        let (tx, mut rx) = mpsc::unbounded_channel();
        apply(&mut app, &mut tasks, &tx, 4);
        app.playback_event(Event::Completed(app.generation), &tx);
        assert_eq!(app.queue.current(), Some(current.as_str()));
        assert_eq!(app.queue.position_ms, 0);
        assert_eq!(app.state, State::Paused);
        assert_eq!(app.ui.listening.sleep, SleepTimer::Off);
        assert!(matches!(rx.try_recv().unwrap(), Command::Stop));
        assert!(rx.try_recv().is_err());
        app.loaded = true;
        apply(&mut app, &mut tasks, &tx, 4);
        app.media_action(MediaAction::Next, &tx);
        app.check_sleep(Instant::now(), &tx);
        assert_eq!(app.ui.listening.sleep, SleepTimer::Off);
    }
}

#[tokio::test]
async fn end_track_sleep_requires_a_loaded_track_and_stale_completion_cannot_stop_new_song() {
    let mut app = fixture();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.loaded = false;
    apply(&mut app, &mut tasks, &tx, 4);
    assert_eq!(app.ui.listening.sleep, SleepTimer::Off);
    assert_eq!(app.ui.overlay, Overlay::ListeningTools);
    app.loaded = true;
    apply(&mut app, &mut tasks, &tx, 4);
    let old = app.generation;
    app.generation += 1;
    app.playback_event(Event::Completed(old), &tx);
    assert_eq!(app.state, State::Playing);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn queue_cleanup_removes_played_entries_without_interrupting_current_song_and_undoes() {
    let mut app = fixture();
    app.queue.order = vec![4, 0, 2, 1, 6, 3, 5];
    app.queue.cursor = Some(3);
    app.queue.selected = 5;
    app.queue.smart_shuffle = true;
    app.queue.suggestions.insert(6);
    let original = app.queue.clone();
    let current = app.queue.current().unwrap().to_owned();
    let selected = app.queue.order[app.queue.selected];
    let selected_id = app.queue.ids[selected].clone();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    apply(&mut app, &mut tasks, &tx, 6);
    assert_eq!(app.queue.current(), Some(current.as_str()));
    assert_eq!(app.queue.cursor, Some(0));
    assert_eq!(app.queue.ids.len(), 4);
    assert_eq!(
        app.queue.ids[app.queue.order[app.queue.selected]],
        selected_id
    );
    assert_eq!(app.queue.position_ms, 23_000);
    assert_eq!(app.state, State::Playing);
    assert_eq!(app.queue.suggestions.len(), 1);
    assert!(rx.try_recv().is_err());
    app.queue.validate().unwrap();
    perform_undo(&mut app, &mut tasks, &tx);
    assert_eq!(app.queue.ids, original.ids);
    assert_eq!(app.queue.order, original.order);
    assert_eq!(app.queue.cursor, original.cursor);
    assert_eq!(app.state, State::Paused);
}

#[tokio::test]
async fn queue_cleanup_deduplicates_only_upcoming_exact_ids_and_keeps_history() {
    let mut app = fixture();
    let current = app.queue.current().unwrap().to_owned();
    let history = app.queue.ids[app.queue.order[0]].clone();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    apply(&mut app, &mut tasks, &tx, 7);
    assert_eq!(app.queue.ids.len(), 5);
    assert_eq!(app.queue.current(), Some(current.as_str()));
    assert_eq!(app.queue.ids[app.queue.order[0]], history);
    assert_eq!(
        app.queue
            .ids
            .iter()
            .filter(|id| **id == test_track(1).id)
            .count(),
        1
    );
    assert_eq!(
        app.queue
            .ids
            .iter()
            .filter(|id| **id == test_track(3).id)
            .count(),
        1
    );
    assert_eq!(app.state, State::Playing);
    assert!(rx.try_recv().is_err());
    let revision = app.queue.revision;
    apply(&mut app, &mut tasks, &tx, 7);
    assert_eq!(app.queue.revision, revision);
    app.queue.validate().unwrap();
}

#[tokio::test]
async fn queue_cleanup_preserves_current_and_unknown_unavailable_metadata() {
    let mut app = fixture();
    for i in [1, 3] {
        let mut track = test_track(i);
        track.playable = false;
        app.cache.insert(track.id.clone(), track);
    }
    app.cache.remove(&test_track(4).id);
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    apply(&mut app, &mut tasks, &tx, 8);
    assert_eq!(app.queue.ids.len(), 4);
    assert_eq!(app.queue.current(), Some(test_track(1).id.as_str()));
    assert!(app.queue.ids.contains(&test_track(4).id));
    assert!(!app.queue.ids.contains(&test_track(3).id));
    app.queue.validate().unwrap();
}

#[tokio::test]
async fn play_next_promotes_occurrences_from_before_and_after_current_and_supports_filters() {
    for from in [0, 4, 6] {
        let mut app = fixture();
        app.queue.cursor = Some(3);
        app.queue.selected = from;
        let current = app.queue.order[3];
        let selected = app.queue.order[from];
        let (mut tasks, _) = tasks();
        let (tx, mut rx) = mpsc::unbounded_channel();
        if from == 6 {
            app.ui.queue.query = "Song 4".into();
        }
        key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
            &mut tasks,
            &tx,
        );
        assert_eq!(app.queue.ids.len(), 7);
        assert_eq!(app.queue.order[app.queue.cursor.unwrap()], current);
        assert_eq!(app.queue.order[app.queue.cursor.unwrap() + 1], selected);
        assert_eq!(app.queue.position_ms, 23_000);
        assert!(rx.try_recv().is_err());
        app.queue.validate().unwrap();
    }
}

#[tokio::test]
async fn play_next_from_catalog_adds_one_track_without_replacing_queue() {
    let mut app = fixture();
    app.catalog.view = View::Liked;
    app.catalog.rows = Rows::Tracks(vec![test_track(42)]);
    let current = app.queue.current().unwrap().to_owned();
    let (mut tasks, _) = tasks();
    let (tx, _) = mpsc::unbounded_channel();
    key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
        &mut tasks,
        &tx,
    );
    assert_eq!(app.queue.ids.len(), 8);
    assert_eq!(
        app.queue.peek_next(Repeat::Off),
        Some(test_track(42).id.as_str())
    );
    assert_eq!(app.queue.current(), Some(current.as_str()));
}

#[tokio::test]
async fn tools_mouse_controls_are_isolated_and_can_apply_and_cancel_in_small_terminal() {
    let mut app = fixture();
    let (mut tasks, _) = tasks();
    let (tx, mut rx) = mpsc::unbounded_channel();
    draw_mouse(&app, 80, 24);
    click_target(
        &mut app,
        MouseTarget::ListeningTools,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    for at in 0..9 {
        app.ui.listening.selected = at;
        draw_mouse(&app, 32, 10);
        click_target(
            &mut app,
            MouseTarget::ListeningRow(at),
            MouseButton::Left,
            &mut tasks,
            &tx,
        );
    }
    app.ui.listening.selected = 1;
    draw_mouse(&app, 32, 10);
    click_target(
        &mut app,
        MouseTarget::ListeningApply,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert!(matches!(app.ui.listening.sleep, SleepTimer::Deadline(_)));
    app.open_listening_tools();
    draw_mouse(&app, 32, 10);
    click_target(
        &mut app,
        MouseTarget::ListeningClose,
        MouseButton::Left,
        &mut tasks,
        &tx,
    );
    assert_eq!(app.ui.overlay, Overlay::None);
    assert!(rx.try_recv().is_err());
}

#[test]
fn sleep_label_and_refresh_deadline_use_wall_time_without_busy_loops() {
    let now = Instant::now();
    let timer = SleepTimer::Deadline(now + Duration::from_millis(61_001));
    assert_eq!(timer.label(now).as_deref(), Some("Sleep 1:02"));
    assert_eq!(timer.refresh_at(now), Some(now + Duration::from_secs(1)));
    assert_eq!(SleepTimer::Deadline(now).refresh_at(now), Some(now));
    assert_eq!(SleepTimer::Off.refresh_at(now), None);
}
