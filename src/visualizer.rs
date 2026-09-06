use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub const BUFFER_SIZE: usize = 4096;
const BUFFER_MASK: usize = BUFFER_SIZE - 1;
pub const FFT_SIZE: usize = 1024;
const SAMPLE_RATE: f32 = 44_100.0;

const BLOCK_CHARS: [char; 8] = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A thread-safe, lock-free audio sample tap and real-time spectrum analyzer.
pub struct AudioVisualizer {
    buffer: Box<[AtomicU32; BUFFER_SIZE]>,
    write_head: AtomicUsize,
    total_samples: AtomicU64,
    state: std::sync::Mutex<VisualizerState>,
}

struct VisualizerState {
    smoothed_bars: Vec<f32>,
    peak_caps: Vec<f32>,
    mini_smoothed_bars: Vec<f32>,
    mini_last_time: Instant,
    dynamic_peak: f32,
    last_samples_seen: u64,
    last_active: Instant,
    last_frame_time: Instant,
    twiddles: Vec<(f32, f32)>,
    hann_window: [f32; FFT_SIZE],
}

impl AudioVisualizer {
    pub fn new() -> Arc<Self> {
        let mut buffer_vec = Vec::with_capacity(BUFFER_SIZE);
        for _ in 0..BUFFER_SIZE {
            buffer_vec.push(AtomicU32::new(0));
        }
        let buffer: Box<[AtomicU32; BUFFER_SIZE]> = buffer_vec
            .into_boxed_slice()
            .try_into()
            .map_err(|_| "Failed to allocate buffer")
            .unwrap();

        // Precompute twiddle factors for 1024-point FFT
        let mut twiddles = Vec::with_capacity(FFT_SIZE / 2);
        for i in 0..(FFT_SIZE / 2) {
            let angle = -2.0 * std::f32::consts::PI * (i as f32) / (FFT_SIZE as f32);
            twiddles.push((angle.cos(), angle.sin()));
        }

        // Precompute Hann window
        let mut hann_window = [0.0f32; FFT_SIZE];
        for (i, val) in hann_window.iter_mut().enumerate() {
            *val = 0.5
                * (1.0 - (2.0 * std::f32::consts::PI * (i as f32) / (FFT_SIZE as f32 - 1.0)).cos());
        }

        let now = Instant::now();
        Arc::new(Self {
            buffer,
            write_head: AtomicUsize::new(0),
            total_samples: AtomicU64::new(0),
            state: std::sync::Mutex::new(VisualizerState {
                smoothed_bars: Vec::new(),
                peak_caps: Vec::new(),
                mini_smoothed_bars: Vec::new(),
                mini_last_time: now,
                dynamic_peak: 0.1,
                last_samples_seen: 0,
                last_active: now,
                last_frame_time: now,
                twiddles,
                hann_window,
            }),
        })
    }

    /// Push a single mono audio sample in real-time. Lock-free and wait-free.
    #[inline]
    pub fn push_sample(&self, sample: f32) {
        let head = self.write_head.fetch_add(1, Ordering::Relaxed);
        let idx = head & BUFFER_MASK;
        self.buffer[idx].store(sample.to_bits(), Ordering::Relaxed);
        self.total_samples.fetch_add(1, Ordering::Relaxed);
    }

    /// Push a batch of mono audio samples. Lock-free.
    pub fn push_samples(&self, samples: &[f32]) {
        for &s in samples {
            self.push_sample(s);
        }
    }

    /// Check whether any real audio samples have been received by the tap.
    #[inline]
    pub fn has_audio_samples(&self) -> bool {
        self.total_samples.load(Ordering::Relaxed) > 0
    }

