//! The session: hardware encoded video passes straight through, audio gets
//! AAC encoded here (a few percent of one core at most), and the packets go
//! wherever they're wanted: a recording being written, the replay buffer
//! clips come from, or both. One encoder feeds all of them. Runs on its own
//! thread so nothing here ever stalls the capture loop.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::aac;
use crate::audio::{AudioChunk, Source, RATE};
use crate::encoder::Codec;
use crate::mix::Mixer;
use crate::replay::{audio_pos, Ring};
use crate::writer::{self, Packet, Saved, Streams};

/// A source this far behind the video counts as silent and gets padded, so
/// every file has continuous audio even when nothing plays.
const QUIET_AFTER: i64 = RATE as i64;
/// How long a finished clip or recording waits for its last audio to arrive.
const AUDIO_GRACE_SECS: i64 = 1;
/// The same wait in wall time, for when no more video is coming (paused).
const STOP_WAIT: Duration = Duration::from_millis(1500);
/// Clips can't come faster than this, so a stuck button or someone mashing
/// it can't pile up files being written.
const CLIP_GAP_SECS: i64 = 1;
/// How often to check on files that are wrapping up when nothing else happens.
const TICK: Duration = Duration::from_millis(250);

pub struct VideoPacket {
    pub data: Vec<u8>,
    /// Microseconds since the start of the recording.
    pub pts_us: u64,
    pub key: bool,
}

pub enum Msg {
    Video(VideoPacket),
    Audio(AudioChunk),
    /// Recording paused at this CLOCK_MONOTONIC time (ns).
    Pause(u64),
    /// Recording resumed at this CLOCK_MONOTONIC time (ns).
    Resume(u64),
    /// Start writing a recording here, from the next keyframe.
    Record(PathBuf),
    StopRecord,
    /// Save the last `secs` seconds from the replay buffer.
    Clip { secs: u32, path: PathBuf },
}

/// One audio track in the file: its title and the sources mixed into it.
pub struct TrackSpec {
    pub title: &'static str,
    pub inputs: Vec<Source>,
}

pub struct Config {
    /// A recording to start right away, if any.
    pub path: Option<PathBuf>,
    /// Seconds of replay buffer to keep, if clipping.
    pub replay_secs: Option<u32>,
    /// Say on stdout when files are saved, for the dashboard tab.
    pub announce: bool,
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub audio_tracks: Vec<TrackSpec>,
    pub audio_bitrate: u32,
    /// CLOCK_MONOTONIC start of the recording, in nanoseconds.
    pub start_ns: u64,
}

#[derive(Default, Debug)]
pub struct Stats {
    pub video_frames: u64,
    pub bytes: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Recording,
    Clip,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Recording => "recording",
            Kind::Clip => "clip",
        }
    }
}

/// A file taking packets from video frame `start` on.
struct Output {
    kind: Kind,
    file: writer::Handle,
    start: i64,
    /// First video frame that's no longer part of it, once it's been stopped.
    end: Option<i64>,
    /// When it was stopped.
    stopped: Option<Instant>,
}

struct Session {
    cfg: Config,
    fps: i64,
    tracks: Vec<aac::Track>,
    mixers: Vec<Option<Mixer>>,
    /// Where each track's encoded audio has got to.
    audio_out: Vec<i64>,
    streams: Option<Arc<Streams>>,
    last_video_pts: i64,
    /// Finished pauses, relative to the start (ns), and one still going.
    pauses: Vec<(i64, i64)>,
    paused_at: Option<i64>,
    ring: Option<Ring>,
    /// A recording waiting for a keyframe to start on.
    start_request: Option<PathBuf>,
    outputs: Vec<Output>,
    /// Files being finished in the background.
    closing: Vec<(Kind, JoinHandle<Result<Saved>>)>,
    stats: Stats,
    recordings: u32,
    /// Where the video was when the last clip was taken.
    last_clip: Option<i64>,
}

