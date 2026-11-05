//! Mixes several audio sources into one stream, lined up by timestamp, so
//! game audio and the mic end up in one track every player will play.
//! While someone talks, everything else ducks under the voice, so a loud
//! game never drowns the mic out.

use crate::audio::{CHANNELS, RATE};

/// A source this far behind the newest one counts as silent instead of
/// holding the mix back (an idle speaker sink stops sending anything).
const MAX_LAG: i64 = RATE as i64 * 3 / 10;
/// Timestamps this close to where a source left off are treated as
/// continuous, so tiny clock jitter doesn't cause clicks.
const SNAP: i64 = RATE as i64 / 20;

/// The voice counts as talking above this level (-40 dBFS). The Frame's mic
/// is digital silence when nobody's speaking, so only speech gets here.
const TALKING: f32 = 0.01;
/// While the voice talks, everything else stays this far under it (-6 dB),
/// whatever the mic's gain, and is left alone when it's already quieter.
const UNDER: f32 = 0.5;
/// The most everything else ducks (-26 dB), so the game never vanishes.
const FLOOR: f32 = 0.05;
/// How long after the last word the duck holds, so it doesn't pump between words.
const HOLD: u32 = RATE * 4 / 10;
/// Time constants in seconds. Levels rise fast; the one that hears the voice
/// stop falls fast, the ones that size the duck fall slowly. The duck drops
/// fast and comes back slowly.
const RISE_SECS: f32 = 0.01;
const STOP_SECS: f32 = 0.03;
const FALL_SECS: f32 = 0.3;
const DUCK_SECS: f32 = 0.015;
const RECOVER_SECS: f32 = 0.4;

pub struct Mixer {
    /// Timeline position (in frames) of `acc[0]` and `voice[0]`.
    base: i64,
    /// Every input but the voice, summed.
    acc: Vec<f32>,
    /// The voice input on its own, so the rest can duck under it.
    voice: Vec<f32>,
    voice_input: Option<usize>,
    /// Where each input's last chunk ended.
    ends: Vec<Option<i64>>,
    duck: Duck,
}

impl Mixer {
    /// `voice` is the input everything else ducks under, if any.
    pub fn new(inputs: usize, voice: Option<usize>) -> Self {
        Self {
            base: 0,
            acc: Vec::new(),
            voice: Vec::new(),
            voice_input: voice,
            ends: vec![None; inputs],
            duck: Duck::default(),
        }
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
            self.voice.resize(needed, 0.0);
        }
        let into = if Some(input) == self.voice_input { &mut self.voice } else { &mut self.acc };
        for (a, s) in into[start..needed].iter_mut().zip(samples) {
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
        let rest = self.acc.drain(..frames * ch);
        let voice = self.voice.drain(..frames * ch);
        let mut out = Vec::with_capacity(frames * ch);
        for (rest, voice) in rest.as_slice().chunks_exact(ch).zip(voice.as_slice().chunks_exact(ch)) {
            let gain = self.duck.next(rest, voice);
            out.extend(rest.iter().zip(voice).map(|(r, v)| (r * gain + v).clamp(-1.0, 1.0)));
        }
        let start = self.base;
        self.base += frames as i64;
        Some((start, out))
    }
}

/// Follows the voice's and the rest's levels and turns them into a gain for the rest.
struct Duck {
    /// Whether the voice is talking right now.
    talking: Level,
    voice: Level,
    rest: Level,
    /// The voice's level the last time it was talking, kept through pauses.
    speech: f32,
    hold: u32,
    gain: f32,
    duck: f32,
    recover: f32,
}

impl Default for Duck {
    fn default() -> Self {
        Self {
            talking: Level::new(STOP_SECS),
            voice: Level::new(FALL_SECS),
            rest: Level::new(FALL_SECS),
            speech: 0.0,
            hold: 0,
            gain: 1.0,
            duck: step(DUCK_SECS),
            recover: step(RECOVER_SECS),
        }
    }
}