    /// Retrieve the most recent FFT_SIZE mono samples from the circular buffer.
    pub fn snapshot_recent_samples(&self) -> [f32; FFT_SIZE] {
        let head = self.write_head.load(Ordering::Relaxed);
        let mut out = [0.0f32; FFT_SIZE];
        for (i, sample) in out.iter_mut().enumerate() {
            let offset = head.wrapping_sub(FFT_SIZE).wrapping_add(i) & BUFFER_MASK;
            let bits = self.buffer[offset].load(Ordering::Relaxed);
            *sample = f32::from_bits(bits);
        }
        out
    }

    /// Computes the real-time frequency spectrum bars and peak caps for UI rendering.
    /// Returns `(bar_heights, peak_heights)`.
    pub fn get_bars_and_peaks(
        &self,
        bar_count: usize,
        max_height: usize,
        is_playing: bool,
    ) -> (Vec<usize>, Vec<usize>) {
        if bar_count == 0 || max_height == 0 {
            return (vec![], vec![]);
        }

        let total = self.total_samples.load(Ordering::Relaxed);
        let now = Instant::now();

        let mut state = match self.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };

        // Resize state vectors if bar_count changed
        if state.smoothed_bars.len() != bar_count {
            state.smoothed_bars = vec![0.0; bar_count];
            state.peak_caps = vec![0.0; bar_count];
        }

        // Determine if audio is actively flowing
        let has_new_samples = total != state.last_samples_seen;
        if has_new_samples {
            state.last_samples_seen = total;
            state.last_active = now;
        }

        let is_active =
            is_playing && now.duration_since(state.last_active) < Duration::from_millis(350);

        let targets: Vec<f32> = if is_active {
            // Snapshot samples and perform Hann-windowed FFT
            let mut samples = self.snapshot_recent_samples();
            for (i, s) in samples.iter_mut().enumerate() {
                *s *= state.hann_window[i];
            }

            let magnitudes = compute_fft_magnitudes(&samples, &state.twiddles);
            let raw_bands = calculate_frequency_bands(&magnitudes, bar_count);

            // Dynamic Auto-Sensitivity (AGC)
            let frame_max = raw_bands.iter().copied().fold(0.0f32, f32::max);
            if frame_max > state.dynamic_peak {
                state.dynamic_peak = state.dynamic_peak * 0.30 + frame_max * 0.70;
            } else {
                state.dynamic_peak = (state.dynamic_peak * 0.965).max(0.008);
            }

            raw_bands
                .iter()
                .map(|&v| {
                    let ratio = (v / state.dynamic_peak).clamp(0.0, 1.0);
                    ratio.powf(0.72)
                })
                .collect()
        } else {
            vec![0.0; bar_count]
        };

        // Ballistics smoothing
        let dt = now
            .duration_since(state.last_frame_time)
            .as_secs_f32()
            .clamp(0.005, 0.1);
        state.last_frame_time = now;

        // Attack & decay factors scaled by elapsed time
        let attack = (1.0 - (-dt / 0.035).exp()).clamp(0.5, 0.95);
        let decay = (-dt / 0.14).exp().clamp(0.65, 0.95);
        let gravity = dt * 1.6;

        let mut bar_heights = Vec::with_capacity(bar_count);
        let mut peak_heights = Vec::with_capacity(bar_count);

        for (i, &target) in targets.iter().enumerate().take(bar_count) {
            let current = state.smoothed_bars[i];

            let next = if target > current {
                current + (target - current) * attack
            } else {
                current * decay
            };
            let next = if next < 0.01 { 0.0 } else { next.min(1.0) };
            state.smoothed_bars[i] = next;

            // Peak cap physics (falls with gravity, jumps on higher signal)
            let current_peak = state.peak_caps[i];
            let next_peak = if next >= current_peak {
                next
            } else {
                (current_peak - gravity).max(next)
            };
            let next_peak = if next_peak < 0.01 {
                0.0
            } else {
                next_peak.min(1.0)
            };
            state.peak_caps[i] = next_peak;

            let h = (next * max_height as f32).round() as usize;
            let p = (next_peak * max_height as f32).round() as usize;

            bar_heights.push(h.min(max_height));
            peak_heights.push(p.min(max_height));
        }

