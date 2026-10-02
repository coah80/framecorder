//! The replay buffer clips come from: the last few seconds of encoded video
//! and audio. Nothing gets re-encoded, a clip is just these packets written
//! into a file, starting at a keyframe.
//!
//! The packets' bytes live on disk, only what's needed to find them stays in
//! memory. Two minutes at 40 Mbit/s is ~600 MB, and holding that in memory
//! for hours pushes the rest of the headset into swap, which (with
//! PipeWire's realtime thread swapped out) once got PipeWire killed on
//! every clip. They go into segment files that are deleted as soon as
//! they're made: they last as long as something holds them (the buffer, a
//! clip being saved), and nothing's left behind after a crash.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::Arc;

use crate::audio::RATE;
use crate::writer::{Packet, Payload};

/// A new segment file every this many bytes, so trimming frees disk as it goes.
const SEGMENT: u64 = 64 * 1024 * 1024;
/// How much gets written before it's pushed to disk and dropped from memory.
const FLUSH_EVERY: u64 = 4 * 1024 * 1024;
/// Audio older than the buffer plus this many seconds goes, video or not.
/// The slack's for one stale-audio trim to cover a second or so of audio,
/// rather than running one per packet.
const STALE_SLACK_SECS: i64 = 4;

/// Where packet bytes go: the current segment file and how far into it.
struct Spill {
    dir: PathBuf,
    file: Option<Arc<File>>,
    at: u64,
    /// Written but not yet pushed out of memory, from here.
    flushed: u64,
}

impl Spill {
    fn new() -> Option<Self> {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
        let dir = base.join("framecorder/replay");
        std::fs::create_dir_all(&dir).ok()?;
        Some(Self { dir, file: None, at: 0, flushed: 0 })
    }

    /// Writes the bytes out, and says where they are.
    fn put(&mut self, bytes: &[u8]) -> std::io::Result<Payload> {
        if self.file.is_none() || self.at + bytes.len() as u64 > SEGMENT {
            self.flush(true);
            let path = self.dir.join(format!("segment-{}-{}", std::process::id(), crate::clock::now().as_nanos()));
            let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path)?;
            // gone from the folder, still there for whoever has it open
            let _ = std::fs::remove_file(&path);
            self.file = Some(Arc::new(file));
            self.at = 0;
            self.flushed = 0;
        }
        let file = self.file.as_ref().expect("just made");
        (&**file).write_all(bytes)?;
        let payload = Payload::Disk { file: file.clone(), offset: self.at, len: bytes.len() };
        self.at += bytes.len() as u64;
        if self.at - self.flushed >= FLUSH_EVERY {
            self.flush(false);
        }
        Ok(payload)
    }

    /// Starts writing what's new out, and drops what's already out from
    /// memory. `wait`: everything, now.
    fn flush(&mut self, wait: bool) {
        let Some(file) = &self.file else { return };
        let fd = file.as_raw_fd();
        let (from, len) = (self.flushed as i64, (self.at - self.flushed) as i64);
        unsafe {
            let how = if wait {
                libc::SYNC_FILE_RANGE_WAIT_BEFORE | libc::SYNC_FILE_RANGE_WRITE | libc::SYNC_FILE_RANGE_WAIT_AFTER
            } else {
                libc::SYNC_FILE_RANGE_WRITE
            };
            libc::sync_file_range(fd, from, len, how);
            // what an earlier flush started writing is done by now, or close
            if from > 0 {
                libc::sync_file_range(fd, 0, from, libc::SYNC_FILE_RANGE_WAIT_BEFORE | libc::SYNC_FILE_RANGE_WAIT_AFTER);
                libc::posix_fadvise(fd, 0, from, libc::POSIX_FADV_DONTNEED);
            }
            if wait {
                libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_DONTNEED);
            }
        }
        self.flushed = self.at;
    }
}

