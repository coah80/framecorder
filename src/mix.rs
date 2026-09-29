//! Mixes several audio sources into one stream, lined up by timestamp, so
//! game audio and the mic end up in one track every player will play.

use crate::audio::{CHANNELS, RATE};

/// A source this far behind the newest one counts as silent instead of
/// holding the mix back (an idle speaker sink stops sending anything).
const MAX_LAG: i64 = RATE as i64 * 3 / 10;
/// Timestamps this close to where a source left off are treated as
/// continuous, so tiny clock jitter doesn't cause clicks.
const SNAP: i64 = RATE as i64 / 20;

pub struct Mixer {
    /// Timeline position (in frames) of `acc[0]`.
    base: i64,
    acc: Vec<f32>,
    /// Where each input's last chunk ended.
    ends: Vec<Option<i64>>,
}

impl Mixer {
    pub fn new(inputs: usize) -> Self {
        Self { base: 0, acc: Vec::new(), ends: vec![None; inputs] }
    }

    /// Adds interleaved samples from `input` that start at timeline frame `pos`.
    pub fn add(&mut self, input: usize, pos: i64, samples: &[f32]) {
        let ch = CHANNELS as usize;
        let mut pos = match self.ends[input] {
            Some(end) if (pos - end).abs() <= SNAP => end,
            _ => pos,
        };
        let mut samples = samples;
        if pos < self.base {
            let skip = ((self.base - pos) as usize).min(samples.len() / ch);
            samples = &samples[skip * ch..];
            pos = self.base;
        }
        let frames = samples.len() / ch;
        let start = (pos - self.base) as usize * ch;
        let needed = start + frames * ch;
        if self.acc.len() < needed {
            self.acc.resize(needed, 0.0);
        }
        for (a, s) in self.acc[start..needed].iter_mut().zip(samples) {
            *a += s;
        }
        self.ends[input] = Some(pos + frames as i64);
    }

    /// Mixed samples every live input has delivered, with their start frame.
    /// `all` takes whatever there is, for the end of the recording.
    pub fn take(&mut self, all: bool) -> Option<(i64, Vec<f32>)> {
        let ch = CHANNELS as usize;
        let newest = self.ends.iter().flatten().copied().max()?;
        let ready = if all {
            newest
        } else {
            self.ends.iter().flatten().copied().filter(|e| newest - e <= MAX_LAG).min()?
        };
        let frames = (ready - self.base).clamp(0, (self.acc.len() / ch) as i64) as usize;
        if frames == 0 {
            return None;
        }
        let out: Vec<f32> = self.acc.drain(..frames * ch).map(|s| s.clamp(-1.0, 1.0)).collect();
        let start = self.base;
        self.base += frames as i64;
        Some((start, out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_up_and_adds() {
        let mut m = Mixer::new(2);
        m.add(0, 0, &[0.25; 8]); // frames 0..4
        m.add(1, 2, &[0.5; 8]); // frames 2..6
        let (start, out) = m.take(false).unwrap();
        assert_eq!(start, 0);
        // Frames 0-1 game only, 2-3 both.
        assert_eq!(&out[..4], &[0.25; 4]);
        assert_eq!(&out[4..8], &[0.75; 4]);
    }

    #[test]
    fn silent_input_doesnt_block() {
        let mut m = Mixer::new(2);
        m.add(1, 0, &[0.1; 2]);
        m.add(0, 0, &vec![0.2; (MAX_LAG as usize + 100) * 2]);
        let (_, out) = m.take(false).unwrap();
        assert_eq!(out.len(), (MAX_LAG as usize + 100) * 2);
    }

    #[test]
    fn clamps() {
        let mut m = Mixer::new(2);
        m.add(0, 0, &[0.9; 2]);
        m.add(1, 0, &[0.9; 2]);
        let (_, out) = m.take(true).unwrap();
        assert_eq!(out, vec![1.0, 1.0]);
    }
}
