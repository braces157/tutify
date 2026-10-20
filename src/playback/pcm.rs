//! Keep network jitter off the audio callback without blocking it.
use crate::visualizer::AudioVisualizer;
use rodio::Source;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::mpsc;

// Decoder chunks contain 100 ms. Keep ten seconds available and wait for
// two seconds at startup or after starvation before resuming the stream.
pub(crate) const CAPACITY: usize = 100;
const WATERMARK: usize = 20;

pub(crate) struct Pcm {
    chunks: mpsc::Receiver<Vec<f32>>,
    current: std::vec::IntoIter<f32>,
    samples: Arc<AtomicU64>,
    rate: u32,
    visualizer: Arc<AudioVisualizer>,
    left: Option<f32>,
    buffering: bool,
    watermark: usize,
}

impl Pcm {
    pub(crate) fn new(
        chunks: mpsc::Receiver<Vec<f32>>,
        samples: Arc<AtomicU64>,
        rate: u32,
        visualizer: Arc<AudioVisualizer>,
    ) -> Self {
        Self {
            chunks,
            current: Vec::new().into_iter(),
            samples,
            rate,
            visualizer,
            left: None,
            buffering: true,
            watermark: WATERMARK,
        }
    }
    #[cfg(test)]
    pub(crate) fn legacy(mut self) -> Self {
        self.watermark = 0;
        self
    }
}

impl Iterator for Pcm {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.current.len() == 0 {
            if self.buffering && self.chunks.len() < self.watermark && !self.chunks.is_closed() {
                return Some(0.0);
            }
            match self.chunks.try_recv() {
                Ok(chunk) => {
                    self.current = chunk.into_iter();
                    self.buffering = false;
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    self.buffering = true;
                    return Some(0.0);
                }
                Err(mpsc::error::TryRecvError::Disconnected) => return None,
            }
        }
        let sample = self.current.next()?;
        self.samples.fetch_add(1, Ordering::Relaxed);
        if let Some(left) = self.left.take() {
            self.visualizer.push_sample((left + sample) * 0.5);
        } else {
            self.left = Some(sample);
        }
        Some(sample)
    }
}

impl Source for Pcm {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_waits_for_reserve_and_short_eof_drains_exactly() {
        let (tx, rx) = mpsc::channel(CAPACITY);
        let samples = Arc::new(AtomicU64::new(0));
        let mut pcm = Pcm::new(rx, samples.clone(), 44100, AudioVisualizer::new());
        for _ in 0..WATERMARK - 1 {
            tx.try_send(vec![0.25, -0.25]).unwrap();
        }
        assert!(pcm.by_ref().take(1000).all(|x| x == 0.0));
        assert_eq!(samples.load(Ordering::Relaxed), 0);
        tx.try_send(vec![0.25, -0.25]).unwrap();
        assert_eq!(
            pcm.by_ref().take(WATERMARK * 2).collect::<Vec<_>>(),
            [0.25, -0.25].repeat(WATERMARK)
        );
        assert_eq!(pcm.next(), Some(0.0));
        tx.try_send(vec![0.125, -0.125]).unwrap();
        assert!(pcm.by_ref().take(1000).all(|x| x == 0.0));
        assert_eq!(samples.load(Ordering::Relaxed), (WATERMARK * 2) as u64);
        drop(tx);
        assert_eq!(pcm.collect::<Vec<_>>(), vec![0.125, -0.125]);
        assert_eq!(samples.load(Ordering::Relaxed), (WATERMARK * 2 + 2) as u64);
    }
}
