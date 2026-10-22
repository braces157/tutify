//! Recording-checked lyric lookups against the providers' own HTTPS endpoints.
use super::{Lyrics, artists, key, request};
use crate::{lyrics::parse_lyrics, model::Track};
use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::Client;
use serde_json::{Value, json};
use windows::{
    Win32::{
        Foundation::LPARAM,
        Globalization::{LCMAP_SIMPLIFIED_CHINESE, LCMapStringEx},
    },
    core::w,
};

pub(super) fn has_han(value: &str) -> bool {
    value.chars().any(|c| {
        matches!(c, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}' | '\u{20000}'..='\u{323af}')
    })
}

fn simplified(value: &str) -> String {
    if !has_han(value) || value.len() > 32_768 {
        return value.to_owned();
    }
    let source: Vec<_> = value.encode_utf16().collect();
    // SAFETY: the wrapper takes bounded UTF-16 slices; the locale is a static
    // terminated string, and optional NLS/reserved/sort arguments are unused.
    let length = unsafe {
        LCMapStringEx(
            w!("zh-CN"),
            LCMAP_SIMPLIFIED_CHINESE,
            &source,
            None,
            None,
            None,
            LPARAM(0),
        )
    };
    if !(1..=65_536).contains(&length) {
        return value.to_owned();
    }
    let mut output = vec![0; length as usize];
    // SAFETY: output is sized using the preceding API call, with distinct input
    // and output buffers that remain alive throughout this synchronous call.
    let written = unsafe {
        LCMapStringEx(
            w!("zh-CN"),
            LCMAP_SIMPLIFIED_CHINESE,
            &source,
            Some(&mut output),
            None,
            None,
            LPARAM(0),
        )
    };
    if written <= 0 {
        value.to_owned()
    } else {
        String::from_utf16(&output[..written as usize]).unwrap_or_else(|_| value.to_owned())
    }
}

fn chinese_key(value: &str) -> String {
    key(&simplified(value))
}

fn query(track: &Track) -> String {
    let credits = artists(track);
    format!(
        "{} {}",
        simplified(&track.name),
        simplified(credits.last().unwrap_or(&""))
    )
}

struct Candidate {
    id: String,
    title: String,
    artists: Vec<String>,
    album: String,
    duration_ms: f64,
}

fn names(credits: &Value) -> Vec<String> {
    let mut names = Vec::new();
    let mut canonical = Vec::new();
    for (index, artist) in credits.as_array().into_iter().flatten().enumerate() {
        if let Some(name) = artist["name"].as_str() {
            canonical.push(name);
        }
        if index != 0 {
            continue;
        }
        if let Some(name) = artist["name"].as_str() {
            names.push(name.to_owned());
        }
        if let Some(title) = artist["title"].as_str() {
            names.push(title.to_owned());
        }
        // Accept explicit provider aliases, rather than guessing translations
        // or accepting a different singer based on the song's title alone.
        for field in ["alias", "alia", "tns", "trans"] {
            if let Some(alias) = artist[field].as_str() {
                names.push(alias.to_owned());
            }
            for alias in artist[field]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                names.push(alias.to_owned());
            }
        }
    }
    names.push(canonical.join(", "));
    names
}

fn netease_candidate(song: &Value) -> Option<Candidate> {
    let id = song["id"].as_u64().filter(|id| *id > 0)?.to_string();
    Some(Candidate {
        id,
        title: song["name"].as_str()?.to_owned(),
        artists: names(song.get("artists").or_else(|| song.get("ar"))?),
        album: song["album"]["name"]
            .as_str()
            .or_else(|| song["al"]["name"].as_str())
            .unwrap_or_default()
            .to_owned(),
        duration_ms: song["duration"].as_f64().or_else(|| song["dt"].as_f64())?,
    })
}

