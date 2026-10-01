//! framecorder: records what the Steam Frame actually shows on its panels,
//! with as little impact on the headset as possible.
//!
//! The pipeline per frame: wait for vblank, look at which buffer the VR
//! compositor is scanning out, run one small compute pass that crops,
//! undistorts, scales and converts it to NV12, and hand that to the hardware
//! encoder. Pixels never touch the CPU.

mod aac;
mod audio;
mod clock;
mod control;
mod encoder;
mod gpu;
mod headset_view;
mod kms;
mod lut;
mod mix;
mod mux;
mod perf;
mod replay;
#[cfg(test)]
mod session_tests;
mod writer;


use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};

use crate::encoder::{Codec, Encoder};
use crate::gpu::Gpu;
use crate::kms::Kms;
use framecorder::openvr;

/// Encoder input slots. One being written, one being encoded, one spare.
const SLOTS: usize = 3;
const STATUS_EVERY: Duration = Duration::from_secs(5);
/// Longest we wait for the compositor to finish drawing a frame we're about to read.
const SCANOUT_WAIT: Duration = Duration::from_millis(4);

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum View {
    /// One eye with the lens distortion removed, like a normal flat video.
    Eye,
    /// Both panels exactly as scanned out, distortion included.
    Raw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Source {
    /// What the panels show, un-warped here. Costs the game nothing.
    Panel,
    /// SteamVR's headset view, like Steam's own recorder. SteamVR only
    /// renders it while someone records it, which costs the game about half
    /// a millisecond of GPU per frame. Fixed at 16:9 of the left eye on the
    /// Frame.
    Headset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Aspect {
    /// 16:9, 1920x1080
    #[value(name = "16:9")]
    Landscape,
    /// 1:1, 1440x1440
    #[value(name = "1:1")]
    Square,
    /// 9:16, 1080x1920
    #[value(name = "9:16")]
    Portrait,
}

impl Aspect {
    /// Default size for each shape, all about the same pixel count as 1080p.
    fn size(self) -> (u32, u32) {
        match self {
            Aspect::Landscape => (1920, 1080),
            Aspect::Square => (1440, 1440),
            Aspect::Portrait => (1080, 1920),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Eye {
    Left,
    Right,
}

#[derive(Parser, Debug)]
#[command(version, about = "Low overhead recorder for the Steam Frame's real headset view")]
struct Args {
    /// Output file, .mp4 or .mkv. Defaults to ~/Videos/framecorder/<time>.mp4,
    /// or no recording at all with --replay.
    output: Option<PathBuf>,

    /// Keep the last this many seconds in memory for clips.
    #[arg(long)]
    replay: Option<u32>,

    /// Take commands on stdin (pause, resume, record <file>, stop-record,
    /// clip [secs] [file], quit) and report saved files on stdout.
    #[arg(long)]
    control: bool,

    #[arg(long, value_enum, default_value_t = View::Eye)]
    view: View,

    /// Where the picture comes from in `eye` view.
    #[arg(long, value_enum, default_value_t = Source::Panel)]
    source: Source,

    /// Which eye to record in `eye` view.
    #[arg(long, value_enum, default_value_t = Eye::Left)]
    eye: Eye,

    /// Shape of the video in `eye` view.
    #[arg(long, value_enum, default_value_t = Aspect::Landscape)]
    aspect: Aspect,

    /// Horizontal field of view to keep in `eye` view, in degrees. By default
    /// it's the widest view the lens shows without black edges.
    #[arg(long)]
    fov: Option<f64>,

    /// Output width. Defaults to 1920 on the long side for the chosen aspect.
    #[arg(long)]
    width: Option<u32>,

    /// Output height. Defaults to 1920 on the long side for the chosen aspect.
    #[arg(long)]
    height: Option<u32>,

    /// Frames per second. Defaults to the panel's refresh rate, halved when
    /// that's above 90 Hz, so every recorded frame is evenly spaced.
    #[arg(long)]
    fps: Option<u32>,

    #[arg(long, value_enum, default_value_t = Codec::Hevc)]
    codec: Codec,

    /// Video bitrate in Mbit/s.
    #[arg(long, default_value_t = 40)]
    bitrate: u32,

    /// Constant quantizer instead of a bitrate (lower is better, ~18-24 is good).
    #[arg(long, conflicts_with = "bitrate")]
    qp: Option<u32>,

    /// Also record the microphone, as its own audio track.
    #[arg(long)]
    mic: bool,

    /// Don't record game audio.
    #[arg(long)]
    no_audio: bool,

    /// Audio bitrate per track in kbit/s.
    #[arg(long, default_value_t = 192)]
    audio_bitrate: u32,

    /// Stop after this many seconds.
    #[arg(long)]
    duration: Option<f64>,

    /// Never use the 4-tap filter, even when shrinking the picture. Slightly cheaper, more aliasing.
    #[arg(long)]
    no_supersample: bool,

    /// Plain bilinear instead of the sharper Catmull-Rom filter for the panel. Cheaper, softer.
    #[arg(long)]
    soft: bool,

    /// How the recorder's GPU work ranks against the game's. High by
    /// default: lower ones wait for the game to leave the GPU idle, which in
    /// a heavy game means missed frames, and reading the panel late enough
    /// that the compositor's already drawing the next frame into it.
    #[arg(long, value_enum, default_value_t = gpu::Priority::High)]
    gpu_priority: gpu::Priority,

    /// Also keep the mic on its own track, for editing.
    #[arg(long)]
    mic_track: bool,

    /// Start paused; SIGUSR2 resumes, SIGUSR1 pauses again. Paused time is
    /// cut out of the video.
    #[arg(long)]
    start_paused: bool,

    /// Don't write the <video>.perf.csv with CPU, GPU and memory use.
    #[arg(long)]
    no_perf_log: bool,

    #[arg(long, default_value = "/dev/dri/card0")]
    drm_device: PathBuf,
}

static STOP: AtomicBool = AtomicBool::new(false);
static PAUSED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(sig: libc::c_int) {
    match sig {
        libc::SIGUSR1 => PAUSED.store(true, Ordering::SeqCst),
        libc::SIGUSR2 => PAUSED.store(false, Ordering::SeqCst),
        _ => STOP.store(true, Ordering::SeqCst),
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_target(false)
        .init();
    if let Err(e) = run(Args::parse()) {
        log::error!("{e:#}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<()> {
    // First thing, so a pause or stop that arrives during startup can't kill us.
    PAUSED.store(args.start_paused, Ordering::SeqCst);
    unsafe {
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGUSR1, libc::SIGUSR2] {
            libc::signal(sig, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
        }
        // The dashboard tab starts us with pause/resume blocked so they can't
        // kill us before the handlers exist. Let them through now.
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGUSR1);
        libc::sigaddset(&mut set, libc::SIGUSR2);
        libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
    }
    validate(&args)?;
    let (width, height) = output_size(&args);
    // With a replay buffer there's only a recording when one is asked for.
    let output = match (&args.output, args.replay) {
        (Some(p), _) => Some(p.clone()),
        (None, Some(_)) => None,
        (None, None) => Some(default_output("")?),
    };
    if let Some(p) = &output {
        create_parent(p)?;
    }

    let kms = Kms::open(&args.drm_device)?;
    let fps = args.fps.unwrap_or_else(|| auto_fps(kms.refresh_hz));
    log::info!("recording at {fps} fps");
    // SteamVR's headset view is fixed on the Frame (1920x1080 of the left eye).
    let hv_shape = if args.view == View::Eye && args.source == Source::Headset && openvr::OpenVr::server_running() {
        openvr::OpenVr::connect().ok().and_then(|vr| headset_view::current(&vr).ok())
    } else {
        None
    };
    let headset = match (args.source, args.view) {
        (Source::Headset, View::Raw) => bail!("the headset view is one eye, use --view eye"),
        (Source::Headset, View::Eye) => {
            let Some(((w, h), eye)) = hv_shape else { bail!("SteamVR's headset view isn't available") };
            if eye != args.eye as usize {
                log::warn!("SteamVR's headset view shows the other eye, recording that one");
            }
            if (w as f64 / h as f64 - width as f64 / height as f64).abs() > 0.01 {
                log::warn!("the headset view is {w}x{h}, it'll be letterboxed into {width}x{height}");
            }
            true
        }
        (Source::Panel, _) => false,
    };
    if headset && args.fov.is_some() {
        log::warn!("--fov only applies to --source panel, the headset view's framing is set by SteamVR");
    }
    // SteamVR's connection has to outlive the headset view and go before the
    // GPU device, since SteamVR keeps objects on our device.
    let vr = if headset {
        Some(openvr::OpenVr::connect_as(openvr::AppType::Overlay).context("connecting to SteamVR")?)
    } else {
        None
    };
    log::info!("source: {}", if headset { "SteamVR headset view" } else { "panel scanout" });
    let lut = match (headset, args.view) {
        (true, _) => {
            let (w, h) = hv_shape.map_or((width, height), |s| s.0);
            lut::flat(width, height, w, h)
        }
        (false, View::Raw) => lut::raw(width, height, kms.mode_size.0, kms.mode_size.1),
        (false, View::Eye) => {
            let vr = openvr::OpenVr::connect().context("asking SteamVR for the lens distortion (try --view raw)")?;
            lut::undistorted(&vr, args.eye as usize, args.fov, width, height)?
        }
    };
    let extensions = vr.as_ref().map(headset_view::vulkan_extensions).transpose()?;

    let enc_cfg = encoder::Config {
        codec: args.codec,
        width,
        height,
        fps,
        bitrate: args.bitrate * 1_000_000,
        qp: args.qp,
    };
    let mut enc = Encoder::open(&encoder::find_device()?, enc_cfg, SLOTS as u32)?;
    // The panel has already been resampled once by the compositor, so read
    // it with a sharp filter; the headset view is already the right size.
    let filter = if lut.supersample && !args.no_supersample {
        gpu::Filter::Supersample
    } else if !headset && !args.soft {
        gpu::Filter::Sharp
    } else {
        gpu::Filter::Bilinear
    };
    let mut gpu = Gpu::new(enc.layout(), SLOTS, &lut, filter, args.gpu_priority, extensions.as_ref())?;
    drop(lut);
    let mut view = match &vr {
        Some(vr) => Some(headset_view::HeadsetView::new(vr, &gpu.native())?),
        None => None,
    };

    // Tracks the file gets. The mic goes into the main track so every
    // player plays it; a separate mic track costs another encoder, so it's
    // opt-in.
    let tracks = audio_tracks(!args.no_audio, args.mic, args.mic_track);
    let mut sources = Vec::new();
    if !args.no_audio {
        sources.push(audio::Source::Game);
    }
    if args.mic {
        sources.push(audio::Source::Mic);
    }

    let start = clock::now();
    let counters = Arc::new(perf::Counters::default());
    let perf_log = if args.no_perf_log {
        None
    } else {
        let base = output.clone().map_or_else(session_log_base, Ok)?;
        match perf::start(&base, counters.clone()) {
            Ok(p) => Some(p),
            Err(e) => {
                log::warn!("not logging performance: {e:#}");
                None
            }
        }
    };
    let (tx, rx) = mpsc::channel();
    let mux_cfg = mux::Config {
        path: output,
        replay_secs: args.replay,
        announce: args.control,
        codec: args.codec,
        width,
        height,
        fps,
        audio_tracks: tracks,
        audio_bitrate: args.audio_bitrate * 1000,
        start_ns: start.as_nanos() as u64,
    };
    let muxer = std::thread::Builder::new()
        .name("mux".into())
        .spawn(move || mux::run(mux_cfg, rx))?;

    let (ctl_tx, commands) = mpsc::channel();
    let clip_secs = args.replay.unwrap_or(DEFAULT_CLIP_SECS);
    let capture = if sources.is_empty() {
        None
    } else {
        match audio::Capture::start(sources, tx.clone()) {
            Ok(c) => Some(c),
            Err(e) => {
                drop(tx);
                let _ = muxer.join();
                return Err(e).context("starting audio capture");
            }
        }
    };
    log::info!("recording, ctrl+c to stop");

    if args.control {
        spawn_control(ctl_tx, clip_secs)?;
    } else {
        drop(ctl_tx);
    }
    let result = capture_loop(&args, fps, &kms, view.as_mut(), &mut gpu, &mut enc, start, (&tx, &commands), &counters);
    // Hand SteamVR's settings back and disconnect while our device exists.
    drop(view);
    drop(vr);
    if let Some(p) = perf_log {
        let csv = p.path.clone();
        log::info!("{}", p.finish().line());
        log::info!("performance log: {}", csv.display());
    }

    if let Some(c) = capture {
        c.stop();
    }
    let send = |p: encoder::Packet| {
        let _ = tx.send(mux::Msg::Video(mux::VideoPacket { data: p.data.to_vec(), pts_us: p.pts_us, key: p.key }));
    };
    if let Err(e) = enc.finish(send) {
        log::warn!("draining the encoder failed: {e:#}");
    }
    drop(tx);

    let stats = muxer.join().map_err(|_| anyhow::anyhow!("the muxer thread crashed"))?;
    result?;
    let stats = stats?;
    log::info!(
        "done: {} frames, {:.1} MB",
        stats.video_frames,
        stats.bytes as f64 / 1e6
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn capture_loop(
    args: &Args,
    fps: u32,
    kms: &Kms,
    mut view: Option<&mut headset_view::HeadsetView>,
    gpu: &mut Gpu,
    enc: &mut Encoder,
    start: Duration,
    (tx, commands): (&mpsc::Sender<mux::Msg>, &mpsc::Receiver<mux::Msg>),
    counters: &perf::Counters,
) -> Result<()> {
    let interval = Duration::from_secs_f64(1.0 / fps as f64);
    let tolerance = Duration::from_secs_f64(0.5 / kms.refresh_hz.max(1.0));
    let ratio = kms.refresh_hz / fps as f64;
    if ratio < 0.99 {
        log::warn!(
            "the display runs at {:.0} Hz, so the video can't really have {fps} fps",
            kms.refresh_hz
        );
    } else if (ratio - ratio.round()).abs() > 0.05 {
        log::warn!(
            "{fps} fps doesn't divide {:.0} Hz evenly, motion won't be perfectly smooth",
            kms.refresh_hz
        );
    }

    let deadline = args.duration.map(|s| Instant::now() + Duration::from_secs_f64(s));
    let mut due = Duration::ZERO;
    let (mut captured, mut dropped, mut skipped, mut idle) = (0u64, 0u64, 0u64, 0u64);
    let mut last_status = Instant::now();
    let mut window = (0u64, 0u64);
    let mut gpu_time = Duration::ZERO;
    // Time spent paused so far, and when the current pause began.
    let mut paused_total = Duration::ZERO;
    let mut paused_since = PAUSED.load(Ordering::SeqCst).then_some(start);
    if paused_since.is_some() {
        send(tx, mux::Msg::Pause(start.as_nanos() as u64))?;
        log::info!("paused");
    }

    while !STOP.load(Ordering::Relaxed) {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break;
        }
        while let Ok(cmd) = commands.try_recv() {
            if matches!(cmd, mux::Msg::Record(_)) {
                enc.force_keyframe();
            }
            send(tx, cmd)?;
        }
        let vblank = match kms.wait_vblank() {
            Ok(t) => t,
            Err(_) if STOP.load(Ordering::Relaxed) => break,
            Err(e) => {
                // The panels go dark when the headset is taken off, and the
                // compositor may come back with new buffers.
                idle += 1;
                if idle % 20 == 1 {
                    log::warn!("{e:#}, is the display off?");
                }
                gpu.clear_sources();
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
        };
        forward_packets(enc, tx)?;

        match (PAUSED.load(Ordering::Relaxed), paused_since) {
            (true, None) => {
                paused_since = Some(vblank);
                send(tx, mux::Msg::Pause(vblank.as_nanos() as u64))?;
                log::info!("paused");
            }
            (false, Some(since)) => {
                paused_total += vblank.saturating_sub(since);
                paused_since = None;
                send(tx, mux::Msg::Resume(vblank.as_nanos() as u64))?;
                // Pick up right away rather than on the old frame grid.
                due = vblank.saturating_sub(start).saturating_sub(paused_total);
                window = (captured, dropped);
                last_status = Instant::now();
                gpu_time = Duration::ZERO;
                log::info!("resumed");
            }
            _ => {}
        }
        if paused_since.is_some() {
            continue;
        }

        let now = vblank.saturating_sub(start).saturating_sub(paused_total);
        if now + tolerance < due {
            continue;
        }
        while due <= now + tolerance {
            due += interval;
        }

        let grabbed = match view.as_deref_mut() {
            Some(v) => capture_headset(v, gpu, enc, now),
            None => capture_frame(kms, gpu, enc, now),
        };
        match grabbed {
            Ok(Some(t)) => {
                gpu_time += t;
                captured += 1;
                counters.frames.fetch_add(1, Ordering::Relaxed);
                counters.shader_ns.fetch_add(t.as_nanos() as u64, Ordering::Relaxed);
            }
            Ok(None) => {
                dropped += 1;
                counters.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                // The compositor rebuilds its buffers now and then, which can
                // make a frame or two fail. Skip them and start fresh.
                skipped += 1;
                if skipped % 50 == 1 {
                    log::warn!("skipping a frame: {e:#}");
                }
                gpu.clear_sources();
            }
        }

        if last_status.elapsed() >= STATUS_EVERY {
            let secs = last_status.elapsed().as_secs_f64();
            let frames = captured - window.0;
            log::info!(
                "{:.1} fps, {} frames, {} dropped, {:.2} ms gpu per frame",
                frames as f64 / secs,
                captured,
                dropped,
                gpu_time.as_secs_f64() * 1000.0 / frames.max(1) as f64
            );
            gpu_time = Duration::ZERO;
            if dropped > window.1 {
                log::warn!("the encoder fell behind, try a lower bitrate or resolution");
            }
            window = (captured, dropped);
            last_status = Instant::now();
        }
    }
    forward_packets(enc, tx)?;
    Ok(())
}

/// Grabs whatever the panel shows right now into the encoder. None when the
/// encoder is still busy with earlier frames.
fn capture_frame(kms: &Kms, gpu: &mut Gpu, enc: &mut Encoder, now: Duration) -> Result<Option<Duration>> {
    let Some(fb) = kms.current_fb()? else { return Ok(None) };
    // Exported every frame: framebuffer ids get recycled when the compositor
    // rebuilds its swapchain, the buffer behind them is what identifies them.
    let buf = kms.export(fb)?;
    kms::wait_idle(&buf.fd, SCANOUT_WAIT);
    let id = kms::buffer_id(&buf.fd)?;
    if !gpu.has_source(fb, id) {
        gpu.add_source(buf, id)?;
    }
    let Some(slot) = enc.free_slot()? else { return Ok(None) };
    let t = gpu.convert(fb, id, slot)?;
    enc.queue_frame(slot, gpu.targets()[slot].fd.as_raw_fd(), now.as_micros() as u64)?;
    Ok(Some(t))
}

/// Grabs SteamVR's newest headset view frame into the encoder.
fn capture_headset(view: &mut headset_view::HeadsetView, gpu: &mut Gpu, enc: &mut Encoder, now: Duration) -> Result<Option<Duration>> {
    let Some(frame) = view.acquire()? else { return Ok(None) };
    if gpu.copy_size() != Some((frame.width, frame.height)) {
        gpu.add_copy_source(frame.width, frame.height)?;
    }
    let Some(slot) = enc.free_slot()? else { return Ok(None) };
    let t = gpu.convert_copied(frame.image, frame.origin, slot)?;
    enc.queue_frame(slot, gpu.targets()[slot].fd.as_raw_fd(), now.as_micros() as u64)?;
    Ok(Some(t))
}

fn send(tx: &mpsc::Sender<mux::Msg>, msg: mux::Msg) -> Result<()> {
    tx.send(msg).map_err(|_| anyhow::anyhow!("the file writer stopped"))
}

fn forward_packets(enc: &mut Encoder, tx: &mpsc::Sender<mux::Msg>) -> Result<()> {
    let mut alive = true;
    enc.poll_packets(|p| {
        let packet = mux::VideoPacket { data: p.data.to_vec(), pts_us: p.pts_us, key: p.key };
        alive &= tx.send(mux::Msg::Video(packet)).is_ok();
    })?;
    if !alive {
        anyhow::bail!("the file writer stopped");
    }
    Ok(())
}

fn audio_tracks(game: bool, mic: bool, mic_track: bool) -> Vec<mux::TrackSpec> {
    use audio::Source::{Game, Mic};
    let track = |title, inputs| mux::TrackSpec { title, inputs };
    match (game, mic) {
        (true, true) if mic_track => vec![track("Game + mic", vec![Game, Mic]), track("Microphone", vec![Mic])],
        (true, true) => vec![track("Game + mic", vec![Game, Mic])],
        (true, false) => vec![track("Game", vec![Game])],
        (false, true) => vec![track("Microphone", vec![Mic])],
        (false, false) => Vec::new(),
    }
}

/// Highest evenly spaced frame rate that stays sensible for video: the
/// refresh rate itself up to 90 Hz, every other refresh above that.
fn auto_fps(refresh_hz: f64) -> u32 {
    let hz = refresh_hz.round().max(1.0) as u32;
    if hz > 90 {
        hz / 2
    } else {
        hz
    }
}

/// Explicit sizes win; a single one keeps the aspect ratio.
fn output_size(args: &Args) -> (u32, u32) {
    let (w, h) = args.aspect.size();
    match (args.width, args.height) {
        (Some(w), Some(h)) => (w, h),
        (Some(nw), None) => (nw, (nw as u64 * h as u64 / w as u64) as u32 / 8 * 8),
        (None, Some(nh)) => ((nh as u64 * w as u64 / h as u64) as u32 / 8 * 8, nh),
        (None, None) => (w, h),
    }
}

fn validate(args: &Args) -> Result<()> {
    let (width, height) = output_size(args);
    if width % 8 != 0 || height % 8 != 0 {
        bail!("width and height must be multiples of 8");
    }
    if !(64..=4096).contains(&width) || !(64..=4096).contains(&height) {
        bail!("resolution must be between 64 and 4096 on each side");
    }
    if args.fps.is_some_and(|f| !(1..=144).contains(&f)) {
        bail!("fps must be between 1 and 144");
    }
    if !(1..=240).contains(&args.bitrate) {
        bail!("bitrate must be between 1 and 240 Mbit/s");
    }
    if args.qp.is_some_and(|q| !(1..=51).contains(&q)) {
        bail!("qp must be between 1 and 51");
    }
    if args.fov.is_some_and(|f| !(10.0..=150.0).contains(&f)) {
        bail!("fov must be between 10 and 150 degrees");
    }
    if !(32..=512).contains(&args.audio_bitrate) {
        bail!("audio bitrate must be between 32 and 512 kbit/s");
    }
    if args.replay.is_some_and(|s| !(5..=300).contains(&s)) {
        bail!("replay must be between 5 and 300 seconds");
    }
    Ok(())
}

/// Clip length when the command doesn't say.
const DEFAULT_CLIP_SECS: u32 = 30;

/// Reads commands from the dashboard tab. Pausing goes straight to the
/// capture loop's flag, the rest gets passed along through `ctl`.
fn spawn_control(ctl: mpsc::Sender<mux::Msg>, default_secs: u32) -> Result<()> {
    std::thread::Builder::new().name("control".into()).spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { break };
            let msg = match control::parse(&line) {
                Ok(control::Command::Pause) => {
                    PAUSED.store(true, Ordering::SeqCst);
                    continue;
                }
                Ok(control::Command::Resume) => {
                    PAUSED.store(false, Ordering::SeqCst);
                    continue;
                }
                Ok(control::Command::Quit) => break,
                Ok(control::Command::Record(path)) => create_parent(&path).map(|_| mux::Msg::Record(path)),
                Ok(control::Command::StopRecord) => Ok(mux::Msg::StopRecord),
                Ok(control::Command::Clip { secs, path }) => path
                    .map_or_else(|| default_output("clips"), Ok)
                    .and_then(|p| create_parent(&p).map(|_| p))
                    .map(|path| mux::Msg::Clip { secs: secs.unwrap_or(default_secs), path }),
                Err(e) => Err(anyhow::anyhow!(e)),
            };
            match msg {
                Ok(m) => {
                    if ctl.send(m).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    log::warn!("{e:#}");
                    println!("failed command {e:#}");
                }
            }
        }
        // The tab went away (or said so): wrap up.
        STOP.store(true, Ordering::SeqCst);
    })?;
    Ok(())
}

fn create_parent(path: &std::path::Path) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    Ok(())
}

/// Where the performance log goes when there's no recording to put it next to.
fn session_log_base() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME isn't set")?;
    let path = PathBuf::from(home).join(".local/state/framecorder").join(timestamp_name());
    create_parent(&path)?;
    Ok(path)
}

/// ~/Videos/framecorder/<sub>/<time>.mp4, with -2, -3... on the end if
/// something from the same second is already there.
fn default_output(sub: &str) -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME isn't set, pass an output path")?;
    let dir = PathBuf::from(home).join("Videos/framecorder").join(sub);
    Ok(unique_path(&dir, &timestamp_name()))
}

fn unique_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let stem = name.trim_end_matches(".mp4");
    let taken = |p: &PathBuf| p.exists() || p.with_extension("mp4.part").exists();
    let mut path = dir.join(name);
    let mut n = 2;
    while taken(&path) {
        path = dir.join(format!("{stem}-{n}.mp4"));
        n += 1;
    }
    path
}

fn timestamp_name() -> String {
    let mut t: libc::tm = unsafe { std::mem::zeroed() };
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    unsafe { libc::localtime_r(&now, &mut t) };
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}.mp4",
        t.tm_year + 1900,
        t.tm_mon + 1,
        t.tm_mday,
        t.tm_hour,
        t.tm_min,
        t.tm_sec
    )
}
