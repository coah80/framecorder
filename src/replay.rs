//! The replay buffer clips come from: the last few seconds of encoded video
//! and audio, kept in memory. Nothing gets re-encoded, a clip is just these
//! packets written into a file, starting at a keyframe.

use std::collections::VecDeque;

use crate::audio::RATE;
use crate::writer::Packet;

pub struct Ring {
    /// How much to keep, in video frames.
    keep: i64,
    fps: i64,
    packets: VecDeque<Packet>,
    /// Timeline positions of the keyframes still in `packets`.
    keyframes: VecDeque<i64>,
    newest: Option<i64>,
    bytes: usize,
}

/// Video frame `pts` as an audio sample position.
pub fn audio_pos(pts: i64, fps: i64) -> i64 {
    pts * RATE as i64 / fps.max(1)
}

impl Ring {
    pub fn new(secs: u32, fps: u32) -> Self {
        Self {
            keep: secs as i64 * fps as i64,
            fps: fps as i64,
            packets: VecDeque::new(),
            keyframes: VecDeque::new(),
            newest: None,
            bytes: 0,
        }
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn push(&mut self, p: Packet) {
        if p.stream == 0 {
            if p.key {
                self.keyframes.push_back(p.pts);
            }
            self.newest = Some(p.pts);
        }
        self.bytes += p.data.len();
        self.packets.push_back(p);
        self.trim();
    }

    /// The keyframe a clip of `frames` frames ending now starts at: the last
    /// one far enough back, so clips are never shorter than asked, or the
    /// oldest one there is.
    fn start_for(&self, frames: i64) -> Option<i64> {
        let newest = self.newest?;
        let want = newest + 1 - frames;
        self.keyframes.iter().rev().find(|&&k| k <= want).or(self.keyframes.front()).copied()
    }

    fn trim(&mut self) {
        let Some(start) = self.start_for(self.keep) else { return };
        while self.keyframes.front().is_some_and(|&k| k < start) {
            self.keyframes.pop_front();
        }
        let start_audio = audio_pos(start, self.fps);
        // Packets arrive roughly in time order, so old ones are at the front.
        while let Some(p) = self.packets.front() {
            let old = if p.stream == 0 { p.pts < start } else { p.pts + p.duration <= start_audio };
            if !old {
                break;
            }
            self.bytes -= p.data.len();
            self.packets.pop_front();
        }
    }

    /// The last `secs` seconds (a bit more, to start on a keyframe): the
    /// video frame it starts at and the packets from there on.
    pub fn snapshot(&self, secs: u32) -> Option<(i64, Vec<Packet>)> {
        let start = self.start_for((secs as i64 * self.fps).min(self.keep))?;
        let start_audio = audio_pos(start, self.fps);
        let packets = self
            .packets
            .iter()
            .filter(|p| if p.stream == 0 { p.pts >= start } else { p.pts >= start_audio })
            .cloned()
            .collect();
        Some((start, packets))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const FPS: u32 = 10;

    fn video(pts: i64) -> Packet {
        // A keyframe every 2 seconds, like the encoder does.
        Packet { stream: 0, pts, duration: 1, key: pts % 20 == 0, data: Arc::from(vec![0u8; 100]) }
    }

    fn audio(pts: i64) -> Packet {
        Packet { stream: 1, pts, duration: 1024, key: true, data: Arc::from(vec![0u8; 10]) }
    }

    /// `secs` seconds of video with audio interleaved, audio a little late.
    fn fill(ring: &mut Ring, secs: i64) {
        let mut next_audio = 0;
        for f in 0..secs * FPS as i64 {
            ring.push(video(f));
            while next_audio + 1024 <= audio_pos(f, FPS as i64) - 2400 {
                ring.push(audio(next_audio));
                next_audio += 1024;
            }
        }
    }

    #[test]
    fn keeps_about_what_it_should() {
        let mut ring = Ring::new(5, FPS);
        fill(&mut ring, 60);
        let frames: Vec<i64> = ring.packets.iter().filter(|p| p.stream == 0).map(|p| p.pts).collect();
        // Newest is 599; 5 s back is 550, the keyframe before that is 540.
        assert_eq!(frames.first(), Some(&540));
        assert_eq!(frames.last(), Some(&599));
        let total: usize = ring.packets.iter().map(|p| p.data.len()).sum();
        assert_eq!(ring.bytes(), total);
    }

    #[test]
    fn empty_ring_has_no_clip() {
        assert!(Ring::new(30, FPS).snapshot(10).is_none());
    }
}