impl Session {
    fn new(cfg: Config) -> Result<Self> {
        let mut tracks = Vec::new();
        let mut mixers = Vec::new();
        for (i, spec) in cfg.audio_tracks.iter().enumerate() {
            tracks.push(aac::Track::new(i + 1, cfg.audio_bitrate)?);
            mixers.push((spec.inputs.len() > 1).then(|| Mixer::new(spec.inputs.len())));
        }
        let ring = cfg.replay_secs.map(|s| Ring::new(s, cfg.fps));
        if let Some(secs) = cfg.replay_secs {
            log::info!("keeping the last {secs} s for clips");
        }
        Ok(Self {
            fps: cfg.fps as i64,
            audio_out: vec![0; tracks.len()],
            tracks,
            mixers,
            streams: None,
            last_video_pts: -1,
            pauses: Vec::new(),
            paused_at: None,
            ring,
            start_request: cfg.path.clone(),
            outputs: Vec::new(),
            closing: Vec::new(),
            stats: Stats::default(),
            recordings: 0,
            last_clip: None,
            cfg,
        })
    }

    fn handle(&mut self, msg: Msg) -> Result<()> {
        match msg {
            Msg::Video(p) => self.video(p)?,
            Msg::Audio(chunk) => self.audio(chunk)?,
            Msg::Pause(at) => self.paused_at = Some(at as i64 - self.cfg.start_ns as i64),
            Msg::Resume(at) => {
                if let Some(since) = self.paused_at.take() {
                    self.pauses.push((since, at as i64 - self.cfg.start_ns as i64));
                }
            }
            Msg::Record(path) => {
                if self.outputs.iter().any(|o| o.kind == Kind::Recording && o.end.is_none()) || self.start_request.is_some() {
                    log::warn!("already recording, ignoring another start");
                } else {
                    self.start_request = Some(path);
                }
            }
            Msg::StopRecord => {
                if self.start_request.take().is_some() {
                    self.announce_failure(Kind::Recording, "no video was recorded");
                }
                let end = self.last_video_pts + 1;
                for o in self.outputs.iter_mut().filter(|o| o.kind == Kind::Recording && o.end.is_none()) {
                    o.end = Some(end);
                    o.stopped = Some(Instant::now());
                }
                if self.ring.is_none() {
                    if let Some(secs) = self.cfg.replay_secs {
                        log::info!("replay buffer back on");
                        self.ring = Some(Ring::new(secs, self.cfg.fps));
                        self.last_clip = None;
                    }
                }
            }
            Msg::Clip { secs, path } => self.clip(secs, path),
        }
        self.close_finished(false);
        self.reap(false);
        Ok(())
    }

    fn recorded_time(&self, t: i64) -> Option<i64> {
        recorded_time(&self.pauses, self.paused_at, t)
    }

    fn video(&mut self, p: VideoPacket) -> Result<()> {
        let (params, has_picture) = split_parameter_sets(&p.data, self.cfg.codec);
        if self.streams.is_none() {
            if params.is_empty() {
                log::warn!("encoder output started without stream headers, dropping a packet");
                return Ok(());
            }
            let audio = self
                .tracks
                .iter()
                .zip(&self.cfg.audio_tracks)
                .map(|(t, spec)| Ok((spec.title, t.params()?)))
                .collect::<Result<Vec<_>>>()?;
            self.streams = Some(Arc::new(Streams {
                codec: self.cfg.codec,
                width: self.cfg.width,
                height: self.cfg.height,
                fps: self.cfg.fps,
                video_params: params,
                audio,
            }));
        }
        if !has_picture {
            return Ok(());
        }

        // The encoder gets real vblank times, the file gets a clean frame grid.
        let mut pts = (p.pts_us as f64 * self.cfg.fps as f64 / 1e6).round() as i64;
        if pts <= self.last_video_pts {
            pts = self.last_video_pts + 1;
        }
        self.last_video_pts = pts;

        if p.key {
            if let Some(path) = self.start_request.take() {
                self.start(Kind::Recording, path, pts, Vec::new(), None);
            }
        }
        self.route(Packet { stream: 0, pts, duration: 1, key: p.key, data: p.data.into() });

        // Keep audio running through quiet stretches.
        let quiet = audio_pos(pts, self.fps) - QUIET_AFTER;
        let mut out = Vec::new();
        for t in &mut self.tracks {
            if t.end() < quiet {
                t.fill_to(quiet, &mut out)?;
            }
        }
        for p in out {
            self.route(p);
        }
        Ok(())
    }

