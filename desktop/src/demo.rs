//! `--demo <screen>`: made up frames and clips, to try the app (or screenshot
//! it) without a frame. nothing syncs and nothing's saved.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use framecorder_app_lib::core::api::{About, Battery, Mic, Recording, Remote, Storage, UpdateStatus};
use framecorder_app_lib::core::discover::Found;
use framecorder_app_lib::core::engine::{Progress, State, Status};
use framecorder_app_lib::core::live::{LivePhase, LiveStatus};
use framecorder_app_lib::core::report::Sent;

use crate::app::{Batch, DesktopUpdate, FrameApp, Page, Speed};
use crate::check::{Check, Outcome, Step};
use crate::preview::{self, Note};
use crate::remote::Live;
use crate::report::Phase;
use crate::selfupdate::Release;
use crate::sync::Clip;

pub const SCREENS: &[&str] = &[
    "clips",
    "list",
    "clips-selecting",
    "list-selecting",
    "syncing",
    "unreachable",
    "pair",
    "settings",
    "frame",
    "frame-recording",
    "frame-paused",
    "frame-old",
    "frame-closed",
    "frame-unreachable",
    "frame-syncing",
    "frame-eye",
    "frame-blended",
    "frame-no-blend",
    "frame-locked",
    "frame-mics",
    "check",
    "live",
    "live-connecting",
    "live-on",
    "live-fullscreen",
    "live-error",
    "live-old",
    "live-no-ffmpeg",
    "report",
    "report-sending",
    "report-sent",
    "report-error",
];

fn status(state: State, update: bool) -> Status {
    Status {
        fingerprint: "demo".into(),
        name: "cole's frame".into(),
        addr: "192.168.1.42:38619".into(),
        state,
        message: (state == State::Unreachable).then(|| "trying again in 12 s, last seen at 192.168.1.42".into()),
        update: Some(UpdateStatus {
            installed: "0.6.2".into(),
            latest: Some("0.7.0".into()),
            available: update,
            updating: false,
        }),
    }
}

fn clips() -> Vec<Clip> {
    let now = chrono::Local::now();
    let today = now.date_naive();
    let at = |days_ago: i64, h: u32, m: u32| {
        let d = today - chrono::Duration::days(days_ago);
        d.and_hms_opt(h, m, 0).unwrap().and_local_timezone(chrono::Local).unwrap()
    };
    let rows: [(i64, u32, u32, bool, f64, u64); 9] = [
        (0, 15, 42, true, 30., 48_000_000),
        (0, 14, 15, true, 60., 96_000_000),
        (0, 11, 8, false, 761., 1_200_000_000),
        (0, 10, 52, true, 30., 51_000_000),
        (1, 21, 31, false, 2042., 3_100_000_000),
        (1, 20, 47, true, 45., 72_000_000),
        (1, 20, 12, true, 30., 47_000_000),
        (3, 18, 20, false, 495., 790_000_000),
        (3, 18, 2, true, 30., 49_000_000),
    ];
    rows.iter()
        .enumerate()
        .map(|(i, &(d, h, m, is_clip, len, size))| Clip {
            key: format!("demo/{i}"),
            host: "demo".into(),
            // named the way the frame names them
            name: at(d, h, m).format("%Y-%m-%d_%H-%M-%S.mp4").to_string(),
            is_clip,
            size,
            created: at(d, h, m).timestamp(),
            duration_s: Some(len),
            // landed a few minutes after it was saved
            synced_at: at(d, h, m).timestamp() + 180,
            location: place(i, &at(d, h, m).format("%Y-%m-%d_%H-%M-%S.mp4").to_string()),
        })
        .collect()
}

/// a real little video behind every made up clip, so open, reveal, copy and
/// the thumbnails behave like they do on real ones
const FILES: [&[u8]; 9] = [
    include_bytes!("../assets/demo/clip0.mp4"),
    include_bytes!("../assets/demo/clip1.mp4"),
    include_bytes!("../assets/demo/clip2.mp4"),
    include_bytes!("../assets/demo/clip3.mp4"),
    include_bytes!("../assets/demo/clip4.mp4"),
    include_bytes!("../assets/demo/clip5.mp4"),
    include_bytes!("../assets/demo/clip6.mp4"),
    include_bytes!("../assets/demo/clip7.mp4"),
    include_bytes!("../assets/demo/clip8.mp4"),
];

fn place(i: usize, name: &str) -> PathBuf {
    let path = std::env::temp_dir().join("framecorder-demo").join("clips").join(name);
    let written = std::fs::create_dir_all(path.parent().unwrap_or(&path)).and_then(|()| std::fs::write(&path, FILES[i % FILES.len()]));
    if let Err(e) = written {
        log::warn!("couldn't write the demo clip {}: {e}", path.display());
    }
    path
}