        (bar_heights, peak_heights)
    }

    /// Retrieve real-time mini equalizer bar characters for the bottom player bar.
    pub fn get_mini_bars(&self, bar_count: usize, is_playing: bool) -> String {
        if bar_count == 0 {
            return String::new();
        }
        if !is_playing {
            return " ".repeat(bar_count);
        }

        let total = self.total_samples.load(Ordering::Relaxed);
        let now = Instant::now();

        let mut state = match self.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };

        // Resize mini state vector if bar_count changed
        if state.mini_smoothed_bars.len() != bar_count {
            state.mini_smoothed_bars = vec![0.0; bar_count];
        }

        // Determine if audio is actively flowing
        let has_new_samples = total != state.last_samples_seen;
        if has_new_samples {
            state.last_samples_seen = total;
            state.last_active = now;
        }

        let is_active =
            is_playing && now.duration_since(state.last_active) < Duration::from_millis(350);

        let targets: Vec<f32> = if is_active {
            // Snapshot samples and perform Hann-windowed FFT
            let mut samples = self.snapshot_recent_samples();
            for (i, s) in samples.iter_mut().enumerate() {
                *s *= state.hann_window[i];
            }

            let magnitudes = compute_fft_magnitudes(&samples, &state.twiddles);
            let raw_bands = calculate_frequency_bands(&magnitudes, bar_count);

            // Dynamic Auto-Sensitivity (AGC) tracking peak energy
            let frame_max = raw_bands.iter().copied().fold(0.0f32, f32::max);
            if frame_max > state.dynamic_peak {
                state.dynamic_peak = state.dynamic_peak * 0.30 + frame_max * 0.70;
            } else {
                state.dynamic_peak = (state.dynamic_peak * 0.965).max(0.008);
            }

            raw_bands
                .iter()
                .map(|&v| {
                    let ratio = (v / state.dynamic_peak).clamp(0.0, 1.25);
                    // Vibrant dynamic curve for mini equalizer
                    (ratio * 1.35).clamp(0.0, 1.0).powf(0.65)
                })
                .collect()
        } else {
            vec![0.0; bar_count]
        };

        // Mini ballistics smoothing
        let dt = now
            .duration_since(state.mini_last_time)
            .as_secs_f32()
            .clamp(0.005, 0.1);
        state.mini_last_time = now;

        let attack = (1.0 - (-dt / 0.028).exp()).clamp(0.6, 0.98);
        let decay = (-dt / 0.12).exp().clamp(0.65, 0.95);

        let mut s = String::with_capacity(bar_count * 4);
        for (i, &target) in targets.iter().enumerate().take(bar_count) {
            let current = state.mini_smoothed_bars[i];

            let next = if target > current {
                current + (target - current) * attack
            } else {
                current * decay
            };
            let next = if next < 0.005 { 0.0 } else { next.min(1.0) };
            state.mini_smoothed_bars[i] = next;

            if !is_active && next == 0.0 {
                s.push(' ');
            } else {
                // Map normalized height 0.0..=1.0 to 8 discrete block characters
                // [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█']
                let level = ((next * 8.0).floor() as usize).min(7);
                s.push(BLOCK_CHARS[level]);
            }
        }

        s
    }
}