impl Duck {
    /// The gain for one frame of the rest, given that frame of each.
    fn next(&mut self, rest: &[f32], voice: &[f32]) -> f32 {
        let rest = self.rest.next(rest);
        let talking = self.talking.next(voice) > TALKING;
        let voice = self.voice.next(voice);
        if talking {
            self.hold = HOLD;
            self.speech = voice;
        } else {
            self.hold = self.hold.saturating_sub(1);
        }
        let target = if self.hold > 0 { (self.speech * UNDER / rest.max(f32::EPSILON)).clamp(FLOOR, 1.0) } else { 1.0 };
        let step = if target < self.gain { self.duck } else { self.recover };
        self.gain += (target - self.gain) * step;
        self.gain
    }
}

/// A loudness (RMS) level that rises and falls at its own rates.
struct Level {
    value: f32,
    rise: f32,
    fall: f32,
}

impl Level {
    fn new(fall_secs: f32) -> Self {
        Self { value: 0.0, rise: step(RISE_SECS), fall: step(fall_secs) }
    }

    fn next(&mut self, frame: &[f32]) -> f32 {
        let power = frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32;
        let step = if power > self.value { self.rise } else { self.fall };
        self.value += (power - self.value) * step;
        self.value.sqrt()
    }
}

/// How far a one-pole smoother moves per frame for a time constant.
fn step(secs: f32) -> f32 {
    1.0 - (-1.0 / (secs * RATE as f32)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_up_and_adds() {
        let mut m = Mixer::new(2, None);
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
        let mut m = Mixer::new(2, None);
        m.add(1, 0, &[0.1; 2]);
        m.add(0, 0, &vec![0.2; (MAX_LAG as usize + 100) * 2]);
        let (_, out) = m.take(false).unwrap();
        assert_eq!(out.len(), (MAX_LAG as usize + 100) * 2);
    }

    #[test]
    fn clamps() {
        let mut m = Mixer::new(2, None);
        m.add(0, 0, &[0.9; 2]);
        m.add(1, 0, &[0.9; 2]);
        let (_, out) = m.take(true).unwrap();
        assert_eq!(out, vec![1.0, 1.0]);
    }

    #[test]
    fn the_rest_ducks_under_the_voice_as_far_as_it_needs_and_comes_back() {
        let second = RATE as usize;
        // half a second of talking, then quiet
        let mix = |game: f32, voice: f32| {
            let mut talk = vec![voice; second / 2 * 2];
            talk.resize(second * 3 * 2, 0.0);
            let mut m = Mixer::new(2, Some(1));
            m.add(0, 0, &vec![game; talk.len()]);
            m.add(1, 0, &talk);
            let (_, out) = m.take(true).unwrap();
            move |secs: f32| out[(secs * RATE as f32) as usize * 2]
        };
        let loud = mix(0.8, 0.2);
        // talking: a loud game sits half as loud as the voice
        assert!((loud(0.4) - (0.2 * UNDER + 0.2)).abs() < 0.01, "{}", loud(0.4));
        // the duck holds through a short pause, then the game comes back
        assert!(loud(0.6) < 0.2 * UNDER + 0.01, "{}", loud(0.6));
        assert!((loud(2.9) - 0.8).abs() < 0.01, "{}", loud(2.9));
        // a game already under the voice isn't touched
        let quiet = mix(0.05, 0.2);
        assert!((quiet(0.4) - 0.25).abs() < 0.001, "{}", quiet(0.4));
        // under a whisper the game still keeps a little of itself
        let whisper = mix(0.8, 0.02);
        assert!((whisper(0.4) - (0.8 * FLOOR + 0.02)).abs() < 0.005, "{}", whisper(0.4));
    }

    #[test]
    fn nothing_ducks_without_a_voice() {
        let mut m = Mixer::new(2, Some(1));
        m.add(0, 0, &[0.8; 9600]);
        m.add(1, 0, &[0.0; 9600]);
        let (_, out) = m.take(true).unwrap();
        assert!(out.iter().all(|s| *s == 0.8));
    }
}