fn qq_candidate(song: &Value) -> Option<Candidate> {
    let id = song["mid"].as_str()?;
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(Candidate {
        id: id.to_owned(),
        // QQ's `name` can omit "(Live)" while `title` retains the version.
        title: song["title"]
            .as_str()
            .or_else(|| song["name"].as_str())?
            .to_owned(),
        artists: names(&song["singer"]),
        album: song["album"]["title"]
            .as_str()
            .or_else(|| song["album"]["name"].as_str())
            .unwrap_or_default()
            .to_owned(),
        duration_ms: song["interval"].as_f64()? * 1000.0,
    })
}

fn matching(mut candidates: Vec<Candidate>, track: &Track) -> Vec<Candidate> {
    let title = chinese_key(&track.name);
    let credits: Vec<_> = artists(track)
        .into_iter()
        .map(chinese_key)
        .filter(|s| !s.is_empty())
        .collect();
    candidates.retain(|song| {
        !title.is_empty()
            && chinese_key(&song.title) == title
            && song.duration_ms.is_finite()
            && song.duration_ms > 0.0
            && (song.duration_ms - f64::from(track.duration_ms)).abs() <= 5_000.0
            && song
                .artists
                .iter()
                .any(|name| credits.contains(&chinese_key(name)))
    });
    candidates.sort_by(|a, b| {
        let distance = |song: &Candidate| (song.duration_ms - f64::from(track.duration_ms)).abs();
        distance(a).total_cmp(&distance(b)).then_with(|| {
            let album_matches = |song: &Candidate| {
                track.album.as_deref().is_some_and(|album| {
                    !album.is_empty() && chinese_key(album) == chinese_key(&song.album)
                })
            };
            album_matches(b).cmp(&album_matches(a))
        })
    });
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|song| seen.insert(song.id.clone()));
    candidates.truncate(3);
    candidates
}

fn service_ok(json: &Value, expected: i64) -> Result<()> {
    match json["code"].as_i64() {
        Some(code) if code == expected => Ok(()),
        Some(code) => bail!("Service rejected the request (code {code})"),
        None => bail!("Invalid service response"),
    }
}

pub(super) async fn netease(
    client: &Client,
    base: &str,
    track: &Track,
    best: &mut Option<Lyrics>,
) -> Result<()> {
    let Some(json) = request(
        client
            .get(format!("{base}/api/search/get"))
            .query(&[
                ("s", query(track)),
                ("type", "1".into()),
                ("limit", "20".into()),
            ])
            .header("Referer", "https://music.163.com/"),
    )
    .await?
    else {
        return Ok(());
    };
    service_ok(&json, 200)?;
    let Some(songs) = json["result"]["songs"].as_array() else {
        if json["result"]["songCount"].as_u64() == Some(0) {
            return Ok(());
        }
        bail!("Invalid search response");
    };
    for song in matching(songs.iter().filter_map(netease_candidate).collect(), track) {
        let Some(json) = request(
            client
                .get(format!("{base}/api/song/lyric"))
                .query(&[("id", song.id.as_str()), ("lv", "-1")])
                .header("Referer", "https://music.163.com/"),
        )
        .await?
        else {
            continue;
        };
        service_ok(&json, 200)?;
        if json["nolyric"].as_bool() == Some(true) || json["uncollected"].as_bool() == Some(true) {
            continue;
        }
        if let Some(text) = json["lrc"]["lyric"].as_str()
            && let Some(lyrics) = catalog_lrc(text, track)
        {
            let synced = !lyrics.lines.is_empty();
            if best.is_none() || synced {
                *best = Some(lyrics);
            }
            if synced {
                return Ok(());
            }
        }
    }
    Ok(())
}

