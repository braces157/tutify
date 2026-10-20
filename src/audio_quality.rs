//! Source-stream quality, separate from catalog metadata and Windows output format.
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Flac,
    Alac,
    Aac,
    Mp3,
    Vorbis,
    Opus,
}
impl Codec {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "flac" | "audio/flac" => Some(Self::Flac),
            "alac" => Some(Self::Alac),
            "aac" | "mp4a.40.2" | "mp4a.40.5" => Some(Self::Aac),
            "mp3" | "audio/mpeg" => Some(Self::Mp3),
            "vorbis" | "ogg" | "ogg_vorbis" => Some(Self::Vorbis),
            "opus" => Some(Self::Opus),
            _ => None,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Flac => "FLAC",
            Self::Alac => "ALAC",
            Self::Aac => "AAC",
            Self::Mp3 => "MP3",
            Self::Vorbis => "Vorbis",
            Self::Opus => "Opus",
        }
    }
    fn lossless(self) -> bool {
        matches!(self, Self::Flac | Self::Alac)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioQuality {
    pub codec: Option<Codec>,
    pub bit_depth: Option<u8>,
    pub sample_rate: Option<u32>,
    pub bitrate_kbps: Option<u32>,
}
impl AudioQuality {
    pub fn volta(value: &Value) -> Self {
        Self {
            codec: ["format", "codec", "audio_format"]
                .into_iter()
                .find_map(|key| value[key].as_str().and_then(Codec::parse)),
            bit_depth: value["bit_depth"]
                .as_u64()
                .filter(|bits| matches!(bits, 8 | 16 | 24 | 32))
                .map(|bits| bits as u8),
            sample_rate: sample_rate(&value["sample_rate"]),
            bitrate_kbps: kbps(&value["bitrate_kbps"]),
        }
    }
    pub fn youtube(value: &Value) -> Self {
        Self {
            codec: value["acodec"].as_str().and_then(Codec::parse),
            sample_rate: sample_rate(&value["asr"]),
            bitrate_kbps: kbps(&value["abr"]),
            ..Self::default()
        }
    }
    /// Read only codec/rate tokens from FFmpeg's input stream description.
    /// Signed URLs, cookies and arbitrary decoder diagnostics never leave the worker.
    pub fn decoded(self, line: &str) -> Option<Self> {
        if !line.trim_start().starts_with("Stream #") {
            return None;
        }
        let description = line.split_once("Audio: ")?.1;
        let codec = Codec::parse(description.split([',', ' ', '(']).next()?)?;
        let rate = description.split(',').find_map(|part| {
            part.trim()
                .strip_suffix(" Hz")?
                .parse::<u32>()
                .ok()
                .filter(|rate| (8000..=768000).contains(rate))
        });
        let bit_depth = codec
            .lossless()
            .then(|| {
                description.split(',').find_map(|part| {
                    let part = part.trim();
                    if let Some((_, bits)) = part.split_once('(') {
                        bits.strip_suffix(" bit)")?
                            .parse::<u8>()
                            .ok()
                            .filter(|bits| (1..=32).contains(bits))
                    } else if matches!(part, "s16" | "s16p") {
                        Some(16)
                    } else {
                        None
                    }
                })
            })
            .flatten();
        Some(Self {
            codec: Some(codec),
            bit_depth: bit_depth.or_else(|| {
                (self.codec == Some(codec))
                    .then_some(self.bit_depth)
                    .flatten()
            }),
            sample_rate: rate.or(self.sample_rate),
            ..self
        })
    }
    /// The pinned librespot player's Bitrate320 selection order, including fallbacks.
    pub fn spotify(formats: impl Iterator<Item = String>) -> Option<Self> {
        let formats: Vec<_> = formats.collect();
        [
            ("OGG_VORBIS_320", Codec::Vorbis, 320),
            ("MP3_320", Codec::Mp3, 320),
            ("MP3_256", Codec::Mp3, 256),
            ("OGG_VORBIS_160", Codec::Vorbis, 160),
            ("MP3_160", Codec::Mp3, 160),
            ("OGG_VORBIS_96", Codec::Vorbis, 96),
            ("MP3_96", Codec::Mp3, 96),
        ]
        .into_iter()
        .find(|(format, _, _)| formats.iter().any(|available| available == format))
        .map(|(_, codec, bitrate)| Self {
            codec: Some(codec),
            bitrate_kbps: Some(bitrate),
            ..Self::default()
        })
    }
    pub fn label(self) -> String {
        let mut label = self
            .codec
            .map_or_else(String::new, |codec| codec.label().into());
        let mut resolution = Vec::new();
        if self.codec.is_none_or(Codec::lossless) {
            if let Some(bits) = self.bit_depth {
                resolution.push(format!("{bits}-bit"));
            }
        } else if let Some(bitrate) = self.bitrate_kbps {
            resolution.push(format!("{bitrate} kbps"));
        }
        if let Some(rate) = self.sample_rate {
            let rate = if rate.is_multiple_of(1000) {
                (rate / 1000).to_string()
            } else {
                format!("{:.3}", f64::from(rate) / 1000.0)
                    .trim_end_matches('0')
                    .to_owned()
            };
            resolution.push(format!("{rate} kHz"));
        }
        if !resolution.is_empty() {
            if !label.is_empty() {
                label.push(' ');
            }
            label.push_str(&resolution.join(" / "));
        }
        if label.is_empty() {
            "Quality unavailable".into()
        } else {
            label
        }
    }
}
fn sample_rate(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .filter(|rate| (8000..=768000).contains(rate))
        .map(|rate| rate as u32)
}
fn kbps(value: &Value) -> Option<u32> {
    value
        .as_f64()
        .filter(|rate| rate.is_finite() && (1.0..=100000.0).contains(rate))
        .map(|rate| rate.round() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quality_uses_reported_fields_without_claiming_requested_lossless() {
        assert_eq!(
            AudioQuality::volta(&json!({"format":"flac","bit_depth":24,"sample_rate":96000}))
                .label(),
            "FLAC 24-bit / 96 kHz"
        );
        assert_eq!(
            AudioQuality::volta(
                &json!({"codec":"aac","bit_depth":16,"sample_rate":44100,"bitrate_kbps":256})
            )
            .label(),
            "AAC 256 kbps / 44.1 kHz"
        );
        assert_eq!(
            AudioQuality::volta(&json!({"sample_rate":44100})).label(),
            "44.1 kHz"
        );
        assert_eq!(AudioQuality::volta(&json!({"quality":"lossless_hq","format":"https://secret.test","sample_rate":0,"bit_depth":256})).label(), "Quality unavailable");
        assert_eq!(
            AudioQuality::youtube(&json!({"acodec":"opus","asr":48000,"abr":127.7})).label(),
            "Opus 128 kbps / 48 kHz"
        );
    }
    #[test]
    fn spotify_quality_follows_the_players_supported_format_fallbacks() {
        let selected =
            |values: &[&str]| AudioQuality::spotify(values.iter().map(|value| (*value).into()));
        assert_eq!(
            selected(&["OGG_VORBIS_96", "MP3_320"]).unwrap().label(),
            "MP3 320 kbps"
        );
        assert_eq!(
            selected(&["OGG_VORBIS_96"]).unwrap().label(),
            "Vorbis 96 kbps"
        );
        assert!(selected(&["FLAC_FLAC"]).is_none());
    }
    #[test]
    fn decoder_quality_accepts_only_audio_descriptions_and_never_raw_urls() {
        let reported = AudioQuality::volta(&json!({"bit_depth":24,"sample_rate":96000}));
        assert_eq!(
            reported
                .decoded("  Stream #0:0: Audio: flac, 96000 Hz, stereo, s32 (24 bit)")
                .unwrap()
                .label(),
            "FLAC 24-bit / 96 kHz"
        );
        assert_eq!(
            reported
                .decoded("  Stream #0:0: Audio: aac (LC), 44100 Hz, stereo, fltp")
                .unwrap()
                .label(),
            "AAC 44.1 kHz"
        );
        assert!(
            reported
                .decoded("Input #0, flac, from 'https://signed.test/?token=private'")
                .is_none()
        );
        assert!(reported.decoded("ERROR Audio: flac, 44100 Hz").is_none());
        assert!(
            reported
                .decoded("  Stream #0:0: Audio: pcm_f32le, 96000 Hz")
                .is_none()
        );
        assert_eq!(
            AudioQuality::default()
                .decoded("  Stream #0:0: Audio: flac, 192000 Hz, stereo, s32 (24 bit)")
                .unwrap()
                .label(),
            "FLAC 24-bit / 192 kHz"
        );
        assert_eq!(
            reported
                .decoded("  Stream #0:0: Audio: flac, 44100 Hz, stereo, s16")
                .unwrap()
                .label(),
            "FLAC 16-bit / 44.1 kHz"
        );
    }
}