pub struct Ring {
    /// How much to keep, in video frames.
    keep: i64,
    fps: i64,
    packets: VecDeque<Packet>,
    /// Timeline positions of the keyframes still in `packets`.
    keyframes: VecDeque<i64>,
    newest: Option<i64>,
    /// End of the newest audio, in samples.
    newest_audio: i64,
    bytes: usize,
    /// None: keeping the bytes in memory (no disk, or it failed).
    spill: Option<Spill>,
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
            newest_audio: 0,
            bytes: 0,
            spill: Spill::new(),
        }
    }

    /// Keeps everything in memory, for tests.
    #[cfg(test)]
    fn in_memory(secs: u32, fps: u32) -> Self {
        Self { spill: None, ..Self::new(secs, fps) }
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn push(&mut self, mut p: Packet) {
        if let (Some(spill), Payload::Mem(bytes)) = (&mut self.spill, &p.data) {
            match spill.put(bytes) {
                Ok(on_disk) => p.data = on_disk,
                Err(e) => {
                    log::warn!("replay buffer: couldn't write to disk ({e}), keeping it in memory");
                    self.spill = None;
                }
            }
        }
        if p.stream > 0 {
            self.newest_audio = self.newest_audio.max(p.pts + p.duration);
        }
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
        self.trim_stale_audio();
    }

    /// With the panels off there's no new video, so the trimming above stops,
    /// but audio keeps coming. Past the buffer's length plus a few seconds,
    /// it goes by its own clock.
    fn trim_stale_audio(&mut self) {
        let limit = self.newest_audio - audio_pos(self.keep + STALE_SLACK_SECS * self.fps, self.fps);
        let oldest = self.packets.iter().find(|p| p.stream > 0).map(|p| p.pts);
        if oldest.is_none_or(|o| o >= limit) {
            return;
        }
        // down to half the slack, so the next one's a couple of seconds off
        let cut = self.newest_audio - audio_pos(self.keep + STALE_SLACK_SECS / 2 * self.fps, self.fps);
        let mut freed = 0;
        self.packets.retain(|p| {
            let old = p.stream > 0 && p.pts + p.duration <= cut;
            if old {
                freed += p.data.len();
            }
            !old
        });
        self.bytes -= freed;
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

    const FPS: u32 = 10;

    fn video(pts: i64) -> Packet {
        // A keyframe every 2 seconds, like the encoder does.
        Packet { stream: 0, pts, duration: 1, key: pts % 20 == 0, data: vec![0u8; 100].into() }
    }

    fn audio(pts: i64) -> Packet {
        Packet { stream: 1, pts, duration: 1024, key: true, data: vec![0u8; 10].into() }
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
        let mut ring = Ring::in_memory(5, FPS);
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
        assert!(Ring::in_memory(30, FPS).snapshot(10).is_none());
    }

    #[test]
    fn bytes_go_to_disk_and_come_back() {
        let dir = std::env::temp_dir().join(format!("framecorder-replay-{}", std::process::id()));
        let spill = Spill { dir: dir.clone(), file: None, at: 0, flushed: 0 };
        std::fs::create_dir_all(&dir).unwrap();
        let mut ring = Ring { spill: Some(spill), ..Ring::in_memory(5, FPS) };
        let bytes: Vec<u8> = (0..=255).collect();
        ring.push(Packet { stream: 0, pts: 0, duration: 1, key: true, data: bytes.clone().into() });
        ring.push(Packet { stream: 0, pts: 1, duration: 1, key: false, data: vec![7u8; 3].into() });
        let (_, clip) = ring.snapshot(5).unwrap();
        assert!(clip.iter().all(|p| matches!(p.data, Payload::Disk { .. })));
        let mut out = vec![0u8; 256];
        clip[0].data.read_into(&mut out).unwrap();
        assert_eq!(out, bytes);
        let mut out = vec![0u8; 3];
        clip[1].data.read_into(&mut out).unwrap();
        assert_eq!(out, [7, 7, 7]);
        // the segment's already gone from the folder
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn audio_without_video_still_gets_trimmed() {
        // the panels go off after 10 s: audio keeps coming for 10 minutes
        let mut ring = Ring::in_memory(5, FPS);
        fill(&mut ring, 10);
        let rate = RATE as i64;
        let mut pts = audio_pos(10 * FPS as i64, FPS as i64);
        while pts < 600 * rate {
            ring.push(audio(pts));
            pts += 1024;
        }
        let audio_secs = ring.packets.iter().filter(|p| p.stream > 0).count() as i64 * 1024 / rate;
        assert!(audio_secs <= 5 + STALE_SLACK_SECS + 1, "kept {audio_secs} s of audio");
    }
}

