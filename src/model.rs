use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub artists: String,
    /// Stable Spotify artist IDs. Older cache entries omit this field.
    #[serde(default)]
    pub artist_ids: Vec<String>,
    pub duration_ms: u32,
    pub playable: bool,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub album_art_url: Option<String>,
    #[serde(default)]
    pub album_id: Option<String>,
    #[serde(default)]
    pub track_number: Option<u32>,
}

impl Track {
    pub fn unknown(id: &str) -> Self {
        Self {
            id: id.into(),
            name: format!("Track {id}"),
            playable: true,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub owner: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    #[default]
    Off,
    Queue,
    Track,
}

impl Repeat {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::Queue,
            Self::Queue => Self::Track,
            Self::Track => Self::Off,
        }
    }
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

pub fn spotify_id(input: &str, entity_type: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    // 1. URI format: spotify:<entity_type>:<id>
    if let Some((scheme_and_type, rest)) = input
        .split_once(':')
        .and_then(|(s, r)| r.split_once(':').map(|(t, id)| ((s, t), id)))
    {
        if scheme_and_type.0.eq_ignore_ascii_case("spotify")
            && scheme_and_type.1.eq_ignore_ascii_case(entity_type)
        {
            let id = rest
                .split(['?', '#'])
                .next()
                .unwrap_or("")
                .trim_end_matches('/');
            return valid_id(id).then(|| id.to_owned());
        }
    }

    // 2. Web URL format: https://open.spotify.com/[intl-<locale>/]<entity_type>/<id>[/]
    // Also handles URLs without scheme (e.g. open.spotify.com/...)
    let candidate = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };

    let url = url::Url::parse(&candidate).ok()?;
    if (url.scheme() != "https" && url.scheme() != "http")
        || url.host_str() != Some("open.spotify.com")
    {
        return None;
    }

    let segments: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    let first = segments.first()?;
    let i = usize::from(first.starts_with("intl-"));
    if segments
        .get(i)
        .is_some_and(|s| s.eq_ignore_ascii_case(entity_type))
        && segments.len() == i + 2
    {
        let id = segments[i + 1];
        if valid_id(id) {
            return Some(id.to_owned());
        }
    }

    None
}

pub fn track_id(input: &str) -> Option<String> {
    spotify_id(input, "track")
}

#[allow(dead_code)]
pub fn album_id(input: &str) -> Option<String> {
    spotify_id(input, "album")
}