/// a frame with its tab up and a 30 s clip buffer going
fn remote() -> Remote {
    Remote { available: true, ready: true, recording: false, running: false, elapsed_ms: 0, clips: Some(30), clip_ready: true, live_viewers: Some(0) }
}

fn recording() -> Recording {
    Recording {
        shape: "wide".into(),
        quality: "high".into(),
        fps: "auto".into(),
        game_audio: true,
        mic: true,
        clips: true,
        clip: 30,
        eye: Some("left".into()),
        panels: Some(true),
        split_audio: Some(true),
        mic_source: Some(String::new()),
        codec: Some("hevc".into()),
        mics: Some(vec![
            Mic { name: "alsa_loopback_device.alsa_input.platform-sound.HiFi__Mic__source".into(), label: "Frame".into(), default: true },
            Mic { name: "alsa_input.usb-Blue_Yeti-00.analog-stereo".into(), label: "Yeti Stereo Microphone".into(), default: false },
        ]),
    }
}

fn live(remote: Option<Remote>) -> Live {
    Live {
        remote_at: remote.as_ref().map(|_| Instant::now()),
        remote,
        about: Some(About {
            battery: Some(Battery { percent: 72, charging: true }),
            storage: Some(Storage { free: 48_000_000_000, total: 128_000_000_000, videos: 12_400_000_000 }),
        }),
        recording: Some(recording()),
        asked: true,
        ..Default::default()
    }
}

/// the stream's state as the core would report it
pub fn live_status(phase: LivePhase) -> LiveStatus {
    let on = phase == LivePhase::Live;
    LiveStatus {
        phase: phase.clone(),
        url: format!("http://127.0.0.1:{}/live.ts", framecorder_app_lib::core::live::DEFAULT_PORT),
        bytes_per_second: if on { 5_100_000 } else { 0 },
        viewers: if on { 2 } else { 0 },
        error: (phase == LivePhase::Error).then(|| "the headset needs an update to show live view".into()),
    }
}

const WENT_WRONG: &str = "the clip i saved at 15:42 is tilted, the horizon leans to the left. 16:9 and the left eye, playing walkabout mini golf.";

fn seed_live(app: &mut FrameApp, phase: Option<LivePhase>, picture: bool) {
    app.page = Page::Live;
    app.ffmpeg = true;
    app.live.insert("demo".into(), live(Some(Remote { live_viewers: Some(1), ..remote() })));
    if let Some(phase) = phase {
        app.live_view.frame = Some("demo".into());
        app.live_view.demo_status = Some(live_status(phase));
    }
    if picture {
        app.live_view.picture = Some(preview::demo_frame());
        app.live_view.note = Note::Showing;
    }
}

fn seed_report(app: &mut FrameApp, phase: Phase) {
    app.page = Page::Report;
    app.report.description = WENT_WRONG.into();
    app.report.phase = phase;
}

