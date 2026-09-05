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
        if self.lines.is_empty() {
            return None;
        }
        let mut best = 0;
        for (i, line) in self.lines.iter().enumerate() {
            if line.position_ms <= position_ms {
                best = i;
            } else {
                break;
            }
        }
        Some(best)
    }
}

pub fn parse_lrc(lrc: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for line in lrc.lines() {
        let line = line.trim();
        if !line.starts_with('[') {
            continue;
        }
        if let Some(close) = line.find(']') {
            let time_part = &line[1..close];
            let text_part = line[close + 1..].trim();
            let parts: Vec<&str> = time_part.split(':').collect();
            if parts.len() == 2 {
                if let (Ok(min), Ok(sec)) = (parts[0].parse::<u32>(), parts[1].parse::<f32>()) {
                    let position_ms = min * 60_000 + (sec * 1000.0) as u32;
                    lines.push(LyricLine {
                        position_ms,
                        text: text_part.to_string(),
                    });
                }
            }
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
) -> Result<Lyrics> {
    let duration_s = (duration_ms / 1000).to_string();
    let query = [
        ("track_name", track_name),
        ("artist_name", artist_name),
        ("duration", &duration_s),
    ];

    let resp = client
        .get("https://lrclib.net/api/get")
        .query(&query)
        .header("User-Agent", "Tuitify/0.1.1 (terminal spotify player)")
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("No lyrics found");
    }

    let json: Value = resp.json().await?;
    let synced = json["syncedLyrics"].as_str();
    let plain = json["plainLyrics"].as_str().map(|s| s.to_string());

    let lines = if let Some(synced_text) = synced {
        parse_lrc(synced_text)
    } else {
        vec![]
    };

    Ok(Lyrics { lines, plain })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(lyrics.current_line_index(500), Some(0));
        assert_eq!(lyrics.current_line_index(2000), Some(0));
        assert_eq!(lyrics.current_line_index(6000), Some(1));
        assert_eq!(lyrics.current_line_index(15000), Some(2));
    }
}
