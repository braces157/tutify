use super::Lyrics;
use lindera::{dictionary::load_dictionary, mode::Mode, segmenter::Segmenter};
use pinyin::ToPinyin;
use std::sync::OnceLock;
use wana_kana::ConvertJapanese;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReadingMode {
    #[default]
    Auto,
    Japanese,
    Mandarin,
    Off,
}

impl ReadingMode {
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Japanese,
            Self::Japanese => Self::Mandarin,
            Self::Mandarin => Self::Off,
            Self::Off => Self::Auto,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Japanese => "romaji",
            Self::Mandarin => "pinyin",
            Self::Off => "off",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Readings {
    japanese: Option<String>,
    mandarin: Option<String>,
}

impl Readings {
    pub fn get(&self, mode: ReadingMode, japanese: bool) -> Option<&str> {
        match mode {
            ReadingMode::Auto if japanese => self.japanese.as_deref(),
            ReadingMode::Auto | ReadingMode::Mandarin => self.mandarin.as_deref(),
            ReadingMode::Japanese => self.japanese.as_deref(),
            ReadingMode::Off => None,
        }
    }

    fn new(text: &str) -> Self {
        if !text.chars().any(|c| is_kana(c) || is_han(c)) {
            return Self::default();
        }
        let normalized: String = unicode_normalization::UnicodeNormalization::nfkc(text).collect();
        Self {
            japanese: japanese_reading(&normalized).filter(|reading| reading != text),
            mandarin: (!normalized.chars().any(is_kana))
                .then(|| mandarin_reading(text))
                .flatten(),
        }
    }
}

impl Lyrics {
    pub fn prepare_readings(&mut self) {
        // Detect across the entire song, so kanji-only Japanese lines inherit
        // the language of surrounding kana instead of becoming Mandarin.
        self.japanese = self.lines.iter().any(|l| l.text.chars().any(is_kana))
            || self
                .plain
                .as_deref()
                .is_some_and(|p| p.chars().any(is_kana));
        self.readings = self.lines.iter().map(|l| Readings::new(&l.text)).collect();
        self.plain_readings = self
            .plain
            .as_deref()
            .map_or_else(Vec::new, |p| p.lines().map(Readings::new).collect());
    }
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{3096}' | '\u{30a1}'..='\u{30fa}' | '\u{ff66}'..='\u{ff9d}')
}

fn is_han(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' | '\u{20000}'..='\u{323af}')
}

fn japanese_reading(text: &str) -> Option<String> {
    static SEGMENTER: OnceLock<Option<Segmenter>> = OnceLock::new();
    let segmenter = SEGMENTER
        .get_or_init(|| match load_dictionary("embedded://ipadic") {
            Ok(dictionary) => Some(Segmenter::new(Mode::Normal, dictionary, None)),
            Err(error) => {
                log::warn!("Could not load Japanese readings dictionary: {error}");
                None
            }
        })
        .as_ref()?;
    let mut result = String::new();
    let mut previous_end = 0;
    let mut previous_word = false;
    for mut token in segmenter.segment(text.into()).ok()? {
        // Preserve source whitespace (including between English words).
        let gap = &text[previous_end..token.byte_start];
        result.push_str(gap);
        previous_end = token.byte_end;
        let surface = token.surface.to_string();
        let is_particle = token.get_detail(0) == Some("助詞");
        let pronunciation = token.get_detail(8).filter(|s| *s != "*").map(str::to_owned);
        let reading = pronunciation.unwrap_or_else(|| surface.clone());
        // IPADIC handles wa/e; を is still written ヲ in its pronunciation
        // field, so use the modern sung pronunciation o for that particle.
        let romaji = if is_particle && surface == "を" {
            "o".into()
        } else {
            reading.as_str().to_romaji()
        };
        let word = romaji.chars().next().is_some_and(char::is_alphanumeric);
        if gap.is_empty() && previous_word && word {
            result.push(' ');
        }
        result.push_str(&romaji);
        previous_word = romaji.chars().last().is_some_and(char::is_alphanumeric);
    }
    result.push_str(&text[previous_end..]);
    Some(result)
}

fn mandarin_reading(text: &str) -> Option<String> {
    let mut result = String::new();
    let mut previous_pinyin = false;
    let mut changed = false;
    for c in text.chars() {
        if let Some(reading) = c.to_pinyin() {
            if result.chars().last().is_some_and(char::is_alphanumeric) {
                result.push(' ');
            }
            result.push_str(reading.with_tone());
            previous_pinyin = true;
            changed = true;
        } else {
            if previous_pinyin && c.is_alphanumeric() {
                result.push(' ');
            }
            result.push(c);
            previous_pinyin = false;
        }
    }
    changed.then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::parse_lrc;

    #[test]
    fn japanese_dictionary_readings_include_kanji_and_spoken_particles() {
        let reading = Readings::new("君は夢を見る");
        assert_eq!(
            reading.get(ReadingMode::Japanese, false),
            Some("kimi wa yume o miru")
        );
        assert_eq!(
            Readings::new("学校へ").japanese.as_deref(),
            Some("gakkoo e")
        );
        assert_eq!(Readings::new("ｶﾀｶﾅ").japanese.as_deref(), Some("katakana"));
        assert_eq!(
            Readings::new("か\u{3099}くせい")
                .japanese
                .map(|r| r.replace(' ', "")),
            Some("gakusei".into())
        );
    }

    #[test]
    fn mandarin_tones_traditional_characters_and_mixed_text_are_preserved() {
        for text in ["你好世界", "你好世界！", "你好世界!", "愛你", "爱你"] {
            let reading = Readings::new(text);
            let pinyin = reading.get(ReadingMode::Mandarin, false).unwrap();
            assert!(!pinyin.chars().any(is_han), "{text}: {pinyin}");
        }
        assert_eq!(
            Readings::new("你好，world 2026！").mandarin.as_deref(),
            Some("nǐ hǎo，world 2026！")
        );
        assert_eq!(
            Readings::new("Love你 forever").mandarin.as_deref(),
            Some("Love nǐ forever")
        );
        assert!(Readings::new("かな").mandarin.is_none());
    }

    #[test]
    fn whole_song_detection_keeps_kanji_only_lines_japanese_and_timestamps_intact() {
        let mut lyrics = Lyrics {
            lines: parse_lrc(
                "[00:01.00]君は夢を見る\n[00:02.00]世界\n[00:03.00]\n[00:04.00]Sing along",
            ),
            ..Lyrics::default()
        };
        let original = lyrics.lines.clone();
        lyrics.prepare_readings();
        assert!(lyrics.japanese);
        assert_eq!(lyrics.lines, original);
        assert_eq!(
            lyrics.readings[1].get(ReadingMode::Auto, lyrics.japanese),
            Some("sekai")
        );
        assert_eq!(
            lyrics.readings[1].get(ReadingMode::Mandarin, lyrics.japanese),
            Some("shì jiè")
        );
        assert_eq!(lyrics.current_line_index(2500), Some(1));
        assert!(lyrics.readings[2].get(ReadingMode::Auto, true).is_none());
        assert!(lyrics.readings[3].get(ReadingMode::Auto, true).is_none());
    }

    #[test]
    fn plain_chinese_uses_pinyin_and_can_override_or_hide_readings() {
        let mut lyrics = Lyrics {
            plain: Some("你好\n\n世界\nHello\n한국어".into()),
            ..Lyrics::default()
        };
        lyrics.prepare_readings();
        assert!(!lyrics.japanese);
        assert_eq!(lyrics.plain_readings.len(), 5);
        assert_eq!(
            lyrics.plain_readings[2].get(ReadingMode::Auto, false),
            Some("shì jiè")
        );
        assert_eq!(
            lyrics.plain_readings[2].get(ReadingMode::Japanese, false),
            Some("sekai")
        );
        assert!(
            lyrics.plain_readings[2]
                .get(ReadingMode::Off, false)
                .is_none()
        );
        assert!(
            lyrics.plain_readings[3]
                .get(ReadingMode::Auto, false)
                .is_none()
        );
        assert!(
            lyrics.plain_readings[4]
                .get(ReadingMode::Auto, false)
                .is_none()
        );
        assert_eq!(
            ReadingMode::Auto.next().next().next().next(),
            ReadingMode::Auto
        );
    }
}