impl Default for AudioVisualizer {
    fn default() -> Self {
        let mut buffer_vec = Vec::with_capacity(BUFFER_SIZE);
        for _ in 0..BUFFER_SIZE {
            buffer_vec.push(AtomicU32::new(0));
        }
        let buffer: Box<[AtomicU32; BUFFER_SIZE]> =
            buffer_vec.into_boxed_slice().try_into().unwrap();

        let mut twiddles = Vec::with_capacity(FFT_SIZE / 2);
        for i in 0..(FFT_SIZE / 2) {
            let angle = -2.0 * std::f32::consts::PI * (i as f32) / (FFT_SIZE as f32);
            twiddles.push((angle.cos(), angle.sin()));
        }

        let mut hann_window = [0.0f32; FFT_SIZE];
        for (i, val) in hann_window.iter_mut().enumerate() {
            *val = 0.5
                * (1.0 - (2.0 * std::f32::consts::PI * (i as f32) / (FFT_SIZE as f32 - 1.0)).cos());
        }

        let now = Instant::now();
        Self {
            buffer,
            write_head: AtomicUsize::new(0),
            total_samples: AtomicU64::new(0),
            state: std::sync::Mutex::new(VisualizerState {
                smoothed_bars: Vec::new(),
                peak_caps: Vec::new(),
                mini_smoothed_bars: Vec::new(),
                mini_last_time: now,
                dynamic_peak: 0.1,
                last_samples_seen: 0,
                last_active: now,
                last_frame_time: now,
                twiddles,
                hann_window,
            }),
        }
    }
}

/// Compute magnitudes of frequency bins using Radix-2 Cooley-Tukey FFT.
/// Returns 512 magnitude bins from 0 Hz to Nyquist (22,050 Hz).
fn compute_fft_magnitudes(
    samples: &[f32; FFT_SIZE],
    twiddles: &[(f32, f32)],
) -> [f32; FFT_SIZE / 2] {
    let mut re = *samples;
    let mut im = [0.0f32; FFT_SIZE];

    // Bit reversal permutation
    for i in 0..FFT_SIZE {
        let rev = i.reverse_bits() >> (usize::BITS - 10);
        if i < rev {
            re.swap(i, rev);
            im.swap(i, rev);
        }
    }

    // Cooley-Tukey DIT FFT stages
    let mut len = 2;
    while len <= FFT_SIZE {
        let half = len / 2;
        let step = FFT_SIZE / len;
        for i in (0..FFT_SIZE).step_by(len) {
            for j in 0..half {
                let twiddle_idx = j * step;
                let (cos, sin) = twiddles[twiddle_idx];
                let u_re = re[i + j];
                let u_im = im[i + j];
                let v_re = re[i + j + half] * cos - im[i + j + half] * sin;
                let v_im = re[i + j + half] * sin + im[i + j + half] * cos;

                re[i + j] = u_re + v_re;
                im[i + j] = u_im + v_im;
                re[i + j + half] = u_re - v_re;
                im[i + j + half] = u_im - v_im;
            }
        }
        len *= 2;
    }

    let mut magnitudes = [0.0f32; FFT_SIZE / 2];
    let norm = 2.0 / (FFT_SIZE as f32);
    for i in 0..(FFT_SIZE / 2) {
        magnitudes[i] = (re[i] * re[i] + im[i] * im[i]).sqrt() * norm;
    }

    magnitudes
}

#[inline]
fn hz_to_mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

#[inline]
fn mel_to_hz(mel: f32) -> f32 {
    700.0 * (10.0f32.powf(mel / 2595.0) - 1.0)
}