    fn audio(&mut self, chunk: AudioChunk) -> Result<()> {
        let Some(start_ns) = self.recorded_time(chunk.time_ns as i64 - self.cfg.start_ns as i64) else {
            return Ok(());
        };
        let pos = start_ns * RATE as i64 / 1_000_000_000;
        let mut out = Vec::new();
        for track in 0..self.tracks.len() {
            let Some(input) = self.cfg.audio_tracks[track].inputs.iter().position(|s| *s == chunk.source) else {
                continue;
            };
            match &mut self.mixers[track] {
                Some(mixer) => {
                    mixer.add(input, pos, &chunk.samples);
                    while let Some((at, mixed)) = self.mixers[track].as_mut().and_then(|m| m.take(false)) {
                        self.tracks[track].place(at, &mixed, &mut out)?;
                    }
                }
                None => self.tracks[track].place(pos, &chunk.samples, &mut out)?,
            }
        }
        for p in out {
            self.route(p);
        }
        Ok(())
    }

    /// Hands a packet to the replay buffer and every file that wants it,
    /// with timestamps moved so each file starts at zero.
    fn route(&mut self, p: Packet) {
        if p.stream > 0 {
            let track = p.stream - 1;
            self.audio_out[track] = self.audio_out[track].max(p.pts + p.duration);
        }
        for o in &self.outputs {
            if let Some(rebased) = rebase(&p, o.start, o.end, self.fps) {
                o.file.send(rebased);
            }
        }
        if let Some(ring) = &mut self.ring {
            ring.push(p);
        }
    }

    /// Starts a file at video frame `start`, with `backlog` already in it.
    fn start(&mut self, kind: Kind, path: PathBuf, start: i64, backlog: Vec<Packet>, end: Option<i64>) {
        let Some(streams) = self.streams.clone() else {
            self.announce_failure(kind, "nothing recorded yet");
            return;
        };
        match writer::Handle::spawn(&path, &streams) {
            Ok(file) => {
                for p in backlog {
                    if let Some(rebased) = rebase(&p, start, end, self.fps) {
                        file.send(rebased);
                    }
                }
                if kind == Kind::Recording {
                    log::info!("writing {}", path.display());
                    if self.cfg.announce {
                        println!("recording {}", path.display());
                    }
                    // The recording has it all, no need to hold the last few
                    // seconds too.
                    if let Some(ring) = self.ring.take() {
                        log::info!("replay buffer off while recording ({:.0} MB freed)", ring.bytes() as f64 / 1e6);
                    }
                }
                self.outputs.push(Output { kind, file, start, end, stopped: end.map(|_| Instant::now()) });
            }
            Err(e) => self.announce_failure(kind, &format!("{e:#}")),
        }
    }

    fn clip(&mut self, secs: u32, path: PathBuf) {
        if self.ring.is_none() && self.cfg.replay_secs.is_some() {
            log::info!("recording, not clipping");
            if self.cfg.announce {
                println!("busy recording");
            }
            return;
        }
        let now = self.last_video_pts + 1;
        if self.last_clip.is_some_and(|last| now - last < CLIP_GAP_SECS * self.fps) {
            log::debug!("clip asked for right after another, skipping");
            return;
        }
        let Some(snapshot) = self.ring.as_ref().and_then(|r| r.snapshot(secs)) else {
            self.announce_failure(Kind::Clip, if self.ring.is_some() { "nothing to clip yet" } else { "clipping is off" });
            return;
        };
        let (start, backlog) = snapshot;
        if let Some(ring) = &self.ring {
            log::info!("clipping from a {:.0} MB replay buffer", ring.bytes() as f64 / 1e6);
        }
        self.last_clip = Some(now);
        self.start(Kind::Clip, path, start, backlog, Some(now));
    }

    /// Stopped files get closed once their audio has caught up with where
    /// the video stopped, or a moment later regardless.
    fn close_finished(&mut self, all: bool) {
        let mut i = 0;
        while i < self.outputs.len() {
            let o = &self.outputs[i];
            let done = match o.end {
                Some(end) => {
                    let audio_end = audio_pos(end, self.fps);
                    all || self.audio_out.iter().all(|&a| a >= audio_end)
                        || self.last_video_pts >= end + AUDIO_GRACE_SECS * self.fps
                        || o.stopped.is_some_and(|t| t.elapsed() >= STOP_WAIT)
                }
                None => all,
            };
            if done {
                let o = self.outputs.swap_remove(i);
                self.closing.push((o.kind, o.file.close()));
            } else {
                i += 1;
            }
        }
    }

