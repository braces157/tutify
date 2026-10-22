use super::*;
use crate::stats::FeedbackKind;

fn streaming_app() -> App {
    let mut app = App::new(Config::default(), Queue::default());
    let mut track = test_track(1);
    track.artist_ids = vec!["artist-one".into()];
    app.queue
        .replace(vec![track.id.clone(), test_track(2).id], 0, false);
    app.cache.insert(track.id.clone(), track.clone());
    app.loaded = true;
    app.generation = 1;
    app.state = State::Playing;
    app.accounting
        .start_generation(1, Some(track.id.clone()), track.duration_ms);
    app.accounting
        .on_playing(1, Some(track.id), track.duration_ms, Instant::now());
    app
}

fn listen(app: &mut App, seconds: u64) {
    let start = app.accounting.last_accounted_at.unwrap();
    for seconds in 1..=seconds {
        app.account_playback_time(start + Duration::from_secs(seconds));
    }
}

#[test]
fn intentional_early_next_records_once_for_the_old_generation() {
    let mut app = streaming_app();
    listen(&mut app, 10);
    let (tx, _rx) = mpsc::unbounded_channel();
    app.media_action(MediaAction::Next, &tx);
    assert_eq!(app.stats.feedback.len(), 1);
    assert_eq!(app.stats.feedback[0].id, test_track(1).id);
    assert_eq!(app.stats.feedback[0].kind, FeedbackKind::EarlySkip);
    app.playback_event(Event::Completed(1), &tx);
    assert_eq!(app.stats.feedback.len(), 1);
    assert_eq!(app.accounting.track_id, Some(test_track(2).id));
}

#[test]
fn sustained_actual_listening_is_credited_once_and_not_changed_to_a_skip() {
    let mut app = streaming_app();
    listen(&mut app, 100);
    assert_eq!(app.stats.feedback.len(), 1);
    assert_eq!(app.stats.feedback[0].kind, FeedbackKind::Enjoyed);
    let (tx, _rx) = mpsc::unbounded_channel();
    app.media_action(MediaAction::Next, &tx);
    assert_eq!(app.stats.feedback.len(), 1);
}

#[test]
fn pause_seek_failure_and_brief_accidental_next_are_neutral() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = streaming_app();
    listen(&mut app, 10);
    app.media_action(MediaAction::Pause, &tx);
    assert!(app.stats.feedback.is_empty());
    app.control(controls::Control::Seek(controls::Seek::End), &tx);
    assert!(app.stats.feedback.is_empty());
    app.playback_event(Event::Completed(app.generation), &tx);
    assert!(app.stats.feedback.is_empty());

    let mut app = streaming_app();
    listen(&mut app, 10);
    app.playback_event(
        Event::TrackError {
            generation: 1,
            message: "transport failed".into(),
        },
        &tx,
    );
    assert!(app.stats.feedback.is_empty());

    let mut app = streaming_app();
    listen(&mut app, 1);
    app.media_action(MediaAction::Next, &tx);
    assert!(app.stats.feedback.is_empty());
}

#[tokio::test]
async fn latest_feedback_filters_radio_autoplay_smart_and_mix_without_removing_originals() {
    let mut skipped = test_track(20);
    skipped.artists = "Skipped artist".into();
    let mut fresh = test_track(21);
    fresh.artists = "Fresh artist".into();
    let batch = || Recommendations {
        tracks: vec![skipped.clone(), fresh.clone()],
        source: crate::catalog::RecommendationSource::Deezer,
    };
    let mut app = streaming_app();
    let at = crate::recommendation::now();
    for _ in 0..2 {
        app.stats
            .record_feedback(&skipped, FeedbackKind::EarlySkip, at);
    }
    let (mut tasks, _) = tasks();
    app.radio_epoch = Some(app.queue.epoch);
    let epoch = app.queue.epoch;
    background(
        &mut app,
        &mut tasks,
        Background::Recommendations(epoch, Ok(batch())),
    );
    assert!(app.queue.ids.contains(&fresh.id));
    assert!(!app.queue.ids.contains(&skipped.id));
    assert_eq!(app.queue.ids[0], test_track(1).id);

    app.queue.replace(vec![test_track(1).id], 0, false);
    tasks.finish_autoplay(&mut app, Ok(batch()));
    assert!(app.queue.ids.contains(&fresh.id));
    assert!(!app.queue.ids.contains(&skipped.id));

    app.queue
        .replace((1..=8).map(|i| test_track(i).id).collect(), 0, false);
    app.queue.set_shuffle(true);
    app.queue.smart_shuffle = true;
    tasks.finish_smart_shuffle(&mut app, Ok(batch()));
    assert!(app.queue.ids.contains(&fresh.id));
    assert!(!app.queue.ids.contains(&skipped.id));
    assert!((1..=8).all(|i| app.queue.ids.contains(&test_track(i).id)));

    let candidates = batch()
        .tracks
        .into_iter()
        .map(|track| MixCandidate {
            track,
            provenance: Provenance::Recommendation {
                seed_id: test_track(1).id,
                seed_name: "Seed".into(),
                provider: crate::catalog::RecommendationSource::Deezer,
            },
        })
        .collect();
    let request = app.mix.recommendation_request;
    background(
        &mut app,
        &mut tasks,
        Background::MixRecommendations(request, candidates, None),
    );
    assert_eq!(app.mix.recommendation_candidates.len(), 1);
    assert_eq!(app.mix.recommendation_candidates[0].track.id, fresh.id);
}