/// Group 512 FFT bins into `bar_count` Mel-spaced frequency bands (45 Hz - 9,500 Hz).
fn calculate_frequency_bands(magnitudes: &[f32; FFT_SIZE / 2], bar_count: usize) -> Vec<f32> {
    if bar_count == 0 {
        return vec![];
    }

    let min_freq = 45.0f32;
    let max_freq = 9_500.0f32;
    let min_mel = hz_to_mel(min_freq);
    let max_mel = hz_to_mel(max_freq);
    let bin_width = SAMPLE_RATE / (FFT_SIZE as f32); // ~43.07 Hz per bin

    let mut raw_bands = Vec::with_capacity(bar_count);

    for b in 0..bar_count {
        let frac_start = b as f32 / bar_count as f32;
        let frac_end = (b + 1) as f32 / bar_count as f32;

        let mel_start = min_mel + (max_mel - min_mel) * frac_start;
        let mel_end = min_mel + (max_mel - min_mel) * frac_end;

        let f_start = mel_to_hz(mel_start);
        let f_end = mel_to_hz(mel_end);
        let f_center = (f_start * f_end).sqrt();

        let k_start = ((f_start / bin_width).floor() as usize).clamp(1, FFT_SIZE / 2 - 1);
        let k_end = ((f_end / bin_width).ceil() as usize).clamp(k_start, FFT_SIZE / 2 - 1);

        let mut peak = 0.0f32;
        let mut sum = 0.0f32;
        let count = (k_end - k_start + 1) as f32;

        for &mag in &magnitudes[k_start..=k_end] {
            if mag > peak {
                peak = mag;
            }
            sum += mag;
        }

        let avg = sum / count;
        // Blended energy: transient clarity + full sustained notes
        let energy = avg * 0.35 + peak * 0.65;

        // Equalization weighting: compensating for natural 1/f falloff in recorded music
        let weight = (f_center / 65.0).powf(0.52);
        let weighted = energy * weight;

        raw_bands.push(weighted);
    }

    // Monstercat 3-tap inter-bar spatial smoothing for organic, cohesive wave motion
    let mut smoothed = vec![0.0f32; bar_count];
    for i in 0..bar_count {
        let left = if i > 0 {
            raw_bands[i - 1]
        } else {
            raw_bands[i]
        };
        let center = raw_bands[i];
        let right = if i + 1 < bar_count {
            raw_bands[i + 1]
        } else {
            raw_bands[i]
        };
        smoothed[i] = left * 0.18 + center * 0.64 + right * 0.18;
    }

    smoothed
}

/// A wrapper around a `rodio::Source` that feeds mono audio samples into the `AudioVisualizer`
/// in real time as the audio hardware pulls them.
pub struct VisualizerSource<S> {
    inner: S,
    visualizer: Arc<AudioVisualizer>,
    channel: u8,
    temp_left: f32,
}

impl<S> VisualizerSource<S> {
    pub fn new(inner: S, visualizer: Arc<AudioVisualizer>) -> Self {
        Self {
            inner,
            visualizer,
            channel: 0,
            temp_left: 0.0,
        }
    }
}