#[allow(dead_code)]
pub fn artist_id(input: &str) -> Option<String> {
    spotify_id(input, "artist")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Paused,
    Loading,
    Playing,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACK_ID: &str = "4cOdK2wGLETKBW3PvgPWqT";
    const ALBUM_ID: &str = "6kZDoAmRSvGQvun5LzyZhk";
    const ARTIST_ID: &str = "4Z8W4fKeB5YxbusRsdQVPb";

    #[test]
    fn test_valid_id() {
        assert!(valid_id(TRACK_ID));
        assert!(valid_id(ALBUM_ID));
        assert!(valid_id(ARTIST_ID));
        assert!(!valid_id("short"));
        assert!(!valid_id("toolong0000000000000000000001"));
        assert!(!valid_id("4cOdK2wGLETKBW3PvgPWq-"));
        assert!(!valid_id("4cOdK2wGLETKBW3PvgPWq!"));
        assert!(!valid_id("                      "));
    }

    #[test]
    fn test_spotify_id_standard_uris() {
        assert_eq!(
            spotify_id(&format!("spotify:track:{TRACK_ID}"), "track"),
            Some(TRACK_ID.into())
        );
        assert_eq!(
            track_id(&format!("spotify:track:{TRACK_ID}")),
            Some(TRACK_ID.into())
        );
        assert_eq!(
            album_id(&format!("spotify:album:{ALBUM_ID}")),
            Some(ALBUM_ID.into())
        );
        assert_eq!(
            artist_id(&format!("spotify:artist:{ARTIST_ID}")),
            Some(ARTIST_ID.into())
        );

        // Case insensitivity in URI scheme and entity type
        assert_eq!(
            track_id(&format!("SPOTIFY:TRACK:{TRACK_ID}")),
            Some(TRACK_ID.into())
        );
        assert_eq!(
            album_id(&format!("Spotify:Album:{ALBUM_ID}")),
            Some(ALBUM_ID.into())
        );
    }

    #[test]
    fn test_spotify_id_cross_entity_rejection() {
        assert!(track_id(&format!("spotify:album:{ALBUM_ID}")).is_none());
        assert!(track_id(&format!("spotify:artist:{ARTIST_ID}")).is_none());
        assert!(album_id(&format!("spotify:track:{TRACK_ID}")).is_none());
        assert!(album_id(&format!("spotify:artist:{ARTIST_ID}")).is_none());
        assert!(artist_id(&format!("spotify:track:{TRACK_ID}")).is_none());
        assert!(artist_id(&format!("spotify:album:{ALBUM_ID}")).is_none());
    }

    #[test]
    fn test_spotify_id_standard_urls() {
        assert_eq!(
            track_id(&format!("https://open.spotify.com/track/{TRACK_ID}")),
            Some(TRACK_ID.into())
        );
        assert_eq!(
            album_id(&format!("https://open.spotify.com/album/{ALBUM_ID}")),
            Some(ALBUM_ID.into())
        );
        assert_eq!(
            artist_id(&format!("https://open.spotify.com/artist/{ARTIST_ID}")),
            Some(ARTIST_ID.into())
        );
    }

    #[test]
    fn test_spotify_id_urls_without_scheme() {
        assert_eq!(
            track_id(&format!("open.spotify.com/track/{TRACK_ID}")),
            Some(TRACK_ID.into())
        );
        assert_eq!(
            album_id(&format!("open.spotify.com/album/{ALBUM_ID}")),
            Some(ALBUM_ID.into())
        );
        assert_eq!(
            artist_id(&format!("open.spotify.com/artist/{ARTIST_ID}")),
            Some(ARTIST_ID.into())
        );
    }

    #[test]
    fn test_spotify_id_international_urls() {
        assert_eq!(
            album_id(&format!(
                "https://open.spotify.com/intl-es/album/{ALBUM_ID}"
            )),
            Some(ALBUM_ID.into())
        );
        assert_eq!(
            artist_id(&format!("open.spotify.com/intl-de/artist/{ARTIST_ID}")),
            Some(ARTIST_ID.into())
        );
        assert_eq!(
            track_id(&format!(
                "https://open.spotify.com/intl-ja/track/{TRACK_ID}"
            )),
            Some(TRACK_ID.into())
        );
        assert_eq!(
            album_id(&format!(
                "https://open.spotify.com/intl-en-us/album/{ALBUM_ID}"
            )),
            Some(ALBUM_ID.into())
        );
    }

    #[test]
    fn test_spotify_id_query_parameters() {
        assert_eq!(
            track_id(&format!(
                "https://open.spotify.com/track/{TRACK_ID}?si=abc123xyz"
            )),
            Some(TRACK_ID.into())
        );
        assert_eq!(
            album_id(&format!(
                "open.spotify.com/album/{ALBUM_ID}?si=123&context=spotify%3Aalbum"
            )),
            Some(ALBUM_ID.into())
        );
        assert_eq!(
            artist_id(&format!("spotify:artist:{ARTIST_ID}?si=test")),
            Some(ARTIST_ID.into())
        );
    }

    #[test]
    fn test_spotify_id_trailing_slashes() {
        assert_eq!(
            album_id(&format!("https://open.spotify.com/album/{ALBUM_ID}/")),
            Some(ALBUM_ID.into())
        );
        assert_eq!(
            artist_id(&format!("open.spotify.com/artist/{ARTIST_ID}/")),
            Some(ARTIST_ID.into())
        );
        assert_eq!(
            track_id(&format!("spotify:track:{TRACK_ID}/")),
            Some(TRACK_ID.into())
        );
        assert_eq!(
            track_id(&format!(
                "https://open.spotify.com/intl-fr/track/{TRACK_ID}/?si=abc"
            )),
            Some(TRACK_ID.into())
        );
        assert_eq!(
            album_id(&format!("open.spotify.com/album/{ALBUM_ID}///")),
            Some(ALBUM_ID.into())
        );
    }

    #[test]
    fn test_spotify_id_invalid_inputs() {
        // Invalid ID length or characters
        assert!(track_id("https://open.spotify.com/track/bad").is_none());
        assert!(album_id("spotify:album:invalid_chars_in_id!").is_none());
        assert!(artist_id("open.spotify.com/artist/short").is_none());
        // Wrong host
        assert!(track_id(&format!("https://evil.com/track/{TRACK_ID}")).is_none());
        // Missing ID
        assert!(track_id("https://open.spotify.com/track/").is_none());
        assert!(album_id("spotify:album:").is_none());
        // Subpath
        assert!(album_id(&format!("https://open.spotify.com/album/{ALBUM_ID}/tracks")).is_none());
        // Empty and blank
        assert!(track_id("").is_none());
        assert!(album_id("   ").is_none());
        assert!(artist_id("random text").is_none());
        assert!(track_id("https://").is_none());
    }

    #[test]
    fn test_track_serialization_and_defaults() {
        let json = serde_json::json!({
            "id": TRACK_ID,
            "name": "Test Track",
            "artists": "Test Artist",
            "duration_ms": 180000,
            "playable": true,
            "album_id": ALBUM_ID,
            "track_number": 4
        });
        let track: Track = serde_json::from_value(json).unwrap();
        assert_eq!(track.album_id.as_deref(), Some(ALBUM_ID));
        assert_eq!(track.track_number, Some(4));

        // Missing album_id and track_number default to None
        let legacy_json = serde_json::json!({
            "id": TRACK_ID,
            "name": "Legacy Track",
            "artists": "Test Artist",
            "duration_ms": 180000,
            "playable": true
        });
        let legacy_track: Track = serde_json::from_value(legacy_json).unwrap();
        assert!(legacy_track.album_id.is_none());
        assert!(legacy_track.track_number.is_none());
    }
}
