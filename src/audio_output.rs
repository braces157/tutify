//! Negotiate the endpoint once, then deliver PCM at that exact rate.
use anyhow::{Context, Result, bail};
use rodio::cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rodio::cpal::{SampleFormat, SampleRate, SupportedStreamConfig};

// All producers supply stereo PCM at the endpoint rate. Rodio's queue exposes
// its previous source's format until next() switches sources. Keep the format
// fixed so that transient metadata cannot resample the start of a new track.
struct CallbackSource {
    queue: rodio::queue::SourcesQueueOutput<f32>,
    channels: u16,
    channel: u16,
    frame: [f32; 2],
}

impl Iterator for CallbackSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.channels == 2 {
            return self.queue.next();
        }
        if self.channel == 0 {
            self.frame = [self.queue.next()?, self.queue.next()?];
        }
        let sample = if self.channels == 1 {
            (self.frame[0] + self.frame[1]) * 0.5
        } else {
            match self.channel {
                0 => self.frame[0],
                1 => self.frame[1],
                _ => 0.0,
            }
        };
        self.channel = (self.channel + 1) % self.channels;
        Some(sample)
    }
}

fn callback_source(queue: rodio::queue::SourcesQueueOutput<f32>, channels: u16) -> CallbackSource {
    CallbackSource {
        queue,
        channels,
        channel: 0,
        frame: [0.0; 2],
    }
}

pub(crate) struct Output {
    pub sink: rodio::Sink,
    pub rate: u32,
    pub _stream: rodio::cpal::Stream,
}

fn precision(format: SampleFormat) -> u8 {
    match format {
        SampleFormat::F32 | SampleFormat::F64 => 0,
        SampleFormat::I32 | SampleFormat::U32 | SampleFormat::I64 | SampleFormat::U64 => 1,
        SampleFormat::I16 | SampleFormat::U16 => 2,
        _ => 3,
    }
}

pub(crate) fn open(source_rate: Option<u32>) -> Result<Output> {
    let device = rodio::cpal::default_host()
        .default_output_device()
        .context("No default Windows audio output; select an output in Sound settings and retry")?;
    open_device(&device, source_rate)
}

pub(crate) fn open_device(
    device: &rodio::cpal::Device,
    source_rate: Option<u32>,
) -> Result<Output> {
    let default = device.default_output_config().ok();
    let preferred_rate =
        source_rate.or_else(|| default.as_ref().map(|config| config.sample_rate().0));
    let mut native: Vec<SupportedStreamConfig> = device
        .supported_output_configs()
        .ok()
        .into_iter()
        .flatten()
        .filter(|config| config.channels() == 2)
        .filter_map(|config| config.try_with_sample_rate(SampleRate(preferred_rate?)))
        .collect();
    // Never let enumeration order choose 16-bit output over floating point/32-bit.
    native.extend(default);
    native.sort_by_key(|config| {
        let rank = precision(config.sample_format());
        (
            rank >= 2,
            Some(config.sample_rate().0) != preferred_rate,
            rank,
        )
    });
    for config in native {
        if let Ok(output) = build(device, &config) {
            log::info!(
                "Windows audio output: {} Hz, {:?}",
                output.rate,
                config.sample_format()
            );
            return Ok(output);
        }
    }
    bail!("Cannot open Windows audio output; select a working default output and retry")
}

fn build(device: &rodio::cpal::Device, config: &SupportedStreamConfig) -> Result<Output> {
    use rodio::cpal::SampleFormat::*;
    // Build CPAL directly: Rodio's OutputStream can silently substitute another
    // config, leaving a caller with the wrong rate and an extra resampling step.
    match config.sample_format() {
        F32 => build_typed::<f32>(device, config),
        F64 => build_typed::<f64>(device, config),
        I32 => build_typed::<i32>(device, config),
        U32 => build_typed::<u32>(device, config),
        I64 => build_typed::<i64>(device, config),
        U64 => build_typed::<u64>(device, config),
        I16 => build_typed::<i16>(device, config),
        U16 => build_typed::<u16>(device, config),
        I8 => build_typed::<i8>(device, config),
        U8 => build_typed::<u8>(device, config),
        _ => bail!("Unsupported audio output sample format"),
    }
}

fn build_typed<T>(device: &rodio::cpal::Device, config: &SupportedStreamConfig) -> Result<Output>
where
    T: rodio::cpal::SizedSample + rodio::cpal::FromSample<f32>,
{
    let rate = config.sample_rate().0;
    let (sink, queue) = rodio::Sink::new_idle();
    let mut source = callback_source(queue, config.channels());
    let stream = device.build_output_stream(
        &config.config(),
        move |data: &mut [T], _| {
            for sample in data {
                *sample = output_sample(source.next().unwrap_or(0.0));
            }
        },
        |error| log::error!("Windows audio stream error: {error}"),
        None,
    )?;
    stream.play()?;
    Ok(Output {
        sink,
        rate,
        _stream: stream,
    })
}