impl<S> Iterator for VisualizerSource<S>
where
    S: Iterator<Item = f32>,
{
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        let sample = self.inner.next()?;
        if self.channel == 0 {
            self.temp_left = sample;
            self.channel = 1;
        } else {
            let mono = (self.temp_left + sample) * 0.5;
            self.visualizer.push_sample(mono);
            self.channel = 0;
        }
        Some(sample)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S> rodio::Source for VisualizerSource<S>
where
    S: rodio::Source<Item = f32> + Send + 'static,
{
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fft_frequency_detection() {
        let vis = AudioVisualizer::new();

        // Feed a pure 440 Hz sine wave
        let freq = 440.0f32;
        let mut samples = [0.0f32; FFT_SIZE];
        for (i, s) in samples.iter_mut().enumerate() {
            let t = i as f32 / SAMPLE_RATE;
            *s = (2.0 * std::f32::consts::PI * freq * t).sin();
        }

        let twiddles = &vis.state.lock().unwrap().twiddles;
        let magnitudes = compute_fft_magnitudes(&samples, twiddles);

        // 440 Hz should be around bin index 10 (440 / 43.07 = ~10.2)
        let expected_bin = (freq / (SAMPLE_RATE / FFT_SIZE as f32)).round() as usize;
        let max_bin = magnitudes
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;

        assert_eq!(max_bin, expected_bin, "Peak FFT bin should match 440 Hz");
    }

    #[test]
    fn test_frequency_bands_bass_vs_treble() {
        let vis = AudioVisualizer::new();
        let twiddles = &vis.state.lock().unwrap().twiddles;

        // Low bass signal (60 Hz)
        let mut bass_samples = [0.0f32; FFT_SIZE];
        for (i, s) in bass_samples.iter_mut().enumerate() {
            let t = i as f32 / SAMPLE_RATE;
            *s = (2.0 * std::f32::consts::PI * 60.0 * t).sin();
        }
        let bass_mags = compute_fft_magnitudes(&bass_samples, twiddles);
        let bass_bars = calculate_frequency_bands(&bass_mags, 16);

        // High treble signal (8 kHz)
        let mut treble_samples = [0.0f32; FFT_SIZE];
        for (i, s) in treble_samples.iter_mut().enumerate() {
            let t = i as f32 / SAMPLE_RATE;
            *s = (2.0 * std::f32::consts::PI * 8_000.0 * t).sin();
        }
        let treble_mags = compute_fft_magnitudes(&treble_samples, twiddles);
        let treble_bars = calculate_frequency_bands(&treble_mags, 16);

        // Bass signal should have its maximum in the first few bars
        let bass_peak_bar = bass_bars
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert!(
            bass_peak_bar <= 3,
            "60 Hz should peak in low bass bars (<= 3), got {bass_peak_bar}"
        );

        // Treble signal should have its maximum in the upper bars
        let treble_peak_bar = treble_bars
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert!(
            treble_peak_bar >= 11,
            "8 kHz should peak in high bars (>= 11), got {treble_peak_bar}"
        );
    }

    #[test]
    fn test_audio_visualizer_push_and_decay() {
        let vis = AudioVisualizer::new();

        // Feed some sine wave audio
        for i in 0..2048 {
            let t = i as f32 / SAMPLE_RATE;
            let sample = (2.0 * std::f32::consts::PI * 100.0 * t).sin() * 0.8;
            vis.push_sample(sample);
        }

        // Active playing returns non-zero bars
        let (bars, peaks) = vis.get_bars_and_peaks(16, 20, true);
        assert!(
            bars.iter().any(|&b| b > 0),
            "Active audio should yield non-zero bars"
        );
        assert!(
            peaks.iter().any(|&p| p > 0),
            "Active audio should yield non-zero peaks"
        );

        let mini = vis.get_mini_bars(8, true);
        assert!(
            !mini.trim().is_empty(),
            "Mini bars should show activity when playing"
        );

        // Paused state decay
        let (paused_bars, _) = vis.get_bars_and_peaks(16, 20, false);
        // Bars decay
        assert!(paused_bars.iter().sum::<usize>() <= bars.iter().sum::<usize>());
    }

    #[test]
    fn test_mini_visualizer_independence_and_block_levels() {
        let vis = AudioVisualizer::new();

        // Feed audio
        for i in 0..2048 {
            let t = i as f32 / SAMPLE_RATE;
            let sample = (2.0 * std::f32::consts::PI * 200.0 * t).sin() * 0.9;
            vis.push_sample(sample);
        }

        // Full visualizer renders 32 bars
        let (full_bars, _) = vis.get_bars_and_peaks(32, 20, true);
        assert_eq!(full_bars.len(), 32);

        // Mini visualizer renders 10 bars
        let mini = vis.get_mini_bars(10, true);
        assert_eq!(mini.chars().count(), 10);
        // All characters should be valid block chars
        for c in mini.chars() {
            assert!(
                BLOCK_CHARS.contains(&c),
                "Character '{c}' should be in BLOCK_CHARS"
            );
        }

        // Interleaved rendering must NOT reset each other
        let (full_bars_2, _) = vis.get_bars_and_peaks(32, 20, true);
        assert_eq!(full_bars_2.len(), 32);
        assert!(full_bars_2.iter().any(|&b| b > 0));

        let mini_2 = vis.get_mini_bars(10, true);
        assert_eq!(mini_2.chars().count(), 10);
        assert!(mini_2.chars().any(|c| c != ' '));
    }
}
