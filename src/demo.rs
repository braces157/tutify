use crate::{
    app::{App, View},
    catalog::{Browse, Page, Rows},
    model::{Playlist, Track},
    queue::Queue,
    storage::Config,
};

pub const PLAYLIST_ID: &str = "9000000000000000000001";
pub const ALBUM_ID: &str = "8000000000000000000001";

pub const ARTISTS: [&str; 6] = [
    "Mali & The Signals",
    "Siamese Static",
    "Velvet Transit",
    "June Decoder",
    "Night Market Radio",
    "The Paper Moons",
];

const ARTIST_TRACKS: [[&str; 8]; 6] = [
    // 500: Mali & The Signals
    [
        "Midnight Tamarind",
        "Neon Monsoon",
        "Siam Square Sunset",
        "Signal Echoes",
        "Bamboo Beats",
        "Silk & Steel",
        "Floating Market",
        "Tuk Tuk Drift",
    ],
    // 501: Siamese Static
    [
        "Skytrain at Dawn",
        "Static Garden",
        "Electric Monks",
        "Rainy Sukhumvit",
        "Temple Frequencies",
        "Monsoon Pulse",
        "Cyber Lotus",
        "Night Bazaar",
    ],
    // 502: Velvet Transit
    [
        "Paper Satellites",
        "Last Ferry Home",
        "Velvet Highway",
        "Asok Station Late",
        "Low Frequency Canal",
        "Silver Express",
        "Thonglor Lights",
        "River Taxi",
    ],
    // 503: June Decoder
    [
        "Lantern Frequency",
        "Soft Circuit",
        "June Horizon",
        "Analog Memories",
        "Cassette Tape Monsoon",
        "Neon Raindrops",
        "Silicon Orchid",
        "Midnight Call",
    ],
    // 504: Night Market Radio
    [
        "Borrowed Tomorrow",
        "Concrete Fireflies",
        "Street Food Symphony",
        "Radio Yaowarat",
        "Neon Noodles",
        "Night Market Groove",
        "Silom Skyline",
        "Late Night Transmission",
    ],
    // 505: The Paper Moons
    [
        "Glass River",
        "Cloud Archive",
        "Paper Eclipse",
        "Orbit Over Bangkok",
        "Lunar Shadows",
        "Constellation 101",
        "Waxing Crescent",
        "Starlight Over Chao Phraya",
    ],
];

fn fictional_track(index: usize, suggested: bool) -> Track {
    const TITLES: [&str; 12] = [
        "Neon Monsoon",
        "Skytrain at Dawn",
        "Paper Satellites",
        "Lantern Frequency",
        "Borrowed Tomorrow",
        "Glass River",
        "Soft Circuit",
        "Midnight Tamarind",
        "Concrete Fireflies",
        "Static Garden",
        "Last Ferry Home",
        "Cloud Archive",
    ];
    let number = if suggested { 80 + index } else { index + 1 };
    let artist = index % ARTISTS.len();
    Track {
        id: format!("{number:022}"),
        name: TITLES[index % TITLES.len()].into(),
        artists: ARTISTS[artist].into(),
        artist_ids: vec![format!("{:022}", 500 + artist)],
        duration_ms: 155_000 + ((index * 23_000) % 105_000) as u32,
        playable: index != 17,
        album: Some("Terminal Sessions: Fictional Catalog".into()),
        album_art_url: None,
        album_id: Some(ALBUM_ID.into()),
        track_number: None,
    }
}

pub(crate) fn playlist_tracks() -> Vec<Track> {
    (0..24).map(|index| fictional_track(index, false)).collect()
}

pub(crate) fn recommendation_tracks() -> Vec<Track> {
    (0..16)
        .map(|index| fictional_track(index + 4, true))
        .collect()
}

