use crate::{
    app::{App, View},
    catalog::{Browse, Page, Rows},
    model::{Playlist, Track},
    queue::Queue,
    storage::Config,
};

const PLAYLIST_ID: &str = "9000000000000000000001";

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
    const ARTISTS: [&str; 6] = [
        "Mali & The Signals",
        "Siamese Static",
        "Velvet Transit",
        "June Decoder",
        "Night Market Radio",
        "The Paper Moons",
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

pub(crate) fn all_tracks() -> Vec<Track> {
    playlist_tracks()
        .into_iter()
        .chain(recommendation_tracks())
        .collect()
}

pub(crate) fn page(browse: &Browse, offset: usize) -> Page {
    let rows = match browse {
        Browse::Playlists => Rows::Playlists(vec![Playlist {
            id: PLAYLIST_ID.into(),
            name: "Bangkok After Midnight (Demo)".into(),
            owner: "Tuitify Demo".into(),
        }]),
        Browse::Playlist(_) | Browse::Liked => Rows::Tracks(playlist_tracks()),
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
    for track in tracks.into_iter().chain(recommendation_tracks()) {
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
}