pub(super) async fn qq(
    client: &Client,
    search_base: &str,
    lyric_base: &str,
    track: &Track,
    best: &mut Option<Lyrics>,
) -> Result<()> {
    let payload = json!({"req_1": {
        "module": "music.search.SearchCgiService", "method": "DoSearchForQQMusicDesktop",
        "param": {"query": query(track), "num_per_page": 20, "page_num": 1, "search_type": 0}
    }});
    let Some(json) = request(
        client
            .get(format!("{search_base}/cgi-bin/musicu.fcg"))
            .query(&[("data", payload.to_string())])
            .header("Referer", "https://y.qq.com/"),
    )
    .await?
    else {
        return Ok(());
    };
    service_ok(&json, 0)?;
    service_ok(&json["req_1"], 0)?;
    let Some(songs) = json["req_1"]["data"]["body"]["song"]["list"].as_array() else {
        if json["req_1"]["data"]["meta"]["sum"].as_u64() == Some(0) {
            return Ok(());
        }
        bail!("Invalid search response");
    };
    for song in matching(songs.iter().filter_map(qq_candidate).collect(), track) {
        let Some(json) = request(
            client
                .get(format!(
                    "{lyric_base}/lyric/fcgi-bin/fcg_query_lyric_new.fcg"
                ))
                .query(&[
                    ("songmid", song.id.as_str()),
                    ("format", "json"),
                    ("nobase64", "1"),
                    ("g_tk", "5381"),
                    ("platform", "yqq"),
                ])
                .header("Referer", "https://y.qq.com/"),
        )
        .await?
        else {
            continue;
        };
        if json["code"].as_i64() == Some(-1901) {
            continue;
        }
        service_ok(&json, 0)?;
        let Some(text) = json["lyric"]
            .as_str()
            .filter(|text| !text.trim().is_empty())
        else {
            continue;
        };
        let text = if text.trim_start().starts_with(['[', '&']) {
            text.to_owned()
        } else {
            // nobase64=1 normally yields raw text, including untimed lyrics.
            // Some responses still encode LRC; decode only recognizable lyrics.
            STANDARD
                .decode(text.trim())
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .filter(|decoded| {
                    decoded
                        .chars()
                        .all(|ch| !ch.is_control() || ch.is_whitespace())
                })
                .unwrap_or_else(|| text.to_owned())
        };
        if let Some(lyrics) = catalog_lrc(&entities(&text), track) {
            let synced = !lyrics.lines.is_empty();
            if best.is_none() || synced {
                *best = Some(lyrics);
            }
            if synced {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn catalog_lrc(text: &str, track: &Track) -> Option<Lyrics> {
    let lines = crate::lyrics::parse_lrc(text);
    let plain = if lines.is_empty() {
        text.lines()
            .filter(|line| {
                let line = line.trim();
                !(line.starts_with('[') && line.ends_with(']') && line.contains(':'))
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let words: Vec<_> = plain
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if words.is_empty()
        || words.iter().all(|line| {
            matches!(
                chinese_key(line).as_str(),
                "纯音乐请欣赏" | "此歌曲为没有填词的纯音乐请您欣赏" | "暂无歌词" | "暂时没有歌词"
            )
        })
    {
        return None;
    }
    let mut lyrics = parse_lyrics(&json!({"syncedLyrics": text, "plainLyrics": plain}));
    let times: Vec<_> = lyrics
        .lines
        .iter()
        .filter(|line| !line.text.is_empty())
        .map(|line| line.position_ms)
        .collect();
    if times.first() == times.last()
        || times
            .last()
            .is_some_and(|time| *time > track.duration_ms.saturating_add(5_000))
    {
        // All-zero or out-of-range LRC should remain readable, without a false
        // karaoke timeline. Another provider can still supply proper timing.
        lyrics.lines.clear();
    }
    (!lyrics.is_empty()).then_some(lyrics)
}

fn entities(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some((prefix, tail)) = rest.split_once('&') {
        out.push_str(prefix);
        if let Some((name, suffix)) = tail.split_once(';').filter(|(name, _)| name.len() <= 12) {
            let decoded = match name {
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "nbsp" => Some(' '),
                _ => name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .and_then(|s| u32::from_str_radix(s, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|s| s.parse::<u32>().ok()))
                    .and_then(char::from_u32),
            };
            if let Some(c) = decoded {
                out.push(c);
                rest = suffix;
                continue;
            }
        }
        out.push('&');
        rest = tail;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests;