pub fn seed(app: &mut FrameApp, screen: &str) {
    app.clips = clips();
    app.statuses = vec![status(State::Connected, false)];
    if screen.starts_with("frame") {
        app.page = Page::Frame;
        app.live.insert("demo".into(), live(Some(remote())));
    }
    if screen.ends_with("-selecting") {
        app.selection.picked = ["demo/0", "demo/1", "demo/3"].map(String::from).into();
        app.selection.anchor = Some("demo/3".into());
    }
    match screen {
        "list" | "list-selecting" => app.grid = false,
        "frame-mics" => app.mic_menu = true,
        "syncing" => {
            app.statuses = vec![status(State::Connected, true)];
            app.frame_updating.insert("demo".into());
            app.progress = Some(Progress {
                fingerprint: "demo".into(),
                id: "incoming".into(),
                name: "clip.mp4".into(),
                done: 31_000_000,
                total: 48_000_000,
                queued: 2,
            });
            app.batch = Some(Batch { total: 3, done: 0 });
            // two samples a second apart, so there's a rate to show
            let t = Instant::now();
            app.speed = Some(Speed::after(Some(Speed::after(None, "incoming", 26_800_000, t)), "incoming", 31_000_000, t + Duration::from_secs(1)));
            app.desktop = DesktopUpdate::Available(Release {
                version: "0.2.0".into(),
                url: "demo".into(),
                size: 0,
            });
        }
        "unreachable" => {
            app.statuses = vec![status(State::Unreachable, false)];
            app.clips.truncate(4);
        }
        "pair" => {
            app.page = Page::Pair;
            app.pairing.found = vec![
                Found {
                    name: "cole's frame".into(),
                    fingerprint: "a".into(),
                    addr: "192.168.1.42:38619".into(),
                    addrs: vec![],
                },
                Found {
                    name: "living room frame".into(),
                    fingerprint: "b".into(),
                    addr: "192.168.1.57:38619".into(),
                    addrs: vec![],
                },
            ];
            app.pairing.selected = Some("a".into());
            app.pairing.code = "481".into();
        }
        "settings" => app.page = Page::Settings,
        "frame-recording" => {
            let rec = Remote { recording: true, running: true, elapsed_ms: 222_000, clip_ready: false, ..remote() };
            app.live.insert("demo".into(), live(Some(rec)));
            let mut other = status(State::Connecting, false);
            other.fingerprint = "demo2".into();
            other.name = "living room frame".into();
            other.addr = "192.168.1.57:38619".into();
            app.statuses.push(other);
        }
        "frame-paused" => {
            let rec = Remote { recording: true, running: false, elapsed_ms: 222_000, clip_ready: false, ..remote() };
            app.live.insert("demo".into(), live(Some(rec)));
        }
        "frame-old" => {
            app.live.insert("demo".into(), Live { asked: true, ..Default::default() });
        }
        "frame-closed" => {
            let closed = Remote { available: false, ready: false, clips: None, clip_ready: false, ..remote() };
            app.live.insert("demo".into(), live(Some(closed)));
        }
        "frame-unreachable" => {
            app.statuses = vec![status(State::Unreachable, false)];
            app.live.insert("demo".into(), live(None));
        }
        "frame-syncing" => {
            app.progress = Some(Progress {
                fingerprint: "demo".into(),
                id: "incoming".into(),
                name: "clip.mp4".into(),
                done: 31_000_000,
                total: 48_000_000,
                queued: 0,
            });
        }
        // set to the right eye, with the panels unlocked
        "frame-eye" => {
            let rec = Recording { eye: Some("right".into()), ..recording() };
            app.live.insert("demo".into(), Live { recording: Some(rec), ..live(Some(remote())) });
        }
        // both eyes blended into one picture
        "frame-blended" => {
            let rec = Recording { eye: Some("both".into()), ..recording() };
            app.live.insert("demo".into(), Live { recording: Some(rec), ..live(Some(remote())) });
        }
        // a frame too old to blend: it doesn't say who's watching, so the choice is greyed
        "frame-no-blend" => {
            let old = Remote { live_viewers: None, ..remote() };
            app.live.insert("demo".into(), live(Some(old)));
        }
        "live" => seed_live(app, None, false),
        "live-connecting" => seed_live(app, Some(LivePhase::Connecting), false),
        "live-on" => seed_live(app, Some(LivePhase::Live), true),
        "live-fullscreen" => {
            seed_live(app, Some(LivePhase::Live), true);
            app.live_view.full = true;
        }
        "live-error" => seed_live(app, Some(LivePhase::Error), false),
        "live-old" => {
            seed_live(app, None, false);
            app.live.insert("demo".into(), live(Some(Remote { live_viewers: None, ..remote() })));
        }
        "live-no-ffmpeg" => {
            seed_live(app, Some(LivePhase::Live), false);
            app.ffmpeg = false;
        }
        "report" => seed_report(app, Phase::Writing),
        "report-sending" => seed_report(app, Phase::Sending),
        "report-sent" => seed_report(app, Phase::Sent(Sent { number: 128, url: "https://github.com/coah80/framecorder/issues/128".into() })),
        "report-error" => seed_report(app, Phase::Failed("couldn't send the report; check your internet connection and try again".into())),
        // a frame whose panels aren't unlocked: a square was picked, but it records steamvr's view
        "frame-locked" => {
            let rec = Recording { shape: "square".into(), panels: Some(false), ..recording() };
            app.live.insert("demo".into(), Live { recording: Some(rec), ..live(Some(remote())) });
        }
        "check" => {
            app.statuses = vec![status(State::Unreachable, false)];
            app.page = Page::Check;
            let step = |title: &str, detail: &str, outcome| Step { title: title.into(), detail: detail.into(), outcome };
            app.check = Some(Check {
                fingerprint: "demo".into(),
                steps: vec![
                    step("this computer is on a network", "192.168.1.x", Outcome::Pass),
                    step("answers at 192.168.1.42", "can't reach the frame (connection refused)", Outcome::Fail),
                    step("found by name", "no answer. some routers and firewalls block this", Outcome::Fail),
                    step("found by asking every address", "checking 192.168.1.x...", Outcome::Running),
                ],
                running: true,
                task: None,
            });
        }
        _ => {}
    }
}
