//! `--demo <screen>`: made up frames and clips, to try the app (or screenshot
//! it) without a frame. nothing syncs and nothing's saved.

use std::path::PathBuf;

use framecorder_app_lib::core::api::UpdateStatus;
use framecorder_app_lib::core::discover::Found;
use framecorder_app_lib::core::engine::{Progress, State, Status};

use crate::app::{Batch, DesktopUpdate, FrameApp, Page};
use crate::selfupdate::Release;
use crate::sync::Clip;

pub const SCREENS: &[&str] = &["clips", "list", "syncing", "unreachable", "pair", "settings"];

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
            // named the way the frame names them
            name: at(d, h, m).format("%Y-%m-%d_%H-%M-%S.mp4").to_string(),
            is_clip,
            size,
            created: at(d, h, m).timestamp(),
            duration_s: Some(len),
            location: PathBuf::from("/nonexistent"),
        })
        .collect()
}

pub fn seed(app: &mut FrameApp, screen: &str) {
    app.clips = clips();
    app.statuses = vec![status(State::Connected, false)];
    match screen {
        "list" => app.grid = false,
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
            app.desktop = DesktopUpdate::Available(Release {
                version: "0.2.0".into(),
                page: "https://github.com/coah80/framecorder/releases".into(),
                asset: Some(("demo".into(), 0)),
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
        _ => {}
    }
}
