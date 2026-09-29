//! Runs the whole session with real encoded video (libx264 standing in for
//! the headset's hardware encoder) and a tone for game audio, then checks
//! the files that come out: recordings started and stopped mid-session, and
//! clips out of the replay buffer.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::mpsc;

use ffmpeg_sys_next as ff;

use crate::audio::{AudioChunk, Source, CHANNELS, RATE};
use crate::encoder::Codec;
use crate::mux::{self, Msg, TrackSpec, VideoPacket};

const FPS: u32 = 30;
const W: u32 = 320;
const H: u32 = 240;
const CHUNK: usize = 1024;

/// Encodes `secs` seconds of H.264 with a keyframe every 2 s.
fn encode_video(secs: u32) -> Option<Vec<VideoPacket>> {
    unsafe {
        let codec = ff::avcodec_find_encoder_by_name(c"libx264".as_ptr());
        if codec.is_null() {
            return None;
        }
        let mut ctx = ff::avcodec_alloc_context3(codec);
        (*ctx).width = W as i32;
        (*ctx).height = H as i32;
        (*ctx).pix_fmt = ff::AVPixelFormat::AV_PIX_FMT_YUV420P;
        (*ctx).time_base = ff::AVRational { num: 1, den: FPS as i32 };
        (*ctx).framerate = ff::AVRational { num: FPS as i32, den: 1 };
        (*ctx).gop_size = (FPS * 2) as i32;
        (*ctx).keyint_min = (FPS * 2) as i32;
        (*ctx).max_b_frames = 0;
        ff::av_opt_set((*ctx).priv_data, c"preset".as_ptr(), c"ultrafast".as_ptr(), 0);
        ff::av_opt_set((*ctx).priv_data, c"tune".as_ptr(), c"zerolatency".as_ptr(), 0);
        ff::av_opt_set((*ctx).priv_data, c"x264-params".as_ptr(), c"scenecut=0".as_ptr(), 0);
        assert!(ff::avcodec_open2(ctx, codec, ptr::null_mut()) >= 0);

        let mut frame = ff::av_frame_alloc();
        (*frame).format = (*ctx).pix_fmt as i32;
        (*frame).width = W as i32;
        (*frame).height = H as i32;
        assert!(ff::av_frame_get_buffer(frame, 0) >= 0);
        let mut pkt = ff::av_packet_alloc();
        let mut out = Vec::new();
        let receive = |ctx, out: &mut Vec<VideoPacket>| loop {
            if ff::avcodec_receive_packet(ctx, pkt) < 0 {
                break;
            }
            let p = &*pkt;
            out.push(VideoPacket {
                data: std::slice::from_raw_parts(p.data, p.size as usize).to_vec(),
                pts_us: (p.pts as u64) * 1_000_000 / FPS as u64,
                key: p.flags & ff::AV_PKT_FLAG_KEY != 0,
            });
            ff::av_packet_unref(pkt);
        };
        for i in 0..(secs * FPS) as i64 {
            assert!(ff::av_frame_make_writable(frame) >= 0);
            for plane in 0..3 {
                let (h, value) = if plane == 0 { (H, (i * 3 % 256) as u8) } else { (H / 2, 128) };
                let stride = (*frame).linesize[plane] as usize;
                std::slice::from_raw_parts_mut((*frame).data[plane], stride * h as usize).fill(value);
            }
            (*frame).pts = i;
            assert!(ff::avcodec_send_frame(ctx, frame) >= 0);
            receive(ctx, &mut out);
        }
        ff::avcodec_send_frame(ctx, ptr::null());
        receive(ctx, &mut out);
        ff::av_packet_free(&mut pkt);
        ff::av_frame_free(&mut frame);
        ff::avcodec_free_context(&mut ctx);
        Some(out)
    }
}