pub(crate) fn album_tracks(album_id: &str) -> Vec<Track> {
    const ALBUM_TITLES: [&str; 10] = [
        "Neon Prelude",
        "Skytrain at Dawn",
        "Paper Satellites",
        "Lantern Frequency",
        "Chao Phraya Reflections",
        "Glass River",
        "Midnight Tamarind",
        "Concrete Fireflies",
        "Static Garden",
        "Last Ferry Home",
    ];
    (1..=10)
        .map(|i| Track {
            id: format!("80000000000000000001{:02}", i),
            name: ALBUM_TITLES[i - 1].into(),
            artists: "Mali & The Signals".into(),
            artist_ids: vec![format!("{:022}", 500)],
            duration_ms: 180_000 + ((i as u32 * 17_000) % 65_000),
            playable: i != 5,
            album: Some("Terminal Sessions: Fictional Catalog".into()),
            album_art_url: None,
            album_id: Some(album_id.into()),
            track_number: Some(i as u32),
        })
        .collect()
}

pub(crate) fn known_artist_tracks(artist_idx: usize) -> Vec<Track> {
    let artist_name = ARTISTS[artist_idx];
    let artist_id = format!("{:022}", 500 + artist_idx);
    let titles = ARTIST_TRACKS[artist_idx];
    (1..=8)
        .map(|i| Track {
            id: format!("500000000000000000{:02}{:02}", artist_idx, i),
            name: titles[i - 1].into(),
            artists: artist_name.into(),
            artist_ids: vec![artist_id.clone()],
            duration_ms: 160_000 + ((artist_idx as u32 * 19_000 + i as u32 * 27_000) % 85_000),
            playable: i != 7,
            album: Some(format!("{artist_name} Anthology")),
            album_art_url: None,
            album_id: Some(ALBUM_ID.into()),
            track_number: None,
        })
        .collect()
}

pub(crate) fn fallback_artist_tracks(artist_id: &str) -> Vec<Track> {
    const FALLBACK_TITLES: [&str; 8] = [
        "Echoes in the Rain",
        "Neon Reflections",
        "Midnight Drift",
        "Signal in the Dark",
        "Lost Frequencies",
        "Starlight Boulevard",
        "Circuit Dreams",
        "Urban Mirage",
    ];
    let artist_name = if crate::model::valid_id(artist_id) {
        "Fictional Artist"
    } else {
        "Demo Artist"
    };
    (1..=8)
        .map(|i| Track {
            id: format!("70000000000000000000{:02}", i),
            name: FALLBACK_TITLES[i - 1].into(),
            artists: artist_name.into(),
            artist_ids: vec![artist_id.to_string()],
            duration_ms: 170_000 + ((i as u32 * 21_000) % 70_000),
            playable: i != 8,
            album: Some("Fictional Sessions".into()),
            album_art_url: None,
            album_id: Some(ALBUM_ID.into()),
            track_number: None,
        })
        .collect()
}

pub(crate) fn artist_tracks(artist_id: &str) -> Vec<Track> {
    for (i, _) in ARTISTS.iter().enumerate() {
        let expected_id = format!("{:022}", 500 + i);
        if expected_id == artist_id || format!("{}", 500 + i) == artist_id {
            return known_artist_tracks(i);
        }
    }
    fallback_artist_tracks(artist_id)
}

pub(crate) fn all_tracks() -> Vec<Track> {
    let mut tracks = playlist_tracks();
    tracks.extend(recommendation_tracks());
    tracks.extend(album_tracks(ALBUM_ID));
    for i in 0..ARTISTS.len() {
        tracks.extend(known_artist_tracks(i));
    }
    tracks.extend(fallback_artist_tracks("7000000000000000000000"));
    tracks
}

pub(crate) fn page(browse: &Browse, offset: usize) -> Page {
    let rows = match browse {
        Browse::Playlists => Rows::Playlists(vec![Playlist {
            id: PLAYLIST_ID.into(),
            name: "Bangkok After Midnight (Demo)".into(),
            owner: "Tuitify Demo".into(),
        }]),
        Browse::Playlist(_) | Browse::Liked => Rows::Tracks(playlist_tracks()),
        Browse::Album(id) => Rows::Tracks(album_tracks(id)),
        Browse::Artist(id) => Rows::Tracks(artist_tracks(id)),
        Browse::Search(query) => {
            let query = query.to_lowercase();
            Rows::Tracks(
                playlist_tracks()
                    .into_iter()
                    .filter(|track| {
                        query.is_empty()
                            || track.name.to_lowercase().contains(&query)
                            || track.artists.to_lowercase().contains(&query)
                    })
                    .collect(),
            )
        }
    };
    Page {
        rows,
        offset,
        next: None,
    }
}

