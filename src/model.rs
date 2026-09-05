use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub artists: String,
    pub duration_ms: u32,
    pub playable: bool,
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

pub fn track_id(input: &str) -> Option<String> {
    let input = input.trim();
    if let Some(id) = input.strip_prefix("spotify:track:") {
        return valid_id(id).then(|| id.to_owned());
    }
    let url = url::Url::parse(input).ok()?;
    if url.scheme() != "https" || url.host_str() != Some("open.spotify.com") {
        return None;
    }
    let segments: Vec<_> = url.path_segments()?.collect();
    let i = usize::from(segments.first()?.starts_with("intl-"));
    (segments.get(i) == Some(&"track") && segments.len() == i + 2)
        .then(|| segments[i + 1])
        .filter(|id| valid_id(id))
        .map(str::to_owned)
}