    /// Picks up files that finished writing and says so.
    fn reap(&mut self, wait: bool) {
        let mut i = 0;
        while i < self.closing.len() {
            if !wait && !self.closing[i].1.is_finished() {
                i += 1;
                continue;
            }
            let (kind, thread) = self.closing.swap_remove(i);
            let result = thread.join().unwrap_or_else(|_| Err(anyhow::anyhow!("the writer thread crashed")));
            match result {
                Ok(saved) => {
                    let secs = saved.seconds(self.cfg.fps);
                    log::info!("saved {} {} ({secs:.1} s, {:.1} MB)", kind.name(), saved.path.display(), saved.bytes as f64 / 1e6);
                    if self.cfg.announce {
                        println!("saved {} {} {secs:.1} {}", kind.name(), saved.bytes, saved.path.display());
                    }
                    if kind == Kind::Recording {
                        self.stats.video_frames += saved.frames;
                        self.stats.bytes += saved.bytes;
                        self.recordings += 1;
                    }
                }
                Err(e) => self.announce_failure(kind, &format!("{e:#}")),
            }
        }
    }

    fn announce_failure(&self, kind: Kind, why: &str) {
        log::error!("{} failed: {why}", kind.name());
        if self.cfg.announce {
            println!("failed {} {why}", kind.name());
        }
    }

    /// Runs every audio track to the end of the video, even one that never
    /// heard a sound (MP4 would drop it entirely), and closes everything.
    fn finish(mut self) -> Result<Stats> {
        let end = audio_pos(self.last_video_pts + 1, self.fps);
        let mut out = Vec::new();
        for track in 0..self.tracks.len() {
            if let Some((at, mixed)) = self.mixers[track].as_mut().and_then(|m| m.take(true)) {
                self.tracks[track].place(at, &mixed, &mut out)?;
            }
            self.tracks[track].fill_to(end, &mut out)?;
            self.tracks[track].flush(&mut out)?;
        }
        for p in out {
            self.route(p);
        }
        let end = self.last_video_pts + 1;
        for o in &mut self.outputs {
            o.end.get_or_insert(end);
        }
        self.close_finished(true);
        self.reap(true);
        if self.cfg.path.is_some() && self.recordings == 0 {
            bail!("no video was recorded");
        }
        Ok(self.stats)
    }
}

/// `p` moved onto the timeline of a file that starts at video frame
/// `start` (and stops before `end`), or None if it's outside it.
fn rebase(p: &Packet, start: i64, end: Option<i64>, fps: i64) -> Option<Packet> {
    let (from, to) = if p.stream == 0 {
        (start, end)
    } else {
        (audio_pos(start, fps), end.map(|e| audio_pos(e, fps)))
    };
    if p.pts < from || to.is_some_and(|to| p.pts >= to) {
        return None;
    }
    Some(Packet { pts: p.pts - from, ..p.clone() })
}

