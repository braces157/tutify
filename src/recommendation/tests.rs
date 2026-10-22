use super::*;
use crate::stats::{MAX_FEEDBACK_EVENTS, TrackStat};

fn song(id: usize, name: &str, artist: &str) -> Track {
    Track {
        id: format!("deezer:{id}"),
        name: name.into(),
        artists: artist.into(),
        artist_ids: vec![format!(
            "deezer:{}",
            100 + artist.bytes().map(usize::from).sum::<usize>()
        )],
        duration_ms: 200_000,
        playable: true,
        ..Default::default()
    }
}

#[test]
fn cold_start_preserves_provider_order_and_all_candidates() {
    let tracks = vec![
        song(1, "First", "A"),
        song(2, "Second", "B"),
        song(3, "Third", "A"),
    ];
    assert_eq!(
        Profile::new(&SongStats::default(), DAY).rank(tracks.clone(), &[]),
        tracks
    );
}

#[test]
fn taste_promotes_related_favourites_but_recent_replays_leave_room_for_discovery() {
    let at = 100 * DAY;
    let favourite = song(1, "Favourite", "A");
    let discovery = song(2, "New song", "A");
    let unfamiliar = song(3, "Other artist", "B");
    let mut stats = SongStats::default();
    stats.tracks.insert(
        favourite.id.clone(),
        TrackStat {
            id: favourite.id.clone(),
            name: favourite.name.clone(),
            artists: favourite.artists.clone(),
            play_count: 12,
            listened_ms: 2_000_000,
        },
    );
    let profile = Profile::new(&stats, at);
    assert!(profile.score(&discovery) > profile.score(&unfamiliar));
    stats.record_feedback(&favourite, FeedbackKind::Enjoyed, at);
    let ranked = Profile::new(&stats, at).rank(vec![favourite.clone(), discovery.clone()], &[]);
    assert_eq!(ranked[0], discovery);
    assert_eq!(ranked.len(), 2); // Familiar tracks remain available to sparse stations.
}

#[test]
fn repeated_skips_cool_down_a_recording_not_its_artist_and_recover() {
    let at = 100 * DAY;
    let skipped = song(1, "Skip me", "A");
    let edition = song(2, "Skip me - Remastered", "A");
    let other = song(3, "Other song", "A");
    let mut stats = SongStats::default();
    stats.record_feedback(&skipped, FeedbackKind::EarlySkip, at - DAY);
    assert!(Profile::new(&stats, at).accepts(&skipped));
    stats.record_feedback(&edition, FeedbackKind::EarlySkip, at);
    let profile = Profile::new(&stats, at);
    assert!(!profile.accepts(&skipped));
    assert!(profile.accepts(&other));
    assert!(Profile::new(&stats, at + 8 * DAY).accepts(&skipped));
    stats.record_feedback(&skipped, FeedbackKind::Enjoyed, at);
    assert!(Profile::new(&stats, at).accepts(&skipped));
    stats.record_feedback(&skipped, FeedbackKind::EarlySkip, at + 1);
    assert!(Profile::new(&stats, at + 1).accepts(&skipped));
}

#[test]
fn negative_feedback_does_not_cross_same_name_artist_identities() {
    let at = 100 * DAY;
    let skipped = song(1, "Song", "Common name");
    let mut other = skipped.clone();
    other.id = "deezer:2".into();
    other.artist_ids = vec!["deezer:999".into()];
    let mut stats = SongStats::default();
    for _ in 0..2 {
        stats.record_feedback(&skipped, FeedbackKind::EarlySkip, at);
    }
    let profile = Profile::new(&stats, at);
    assert!(!profile.accepts(&skipped));
    assert!(profile.accepts(&other));
    assert!(profile.score(&other) > profile.score(&skipped));
}

#[test]
fn preference_ranking_keeps_artist_spacing_including_queue_boundary() {
    let mut stats = SongStats::default();
    let a = song(1, "A1", "A");
    stats.record_feedback(&a, FeedbackKind::Enjoyed, DAY);
    let mut tracks = Vec::new();
    for i in 0..9 {
        tracks.push(song(i + 10, &format!("Song {i}"), ["A", "B", "C"][i / 3]));
    }
    let ranked = Profile::new(&stats, DAY).rank(tracks, std::slice::from_ref(&a));
    assert_eq!(ranked.len(), 9);
    assert_ne!(ranked[0].artists, a.artists);
    assert!(
        ranked
            .windows(2)
            .all(|pair| pair[0].artists != pair[1].artists)
    );
}

#[test]
fn feedback_is_bounded_validated_and_survives_legacy_stats_migration() {
    let mut stats: SongStats = serde_json::from_str(r#"{"version":1,"tracks":{}}"#).unwrap();
    let track = song(1, "Song", "Artist");
    for at in 0..300 {
        stats.record_feedback(&track, FeedbackKind::Enjoyed, at);
    }
    assert_eq!(stats.feedback.len(), MAX_FEEDBACK_EVENTS);
    assert_eq!(stats.feedback[0].at, 300 - MAX_FEEDBACK_EVENTS as u64);
    let restored: SongStats = serde_json::from_slice(&serde_json::to_vec(&stats).unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.feedback.last().unwrap().at, 299);
    stats.feedback[0].id = "invalid".into();
    assert!(stats.validate().is_err());
}

#[test]
fn stale_skips_decay_instead_of_becoming_permanent_dislikes() {
    let track = song(1, "Song", "Artist");
    let mut stats = SongStats::default();
    stats.record_feedback(&track, FeedbackKind::EarlySkip, DAY);
    let immediate = Profile::new(&stats, DAY).score(&track);
    let later = Profile::new(&stats, 60 * DAY).score(&track);
    assert!(immediate < later);
    assert_eq!(Profile::new(&stats, 100 * DAY).score(&track), 0);
}
