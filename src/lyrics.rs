use crate::model::Track;
use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

mod romanization;
mod sources;
pub use romanization::{ReadingMode, Readings};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LyricLine {
    pub position_ms: u32,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    pub plain: Option<String>,
    pub readings: Vec<Readings>,
    pub plain_readings: Vec<Readings>,
    pub japanese: bool,
}

impl Lyrics {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
            && self
                .plain
                .as_ref()
                .is_none_or(|plain| plain.trim().is_empty())
    }

    pub fn current_line_index(&self, position_ms: u32) -> Option<usize> {
        self.lines
            .partition_point(|line| line.position_ms <= position_ms)
            .checked_sub(1)
    }
}

fn timestamp(value: &str) -> Option<u32> {
    let (minutes, seconds) = value.split_once(':')?;
    let minutes = minutes.parse::<u32>().ok()?;
    let (seconds, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let seconds = seconds.parse::<u32>().ok()?;
    if seconds >= 60 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let millis = format!("{fraction:0<3}").get(..3)?.parse::<u32>().ok()?;
    minutes
        .checked_mul(60_000)?
        .checked_add(seconds * 1000)?
        .checked_add(millis)
}

pub fn parse_lrc(lrc: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for line in lrc.lines() {
        let mut rest = line.trim();
        let mut times = Vec::new();
        while let Some(content) = rest.strip_prefix('[') {
            let Some((time, tail)) = content.split_once(']') else {
                break;
            };
            if let Some(position) = timestamp(time) {
                times.push(position);
            }
            rest = tail;
        }
        let text = rest
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>();
        for position_ms in times {
            lines.push(LyricLine {
                position_ms,
                text: text.clone(),
            });
        }
    }
    lines.sort_by_key(|l| l.position_ms);
    lines
}

pub async fn fetch(client: &Client, track: &Track) -> Result<Option<Lyrics>> {
    let lyrics = sources::fetch(client, track).await?;
    // Dictionary initialization and conversion run outside the async/UI threads.
    tokio::task::spawn_blocking(move || {
        lyrics.map(|mut lyrics| {
            lyrics.prepare_readings();
            lyrics
        })
    })
    .await
    .map_err(Into::into)
}

fn parse_lyrics(json: &Value) -> Lyrics {
    let synced = json["syncedLyrics"].as_str();
    let plain = json["plainLyrics"].as_str().and_then(|s| {
        let text = s.replace("\r\n", "\n").replace('\r', "\n");
        let text = text
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .collect::<String>();
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    });

    let mut lines = if let Some(synced_text) = synced {
        parse_lrc(synced_text)
    } else {
        vec![]
    };
    if !lines.iter().any(|line| !line.text.is_empty()) {
        lines.clear();
    }

    Lyrics {
        lines,
        plain,
        ..Lyrics::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn duet() -> Track {
        Track {
            name: "Mưa".into(),
            artists: "Minh Vương M4U, Thùy Chi".into(),
            artist_ids: vec!["lead".into(), "guest".into()],
            duration_ms: 277_000,
            ..Track::default()
        }
    }

    #[tokio::test]
    #[ignore = "Live lyrics services only; no audio, lyrics text output, or saved-state writes"]
    async fn live_vietnamese_and_english_lyrics_acceptance() -> Result<()> {
        let client = crate::auth::http_client()?;
        let mut castle = duet();
        castle.name = "Castle on the Hill".into();
        castle.artists = "Ed Sheeran".into();
        castle.artist_ids.truncate(1);
        castle.duration_ms = 261_000;
        for track in [duet(), castle] {
            let lyrics = fetch(&client, &track).await?.expect("Missing live lyrics");
            assert!(!lyrics.lines.is_empty());
            println!(
                "LIVE {} / {}: {} synced lines",
                track.name,
                track.artists,
                lyrics.lines.len()
            );
        }
        Ok(())
    }

    #[test]
    fn multiple_timestamps_and_invalid_times_are_handled_safely() {
        let lines = parse_lrc(
            "[00:01.5][00:05.050] Chorus\n[9999999999:00] overflow\n[00:60] bad seconds\n[00:NaN] bad float\n[00:-1] negative\n[00:03.日] unicode fraction",
        );
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].position_ms, 1500);
        assert_eq!(lines[1].position_ms, 5050);
        assert_eq!(lines[1].text, "Chorus");
    }
    #[test]
    fn parse_lrc_lines() {
        let sample = "[00:01.50] Line one\n[00:04.25] Line two\n[01:10.00] Line three";
        let lines = parse_lrc(sample);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].position_ms, 1500);
        assert_eq!(lines[0].text, "Line one");
        assert_eq!(lines[1].position_ms, 4250);
        assert_eq!(lines[2].position_ms, 70000);
    }

    #[test]
    fn current_line_lookup() {
        let sample = "[00:01.00] A\n[00:05.00] B\n[00:10.00] C";
        let lyrics = Lyrics {
            lines: parse_lrc(sample),
            plain: None,
            ..Lyrics::default()
        };
        assert_eq!(lyrics.current_line_index(500), None);
        assert_eq!(lyrics.current_line_index(2000), Some(0));
        assert_eq!(lyrics.current_line_index(6000), Some(1));
        assert_eq!(lyrics.current_line_index(15000), Some(2));
    }
}
