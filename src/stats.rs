//! The one thing framecorder says about itself, and only with anonymous
//! stats on: once a day, which version this is, roughly how long it's been
//! installed, how many of a few things happened since the last time
//! (recordings, clips, minutes recorded, live views, recordings started from
//! an app, reports sent), and how its settings stand. No id, no names,
//! nothing from a video. The service on the other end keeps daily
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
    /// A recording this many seconds long.
    Recording(f64),
    Clip,
    /// Someone started watching live.
    Live,
    /// A recording started from a phone or computer.
    Remote,
    /// An issue report sent.
    Report,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct State {
    /// The UTC day of the last ping that got through.
    last: String,
    /// Whether one ever has: the first is how new installs are counted.
    ever: bool,
    /// The day this file first existed, in days since 1970: how old the install is.
    first: u64,
    recordings: u64,
    clips: u64,
    /// Seconds recorded; the ping says whole minutes.
    seconds: u64,
    live: u64,
    remote: u64,
    reports: u64,
}

impl State {
    fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for (key, value) in text.lines().filter_map(|l| l.split_once('=')) {
            match key {
                "last" => s.last = value.to_owned(),
                "ever" => s.ever = value == "true",
                "first" => s.first = value.parse().unwrap_or(0),
                "recordings" => s.recordings = value.parse().unwrap_or(0),
                "clips" => s.clips = value.parse().unwrap_or(0),
                "seconds" => s.seconds = value.parse().unwrap_or(0),
                "live" => s.live = value.parse().unwrap_or(0),
                "remote" => s.remote = value.parse().unwrap_or(0),
                "reports" => s.reports = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        s
    }

    fn text(&self) -> String {
        format!(
            "last={}\never={}\nfirst={}\nrecordings={}\nclips={}\nseconds={}\nlive={}\nremote={}\nreports={}\n",
            self.last, self.ever, self.first, self.recordings, self.clips, self.seconds, self.live, self.remote, self.reports
        )
    }
}

fn path() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(dir.join("framecorder/stats"))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn load() -> State {
    let mut state = path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| State::parse(&t)).unwrap_or_default();
    if state.first == 0 {
        state.first = now() / 86400;
    }
    state
}

/// How long an install has been around, as the stats service buckets it.
fn age(days: u64) -> &'static str {
    match days {
        0 => "0",
        1..=6 => "1-6",
        7..=29 => "7-29",
        30..=89 => "30-89",
        _ => "90+",
    }
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
    day(now())
}

/// One more of something for the next ping to mention.
pub fn made(what: Made) {
    // the tab's own tests save recordings that never happened
    if cfg!(test) {
        return;
    }
    let _held = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load();
    match what {
        Made::Recording(secs) => {
            state.recordings += 1;
            state.seconds += secs.max(0.0) as u64;
        }
        Made::Clip => state.clips += 1,
        Made::Live => state.live += 1,
        Made::Remote => state.remote += 1,
        Made::Report => state.reports += 1,
    }
    save(&state);
}

/// Everything a ping says. `settings` are the tab's, as the service names them.
fn body(state: &State, settings: &[(&str, String)]) -> Value {
    let settings: serde_json::Map<String, Value> = settings.iter().map(|(k, v)| ((*k).to_owned(), json!(v))).collect();
    json!({
        "app": "headset",
        "version": env!("CARGO_PKG_VERSION"),
        "os": "steamos",
        "new": !state.ever,
        "age": age((now() / 86400).saturating_sub(state.first)),
        // the service takes no more than this of each
        "counts": {
            "recordings": state.recordings.min(10_000),
            "clips": state.clips.min(10_000),
            "minutes": (state.seconds / 60).min(100_000),
            "live": state.live.min(10_000),
            "remote": state.remote.min(10_000),
            "reports": state.reports.min(1_000),
        },
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
pub fn ping(settings: Vec<(&'static str, String)>) {
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
        // whole minutes went; the seconds left over wait for the next one
        state.seconds = state.seconds.saturating_sub(sent.seconds / 60 * 60);
        state.live = state.live.saturating_sub(sent.live);
        state.remote = state.remote.saturating_sub(sent.remote);
        state.reports = state.reports.saturating_sub(sent.reports);
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
        let state = State { last: "2026-11-13".into(), ever: false, first: now() / 86400 - 12, recordings: 3, clips: 20_000, seconds: 185, live: 2, remote: 1, reports: 0 };
        let settings = [("shape", "wide".to_string()), ("paired", "3+".to_string())];
        assert_eq!(
            body(&state, &settings).to_string(),
            format!(
                r#"{{"age":"7-29","app":"headset","counts":{{"clips":10000,"live":2,"minutes":3,"recordings":3,"remote":1,"reports":0}},"new":true,"os":"steamos","settings":{{"paired":"3+","shape":"wide"}},"version":"{}"}}"#,
                env!("CARGO_PKG_VERSION")
            )
        );
        assert_eq!(State::parse(&state.text()), state);
        assert_eq!([age(0), age(6), age(7), age(29), age(30), age(89), age(90)], ["0", "1-6", "7-29", "7-29", "30-89", "30-89", "90+"]);
        assert_eq!(day(1_794_700_800 - 1), "2026-11-14");
        assert_eq!(day(951_782_400), "2000-02-29");
    }
}