pub fn run(cfg: Config, rx: Receiver<Msg>) -> Result<Stats> {
    let mut session = Session::new(cfg).context("setting up the output")?;
    // Senders all hang up once capture is done, which ends this loop. On an
    // error the receiver goes away, which tells the capture side to stop.
    let mut failure = None;
    loop {
        match rx.recv_timeout(TICK) {
            Ok(msg) => {
                if let Err(e) = session.handle(msg) {
                    failure = Some(e);
                    break;
                }
            }
            // Nothing coming in (paused): still finish off stopped files.
            Err(RecvTimeoutError::Timeout) => {
                session.close_finished(false);
                session.reap(false);
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let finished = session.finish();
    match failure {
        Some(e) => Err(e),
        None => finished,
    }
}

/// Moves a time (ns since start) onto the recording's timeline, with
/// paused stretches cut out. None if it falls inside a pause.
fn recorded_time(pauses: &[(i64, i64)], paused_at: Option<i64>, t: i64) -> Option<i64> {
    if paused_at.is_some_and(|since| t >= since) {
        return None;
    }
    let mut cut = 0;
    for &(a, b) in pauses {
        if t >= b {
            cut += b - a;
        } else if t >= a {
            return None;
        }
    }
    Some(t - cut)
}

/// Pulls the parameter sets (VPS/SPS/PPS) out of an Annex B packet. Also
/// says whether the packet carries an actual picture.
fn split_parameter_sets(data: &[u8], codec: Codec) -> (Vec<u8>, bool) {
    let mut params = Vec::new();
    let mut has_picture = false;
    for nal in nal_units(data) {
        let Some(&first) = nal.first() else { continue };
        let is_param = match codec {
            Codec::Hevc => matches!((first >> 1) & 0x3f, 32..=34),
            Codec::H264 => matches!(first & 0x1f, 7 | 8),
        };
        if is_param {
            params.extend_from_slice(&[0, 0, 0, 1]);
            params.extend_from_slice(nal);
        } else {
            let is_vcl = match codec {
                Codec::Hevc => (first >> 1) & 0x3f < 32,
                Codec::H264 => matches!(first & 0x1f, 1..=5),
            };
            has_picture |= is_vcl;
        }
    }
    (params, has_picture)
}

fn nal_units(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let ends: Vec<usize> = starts
        .iter()
        .skip(1)
        .map(|&s| {
            let mut e = s - 3;
            // Trailing zero belongs to a 4 byte start code.
            while e > 0 && data[e - 1] == 0 {
                e -= 1;
            }
            e
        })
        .chain(std::iter::once(data.len()))
        .collect();
    starts.into_iter().zip(ends).map(move |(s, e)| &data[s..e.max(s)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_hevc_headers() {
        let data = [
            0, 0, 0, 1, 0x40, 1, 0xaa, // VPS
            0, 0, 0, 1, 0x42, 1, 0xbb, // SPS
            0, 0, 1, 0x44, 1, 0xcc, // PPS
            0, 0, 0, 1, 0x26, 1, 0xdd, 0, // IDR slice
        ];
        let (params, pic) = split_parameter_sets(&data, Codec::Hevc);
        assert!(pic);
        assert_eq!(params, [0, 0, 0, 1, 0x40, 1, 0xaa, 0, 0, 0, 1, 0x42, 1, 0xbb, 0, 0, 0, 1, 0x44, 1, 0xcc]);
    }

    #[test]
    fn pauses_are_cut_out() {
        let pauses = [(100, 150), (300, 400)];
        assert_eq!(recorded_time(&pauses, None, 50), Some(50));
        assert_eq!(recorded_time(&pauses, None, 120), None);
        assert_eq!(recorded_time(&pauses, None, 200), Some(150));
        assert_eq!(recorded_time(&pauses, None, 450), Some(300));
        assert_eq!(recorded_time(&pauses, Some(500), 499), Some(349));
        assert_eq!(recorded_time(&pauses, Some(500), 600), None);
        // Started paused: nothing counts until the first resume.
        assert_eq!(recorded_time(&[], Some(0), 10), None);
        assert_eq!(recorded_time(&[(0, 1000)], None, 1500), Some(500));
    }

    #[test]
    fn header_only_packet_has_no_picture() {
        let data = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x68, 3];
        let (params, pic) = split_parameter_sets(&data, Codec::H264);
        assert!(!pic);
        assert_eq!(params.len(), data.len());
    }

    fn packet(stream: usize, pts: i64) -> Packet {
        Packet { stream, pts, duration: 1, key: false, data: vec![0u8; 1].into() }
    }

    #[test]
    fn rebase_moves_files_to_zero() {
        // 60 fps: frame 120 is 2 s, sample 96000.
        assert_eq!(rebase(&packet(0, 130), 120, None, 60).unwrap().pts, 10);
        assert!(rebase(&packet(0, 119), 120, None, 60).is_none());
        assert!(rebase(&packet(0, 180), 120, Some(180), 60).is_none());
        assert_eq!(rebase(&packet(1, 96_000 + 480), 120, None, 60).unwrap().pts, 480);
        assert!(rebase(&packet(1, 95_000), 120, None, 60).is_none());
        assert!(rebase(&packet(1, 144_000), 120, Some(180), 60).is_none());
    }
}
