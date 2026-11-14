//! The one thing framecorder says about itself, and only with anonymous
//! stats on: once a day, which version this is, how many recordings and clips
//! were made since the last time, and how a few settings stand. No id, no
//! names, nothing from a video. The service on the other end keeps daily
//! totals only (`stats/` in this repo), so one headset can't be told from
//! another.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const URL: &str = "https://stats.coah80.com/v1/ping";
/// One at a time at the state file: the tab counting, and a ping finishing.
static FILE: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
pub enum Made {
    Recording,
    Clip,
}

#[derive(Debug, Default, PartialEq)]
struct State {
    /// The UTC day of the last ping that got through.
    last: String,
    /// Whether one ever has: the first is how new installs are counted.
    ever: bool,
    recordings: u64,
    clips: u64,
}

impl State {
    fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for (key, value) in text.lines().filter_map(|l| l.split_once('=')) {
            match key {
                "last" => s.last = value.to_owned(),
                "ever" => s.ever = value == "true",
                "recordings" => s.recordings = value.parse().unwrap_or(0),
                "clips" => s.clips = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        s
    }

    fn text(&self) -> String {
        format!("last={}\never={}\nrecordings={}\nclips={}\n", self.last, self.ever, self.recordings, self.clips)
    }
}

fn path() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(dir.join("framecorder/stats"))
}

fn load() -> State {
    path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| State::parse(&t)).unwrap_or_default()
}

fn save(state: &State) {
    let Some(path) = path() else { return };
    let tmp = path.with_extension("tmp");
    let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&tmp, state.text())).and_then(|_| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        log::debug!("couldn't keep the stats count: {e}");
    }
}

/// The UTC date `secs` after 1970 falls on, as 2026-11-14.
fn day(secs: u64) -> String {
    // civil-from-days, Howard Hinnant's
    let z = (secs / 86400) as i64 + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

fn today() -> String {
    day(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()))
}

/// One more recording or clip for the next ping to mention.
pub fn made(what: Made) {
    // the tab's own tests save recordings that never happened
    if cfg!(test) {
        return;
    }
    let _held = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load();
    match what {
        Made::Recording => state.recordings += 1,
        Made::Clip => state.clips += 1,
    }
    save(&state);
}

/// Everything a ping says. `settings` are the tab's, as the service names them.
fn body(state: &State, settings: &[(&str, &str)]) -> Value {
    let settings: serde_json::Map<String, Value> = settings.iter().map(|(k, v)| ((*k).to_owned(), json!(v))).collect();
    json!({
        "app": "headset",
        "version": env!("CARGO_PKG_VERSION"),
        "os": "steamos",
        "new": !state.ever,
        // the service takes up to 10000 of each
        "counts": {"recordings": state.recordings.min(10_000), "clips": state.clips.min(10_000)},
        "settings": settings,
    })
}

/// Whether today's ping is still to go.
pub fn due() -> bool {
    load().last != today()
}

/// Sends today's ping, off to the side on its own thread: a few hundred
/// bytes through curl, the same way a report goes. Whatever it mentioned
/// comes off the count once it's through; a ping that didn't make it changes
/// nothing, and tomorrow's carries the lot.
pub fn ping(settings: Vec<(&'static str, &'static str)>) {
    let spawned = std::thread::Builder::new().name("stats".into()).spawn(move || {
        let sent = load();
        let day = today();
        let url = std::env::var("FRAMECORDER_STATS_URL").unwrap_or_else(|_| URL.into());
        let child = Command::new("curl")
            .args(["--silent", "--fail", "--max-time", "15", "--connect-timeout", "5", "--proto", "=https", "-H", "Content-Type: application/json"])
            .args(["-A", concat!("framecorder-headset/", env!("CARGO_PKG_VERSION")), "--data-binary", &body(&sent, &settings).to_string(), &url])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !child.is_ok_and(|s| s.success()) {
            return;
        }
        let _held = FILE.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = load();
        state.last = day;
        state.ever = true;
        state.recordings = state.recordings.saturating_sub(sent.recordings);
        state.clips = state.clips.saturating_sub(sent.clips);
        save(&state);
    });
    if let Err(e) = spawned {
        log::debug!("couldn't start the stats ping: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ping_says_only_what_the_service_takes() {
        let state = State { last: "2026-11-13".into(), ever: false, recordings: 3, clips: 20_000 };
        // what the tab's default settings come to, with the panels unlocked
        let settings = [("shape", "wide"), ("eye", "left"), ("quality", "high"), ("fps", "auto"), ("codec", "hevc"), ("format", "mp4"), ("clips", "on"), ("mic", "on"), ("split", "on"), ("panels", "on")];
        assert_eq!(
            body(&state, &settings).to_string(),
            format!(
                r#"{{"app":"headset","counts":{{"clips":10000,"recordings":3}},"new":true,"os":"steamos","settings":{{"clips":"on","codec":"hevc","eye":"left","format":"mp4","fps":"auto","mic":"on","panels":"on","quality":"high","shape":"wide","split":"on"}},"version":"{}"}}"#,
                env!("CARGO_PKG_VERSION")
            )
        );
        assert_eq!(State::parse(&state.text()), state);
        assert_eq!(day(1_794_700_800 - 1), "2026-11-14");
        assert_eq!(day(951_782_400), "2000-02-29");
    }
}