pub(crate) fn app() -> App {
    let tracks = playlist_tracks();
    let mut queue = Queue::default();
    queue.replace(
        tracks
            .iter()
            .take(9)
            .map(|track| track.id.clone())
            .collect(),
        0,
        false,
    );
    let mut app = App::new(
        Config {
            discord_rpc: false,
            ..Config::default()
        },
        queue,
    );
    app.demo = true;
    app.catalog.view = View::Queue;
    app.catalog.nav = View::Queue.index();
    for track in all_tracks() {
        app.cache.insert(track.id.clone(), track);
    }
    app.status = "DEMO • simulated playback • M opens Mix Builder • ? shows help".into();
    app
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_app_uses_only_fictional_session_state() {
        let app = app();
        assert!(app.demo);
        assert!(!app.config.discord_rpc);
        assert!(app.queue.ids.iter().all(|id| id.starts_with('0')));
        assert!(app.queue.ids.iter().all(|id| app.cache.get(id).is_some()));
        assert!(app.mix_recipes.recipes.is_empty());
        assert!(app.stats.is_empty());
    }

    #[test]
    fn demo_album_tracks_ordered_and_metadata_complete() {
        let tracks = album_tracks(ALBUM_ID);
        assert_eq!(tracks.len(), 10);
        for (i, track) in tracks.iter().enumerate() {
            assert_eq!(track.track_number, Some((i + 1) as u32));
            assert_eq!(track.album_id.as_deref(), Some(ALBUM_ID));
            assert!(crate::model::valid_id(&track.id));
            assert!(track.duration_ms >= 180_000);
        }
        assert!(tracks.iter().any(|t| !t.playable));
        assert!(tracks.iter().any(|t| t.playable));
    }

    #[test]
    fn demo_artist_top_tracks_for_known_and_fallback_artists() {
        for (i, artist) in ARTISTS.iter().enumerate() {
            let artist_id = format!("{:022}", 500 + i);
            let tracks = artist_tracks(&artist_id);
            assert_eq!(tracks.len(), 8);
            assert!(tracks.iter().all(|t| crate::model::valid_id(&t.id)));
            assert!(
                tracks
                    .iter()
                    .all(|t| t.artist_ids == vec![artist_id.clone()])
            );
            assert!(tracks.iter().all(|t| t.artists == *artist));
        }

        let fallback_id = "1111111111111111111111";
        let fallback = artist_tracks(fallback_id);
        assert_eq!(fallback.len(), 8);
        assert!(fallback.iter().all(|t| crate::model::valid_id(&t.id)));
        assert!(
            fallback
                .iter()
                .all(|t| t.artist_ids == vec![fallback_id.to_string()])
        );
    }

    #[test]
    fn demo_all_tracks_are_cached_and_unique() {
        let all = all_tracks();
        let mut ids = std::collections::HashSet::new();
        for track in &all {
            assert!(crate::model::valid_id(&track.id));
            assert!(
                ids.insert(track.id.clone()),
                "Duplicate track id: {}",
                track.id
            );
        }
        let app = app();
        for track in all {
            assert!(app.cache.get(&track.id).is_some());
        }
    }

    #[test]
    fn demo_page_handles_album_and_artist() {
        let album_page = page(&Browse::Album(ALBUM_ID.into()), 0);
        assert!(album_page.next.is_none());
        if let Rows::Tracks(tracks) = album_page.rows {
            assert_eq!(tracks.len(), 10);
        } else {
            panic!("Expected Rows::Tracks for Browse::Album");
        }

        let artist_page = page(&Browse::Artist(format!("{:022}", 500)), 0);
        assert!(artist_page.next.is_none());
        if let Rows::Tracks(tracks) = artist_page.rows {
            assert_eq!(tracks.len(), 8);
        } else {
            panic!("Expected Rows::Tracks for Browse::Artist");
        }
    }
}