pub(crate) fn pcm_sample(bytes: [u8; 4]) -> f32 {
    let sample = f32::from_le_bytes(bytes);
    if sample.is_finite() {
        // Lossy decoding and bandlimited resampling can produce valid peaks
        // above full scale. Retain them until the sink has applied volume.
        sample
    } else {
        0.0
    }
}

fn output_sample<T>(sample: f32) -> T
where
    T: rodio::cpal::SizedSample + rodio::cpal::FromSample<f32>,
{
    let sample = if !sample.is_finite() {
        0.0
    } else if T::FORMAT.is_float() {
        // Preserve headroom through the Windows floating point mixer as well.
        sample
    } else {
        sample.clamp(-1.0, 1.0)
    };
    T::from_sample(sample)
}

/// FFmpeg's bandlimited converter handles rate changes before Rodio sees PCM.
/// Matching rates bypass resampling; float PCM retains all 24-bit FLAC samples.
pub(crate) fn pcm_args(command: &mut tokio::process::Command, rate: u32) {
    command
        .args(["-vn", "-sn", "-dn", "-ac", "2", "-af"])
        .arg(format!(
            "aresample={rate}:resampler=swr:filter_size=64:phase_shift=10:exact_rational=1:cutoff=0.97:tsf=dblp"
        ))
        .arg("-ar")
        .arg(rate.to_string())
        .args(["-f", "f32le", "-acodec", "pcm_f32le", "pipe:1"]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_peaks_retain_headroom_until_after_volume() {
        let input = [
            1.25f32,
            -1.25,
            0.5 + 1.0 / 8388608.0,
            f32::NAN,
            f32::INFINITY,
        ];
        let (sink, queue) = rodio::Sink::new_idle();
        sink.set_volume(0.5);
        sink.append(rodio::buffer::SamplesBuffer::new(
            2,
            48000,
            input
                .into_iter()
                .map(|sample| pcm_sample(sample.to_le_bytes()))
                .collect::<Vec<_>>(),
        ));
        let mut source = callback_source(queue, 2);
        let actual: Vec<_> = source
            .by_ref()
            .take(input.len())
            .map(output_sample::<f32>)
            .collect();
        assert_eq!(
            actual,
            [0.625, -0.625, (0.5 + 1.0 / 8388608.0) * 0.5, 0.0, 0.0]
        );
    }

    #[test]
    fn device_boundary_retains_float_headroom_and_bounds_integer_output() {
        for sample in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(output_sample::<f32>(sample), 0.0);
            assert_eq!(output_sample::<i16>(sample), 0);
            assert_eq!(output_sample::<u16>(sample), 32768);
        }
        assert_eq!(output_sample::<f32>(1.25), 1.25);
        assert_eq!(output_sample::<f64>(-1.25), -1.25);
        assert_eq!(output_sample::<i16>(1.25), i16::MAX);
        assert_eq!(output_sample::<i16>(-1.25), i16::MIN);
        assert_eq!(output_sample::<u16>(1.25), u16::MAX);
        assert_eq!(output_sample::<u16>(-1.25), u16::MIN);
        assert_eq!(
            output_sample::<f32>(0.5 + 1.0 / 8388608.0),
            0.5 + 1.0 / 8388608.0
        );
    }

    #[test]
    fn sink_to_callback_source_retains_24_bit_samples_at_native_rates() {
        let expected = vec![
            -1.0,
            8388607.0 / 8388608.0,
            -1.0 / 8388608.0,
            1.0 / 8388608.0,
            0.5 + 1.0 / 8388608.0,
            0.0,
        ];
        for rate in [44100, 48000, 96000, 192000, 384000] {
            let (sink, queue) = rodio::Sink::new_idle();
            sink.append(rodio::buffer::SamplesBuffer::new(2, rate, expected.clone()));
            let mut source = callback_source(queue, 2);
            assert_eq!(
                source.by_ref().take(expected.len()).collect::<Vec<_>>(),
                expected,
                "rate {rate}"
            );
            assert!(source.by_ref().take(512).all(|sample| sample == 0.0));
            sink.append(rodio::buffer::SamplesBuffer::new(2, rate, expected.clone()));
            assert_eq!(
                source.by_ref().take(expected.len()).collect::<Vec<_>>(),
                expected,
                "queue transition after silence at {rate} Hz"
            );
        }
    }

    #[test]
    #[ignore = "Opens default Windows audio with silence only; no account or music playback"]
    fn live_output_opens_verified_rate_and_drains_silence() -> Result<()> {
        for requested in [44100, 48000, 96000, 192000, 384000] {
            let output = open(Some(requested))?;
            output.sink.append(rodio::buffer::SamplesBuffer::new(
                2,
                output.rate,
                vec![0.0f32; output.rate as usize / 10],
            ));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while !output.sink.empty() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(
                output.sink.empty(),
                "Windows output did not consume silence"
            );
            println!(
                "Windows output PASS: requested {requested} Hz, opened {} Hz",
                output.rate
            );
        }
        Ok(())
    }
}