fn tone(start: usize) -> Vec<f32> {
    (start..start + CHUNK)
        .flat_map(|i| {
            let s = (i as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin() * 0.2;
            [s; CHANNELS as usize]
        })
        .collect()
}

/// Duration (s) of each stream in a file, video first.
fn probe(path: &Path) -> Vec<f64> {
    unsafe {
        let name = CString::new(path.to_str().unwrap()).unwrap();
        let mut fmt = ptr::null_mut();
        assert!(ff::avformat_open_input(&mut fmt, name.as_ptr(), ptr::null(), ptr::null_mut()) >= 0, "can't open {}", path.display());
        assert!(ff::avformat_find_stream_info(fmt, ptr::null_mut()) >= 0);
        let streams = (0..(*fmt).nb_streams as usize)
            .map(|i| {
                let st = *(*fmt).streams.add(i);
                (*st).duration as f64 * (*st).time_base.num as f64 / (*st).time_base.den as f64
            })
            .collect();
        ff::avformat_close_input(&mut fmt);
        streams
    }
}

fn first_video_is_key(path: &Path) -> bool {
    unsafe {
        let name = CString::new(path.to_str().unwrap()).unwrap();
        let mut fmt = ptr::null_mut();
        assert!(ff::avformat_open_input(&mut fmt, name.as_ptr(), ptr::null(), ptr::null_mut()) >= 0);
        let mut pkt = ff::av_packet_alloc();
        let mut key = false;
        while ff::av_read_frame(fmt, pkt) >= 0 {
            let p = &*pkt;
            let is_video = p.stream_index == 0;
            key = p.flags & ff::AV_PKT_FLAG_KEY != 0;
            ff::av_packet_unref(pkt);
            if is_video {
                break;
            }
        }
        ff::av_packet_free(&mut pkt);
        ff::avformat_close_input(&mut fmt);
        key
    }
}

/// Feeds `secs` seconds through a session, sending `extra` messages at the
/// given times, interleaving audio the way PipeWire would (a bit late).
fn run_session(cfg: mux::Config, secs: u32, extra: Vec<(f64, Msg)>) -> anyhow::Result<mux::Stats> {
    let video = encode_video(secs).expect("no libx264 in this FFmpeg");
    let (tx, rx) = mpsc::channel();
    let session = std::thread::spawn(move || mux::run(cfg, rx));
    let mut extra = extra.into_iter().peekable();
    let mut audio_at = 0usize;
    for v in video {
        let t = v.pts_us as f64 / 1e6;
        while extra.peek().is_some_and(|(at, _)| *at <= t) {
            tx.send(extra.next().unwrap().1).unwrap();
        }
        // Audio shows up around 40 ms after it was heard.
        while (audio_at + CHUNK) as f64 / RATE as f64 <= t - 0.04 {
            let time_ns = audio_at as u64 * 1_000_000_000 / RATE as u64;
            tx.send(Msg::Audio(AudioChunk { source: Source::Game, time_ns, samples: tone(audio_at) })).unwrap();
            audio_at += CHUNK;
        }
        tx.send(Msg::Video(v)).unwrap();
    }
    for (_, m) in extra {
        tx.send(m).unwrap();
    }
    drop(tx);
    session.join().unwrap()
}

fn config(path: Option<PathBuf>, replay: Option<u32>) -> mux::Config {
    mux::Config {
        path,
        replay_secs: replay,
        announce: false,
        codec: Codec::H264,
        width: W,
        height: H,
        fps: FPS,
        audio_tracks: vec![TrackSpec { title: "Game", inputs: vec![Source::Game] }],
        audio_bitrate: 128_000,
        start_ns: 0,
    }
}

fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("framecorder-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

#[test]
fn plain_recording_covers_everything() {
    let dir = tempdir("plain");
    let path = dir.join("rec.mp4");
    let stats = run_session(config(Some(path.clone()), None), 5, Vec::new()).unwrap();
    assert_eq!(stats.video_frames, (5 * FPS) as u64);
    let d = probe(&path);
    assert_eq!(d.len(), 2);
    assert!(close(d[0], 5.0, 0.05), "video {d:?}");
    assert!(close(d[1], 5.0, 0.1), "audio {d:?}");
    assert!(!dir.join("rec.mp4.part").exists());
}

#[test]
fn recording_mid_session_starts_on_a_keyframe() {
    let dir = tempdir("mid");
    let path = dir.join("rec.mp4");
    // Keyframes every 2 s: asked for at 3.1 s it starts at 4 s, stopped at 9 s.
    let extra = vec![(3.1, Msg::Record(path.clone())), (9.0, Msg::StopRecord)];
    let stats = run_session(config(None, Some(10)), 12, extra).unwrap();
    assert_eq!(stats.video_frames, (5 * FPS) as u64);
    let d = probe(&path);
    assert!(close(d[0], 5.0, 0.05), "video {d:?}");
    assert!(close(d[1], 5.0, 0.1), "audio {d:?}");
    assert!(first_video_is_key(&path));
}

#[test]
fn clips_come_from_the_replay_buffer() {
    let dir = tempdir("clip");
    let clip = dir.join("clip.mp4");
    let early = dir.join("early.mp4");
    let extra = vec![
        // Before any video there's nothing to clip.
        (-1.0, Msg::Clip { secs: 5, path: early.clone() }),
        // 3 s back from 10 s is 7 s; the keyframe at or before it is at 6 s.
        (10.0, Msg::Clip { secs: 3, path: clip.clone() }),
    ];
    run_session(config(None, Some(5)), 12, extra).unwrap();
    assert!(!early.exists());
    let d = probe(&clip);
    assert!(close(d[0], 4.0, 0.05), "video {d:?}");
    assert!(close(d[1], 4.0, 0.1), "audio {d:?}");
    assert!(first_video_is_key(&clip));
    assert!(!dir.join("clip.mp4.part").exists());
}

#[test]
fn recording_turns_the_replay_buffer_off() {
    let dir = tempdir("both");
    let (rec, during, after) = (dir.join("rec.mp4"), dir.join("during.mp4"), dir.join("after.mp4"));
    let extra = vec![
        (0.0, Msg::Record(rec.clone())),
        // Recording: the buffer's off, the recording has it all.
        (4.0, Msg::Clip { secs: 30, path: during.clone() }),
        (5.0, Msg::StopRecord),
        // Back on after, filling up from where the recording stopped.
        (9.5, Msg::Clip { secs: 30, path: after.clone() }),
    ];
    run_session(config(None, Some(30)), 10, extra).unwrap();
    assert!(close(probe(&rec)[0], 5.0, 0.05));
    assert!(!during.exists());
    let d = probe(&after)[0];
    assert!(d > 3.0 && d < 5.0, "clip after recording is {d} s");
}

/// What the session costs at headset-like rates: 40 Mbit/s at 72 fps, game
/// audio and mic mixed into one track, with a replay buffer. Prints CPU time
/// per second of recording and the buffer's memory.
/// `cargo test --release session_cost -- --ignored --nocapture`
#[test]
#[ignore]
fn session_cost() {
    const FPS: u32 = 72;
    const SECS: u32 = 150;
    let frame_bytes = 40_000_000 / 8 / FPS as usize;
    for replay in [30, 60, 120] {
        let (tx, rx) = mpsc::channel();
        let cfg = mux::Config {
            path: None,
            replay_secs: Some(replay),
            announce: false,
            codec: Codec::Hevc,
            width: 1920,
            height: 1080,
            fps: FPS,
            audio_tracks: vec![TrackSpec { title: "Game + mic", inputs: vec![Source::Game, Source::Mic] }],
            audio_bitrate: 192_000,
            start_ns: 0,
        };
        let cpu = || {
            let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
            unsafe { libc::getrusage(libc::RUSAGE_THREAD, &mut ru) };
            ru.ru_utime.tv_sec as f64 + ru.ru_utime.tv_usec as f64 / 1e6 + ru.ru_stime.tv_sec as f64 + ru.ru_stime.tv_usec as f64 / 1e6
        };
        let session = std::thread::spawn(move || {
            let before = cpu();
            let stats = mux::run(cfg, rx);
            (cpu() - before, stats)
        });
        // A fake HEVC stream: parameter sets on keyframes, an IDR or trailing slice NAL, filler.
        let header = [0u8, 0, 0, 1, 0x40, 1, 0xaa, 0, 0, 0, 1, 0x42, 1, 0xbb, 0, 0, 0, 1, 0x44, 1, 0xcc];
        let mut audio_at = 0usize;
        for f in 0..(SECS * FPS) as u64 {
            let t = f as f64 / FPS as f64;
            let key = f % (FPS as u64 * 2) == 0;
            let mut data = Vec::with_capacity(frame_bytes + header.len());
            if key {
                data.extend_from_slice(&header);
            }
            data.extend_from_slice(&[0, 0, 0, 1, if key { 0x26 } else { 0x02 }, 1]);
            data.resize(frame_bytes, 0x55);
            while (audio_at + CHUNK) as f64 / RATE as f64 <= t - 0.04 {
                let time_ns = audio_at as u64 * 1_000_000_000 / RATE as u64;
                for source in [Source::Game, Source::Mic] {
                    tx.send(Msg::Audio(AudioChunk { source, time_ns, samples: tone(audio_at) })).unwrap();
                }
                audio_at += CHUNK;
            }
            tx.send(Msg::Video(VideoPacket { data, pts_us: f * 1_000_000 / FPS as u64, key })).unwrap();
        }
        drop(tx);
        let (secs, stats) = session.join().unwrap();
        stats.unwrap();
        let ring_mb = frame_bytes as f64 * FPS as f64 * (replay as f64 + 2.0) / 1e6;
        println!(
            "replay {replay:>3} s: session thread {:.2}% of a core ({:.1} ms cpu per recorded second), ring about {ring_mb:.0} MB",
            secs / SECS as f64 * 100.0,
            secs / SECS as f64 * 1000.0
        );
    }
}

#[test]
fn clips_cant_come_faster_than_one_a_second() {
    let dir = tempdir("twice");
    let (a, b, c) = (dir.join("a.mp4"), dir.join("b.mp4"), dir.join("c.mp4"));
    let extra = vec![
        (6.0, Msg::Clip { secs: 5, path: a.clone() }),
        // Half a second later: too soon.
        (6.5, Msg::Clip { secs: 5, path: b.clone() }),
        (7.6, Msg::Clip { secs: 5, path: c.clone() }),
    ];
    run_session(config(None, Some(5)), 14, extra).unwrap();
    assert!(a.exists());
    assert!(!b.exists() && !dir.join("b.mp4.part").exists());
    assert!(c.exists());
}

/// Resident memory of this process, MB.
fn rss_mb() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let kb: f64 = status.lines().find(|l| l.starts_with("VmRSS:")).unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    kb / 1024.0
}

/// How much memory a 30 s replay buffer adds while running.
/// `cargo test --release memory_use -- --ignored --nocapture --test-threads 1`
#[test]
#[ignore]
fn memory_use() {
    // 40 s at 40 Mbit/s through a 30 s replay buffer.
    let before = rss_mb();
    let (tx, rx) = mpsc::channel();
    let cfg = mux::Config {
        path: None,
        replay_secs: Some(30),
        announce: false,
        codec: Codec::Hevc,
        width: 1920,
        height: 1080,
        fps: 72,
        audio_tracks: vec![TrackSpec { title: "Game + mic", inputs: vec![Source::Game, Source::Mic] }],
        audio_bitrate: 192_000,
        start_ns: 0,
    };
    let session = std::thread::spawn(move || mux::run(cfg, rx));
    let frame_bytes = 40_000_000 / 8 / 72;
    let header = [0u8, 0, 0, 1, 0x40, 1, 0xaa, 0, 0, 0, 1, 0x42, 1, 0xbb, 0, 0, 0, 1, 0x44, 1, 0xcc];
    let mut peak = 0.0f64;
    for f in 0..40 * 72u64 {
        let key = f % 144 == 0;
        let mut data = if key { header.to_vec() } else { Vec::new() };
        data.extend_from_slice(&[0, 0, 0, 1, if key { 0x26 } else { 0x02 }, 1]);
        data.resize(frame_bytes, 0x55);
        tx.send(Msg::Video(VideoPacket { data, pts_us: f * 1_000_000 / 72, key })).unwrap();
        if f % 72 == 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            peak = peak.max(rss_mb() - before);
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    peak = peak.max(rss_mb() - before);
    drop(tx);
    session.join().unwrap().unwrap();
    println!("30 s replay buffer at 40 Mbit/s: {peak:.0} MB");
}
