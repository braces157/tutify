use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LyricLine {
    pub position_ms: u32,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    pub plain: Option<String>,
}

impl Lyrics {
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.plain.is_none()
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

pub async fn fetch(
    client: &Client,
    track_name: &str,
    artist_name: &str,
    duration_ms: u32,
) -> Result<Option<Lyrics>> {
    let duration_s = (duration_ms / 1000).to_string();
    let query = [
        ("track_name", track_name),
        ("artist_name", artist_name),
        ("duration", &duration_s),
    ];

    let resp = client
        .get("https://lrclib.net/api/get")
        .query(&query)
        .header(
            "User-Agent",
            concat!(
                "Tuitify/",
                env!("CARGO_PKG_VERSION"),
                " (terminal spotify player)"
            ),
        )
        .send()
        .await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let resp = resp.error_for_status()?;

    let json: Value = resp.json().await?;
    let synced = json["syncedLyrics"].as_str();
    let plain = json["plainLyrics"].as_str().map(|s| {
        s.chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .collect::<String>()
    });

    let lines = if let Some(synced_text) = synced {
        parse_lrc(synced_text)
    } else {
        vec![]
    };

    Ok(Some(Lyrics { lines, plain }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        };
        assert_eq!(lyrics.current_line_index(500), None);
        assert_eq!(lyrics.current_line_index(2000), Some(0));
        assert_eq!(lyrics.current_line_index(6000), Some(1));
        assert_eq!(lyrics.current_line_index(15000), Some(2));
    }
}
