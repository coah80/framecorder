//! Recording and clipping from a paired phone. The dashboard tab owns the
//! recorder, so we only pass things along: command.json to it, and
//! status.json back, which also says whether it's recording right now.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::config::{write_atomic, Paths};

/// How long a phone waits for the tab to do what it asked.
const ANSWER_WITHIN: Duration = Duration::from_secs(4);
const LOOK_EVERY: Duration = Duration::from_millis(50);

pub const COMMANDS: [&str; 3] = ["record", "stop", "clip"];

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn read(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Whether the tab that wrote `status` is still around.
fn alive(status: &Value) -> bool {
    status["pid"].as_u64().is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists())
}

/// How things stand, for phones: whether the tab's running at all, and if
/// so whether it's recording (for how long) and whether a clip can be saved.
pub fn status(paths: &Paths) -> Value {
    let Some(s) = read(&paths.status()).filter(alive) else {
        return json!({ "available": false, "ready": false, "recording": false, "running": false, "elapsed_ms": 0, "clips": null, "clip_ready": false });
    };
    let running = s["running"].as_bool().unwrap_or(false);
    let since = if running { now_ms().saturating_sub(s["at"].as_u64().unwrap_or(0)) } else { 0 };
    json!({
        "available": true,
        "ready": s["ready"].as_bool().unwrap_or(false),
        "recording": s["recording"].as_bool().unwrap_or(false),
        "running": running,
        "elapsed_ms": s["recorded_ms"].as_u64().unwrap_or(0) + since,
        "clips": s["clips"],
        "clip_ready": s["clip_ready"].as_bool().unwrap_or(false),
    })
}

#[derive(Default)]
pub struct Remote {
    /// One command at a time, so none gets written over before the tab saw it.
    last: Mutex<u64>,
}

impl Remote {
    /// Asks the tab to `what` (one of [`COMMANDS`]) and waits for it to say
    /// how that went. Returns the new status, or a status code and why not.
    pub fn send(&self, paths: &Paths, what: &str, from: &str) -> Result<Value, (u16, String)> {
        if !COMMANDS.contains(&what) {
            return Err((400, format!("can't {what}, only record, stop or clip")));
        }
        if !read(&paths.status()).is_some_and(|s| alive(&s)) {
            return Err((503, "framecorder isn't open on the headset. it starts with SteamVR".into()));
        }
        let mut last = self.last.lock().unwrap();
        let done = |s: &Value| s["done"].as_u64().unwrap_or(0);
        let id = (*last).max(read(&paths.status()).map_or(0, |s| done(&s))) + 1;
        *last = id;
        let command = json!({ "id": id, "do": what, "from": from, "at": now_ms() });
        write_atomic(&paths.command(), command.to_string().as_bytes()).map_err(|e| (500, format!("couldn't pass that on: {e}")))?;

        let asked = Instant::now();
        while asked.elapsed() < ANSWER_WITHIN {
            std::thread::sleep(LOOK_EVERY);
            let Some(s) = read(&paths.status()) else { continue };
            if done(&s) < id {
                continue;
            }
            return match s["result"].as_str().unwrap_or("ok") {
                "ok" => Ok(status(paths)),
                why => Err((409, why.to_string())),
            };
        }
        Err((504, "the headset didn't answer, try again".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn paths(dir: &Path) -> Paths {
        Paths::new(&dir.join("Videos"), &dir.join("state"))
    }

    /// Stands in for the tab: answers each command the way it's told to.
    fn tab(paths: Paths, answer: &'static str) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(3) {
                if let Some(c) = read(&paths.command()) {
                    let recording = c["do"] == "record" && answer == "ok";
                    let s = json!({ "pid": std::process::id(), "ready": true, "recording": recording, "running": recording,
                        "recorded_ms": 0, "at": now_ms(), "clips": 30, "clip_ready": !recording, "done": c["id"], "result": answer });
                    write_atomic(&paths.status(), s.to_string().as_bytes()).unwrap();
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })
    }

    fn running_tab(paths: &Paths) {
        std::fs::create_dir_all(&paths.state).unwrap();
        let s = json!({ "pid": std::process::id(), "ready": true, "recording": false, "running": false, "recorded_ms": 0, "at": now_ms(), "clips": 30, "clip_ready": true, "done": 0, "result": "ok" });
        write_atomic(&paths.status(), s.to_string().as_bytes()).unwrap();
    }

    #[test]
    fn no_tab_no_commands() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        assert_eq!(status(&p)["available"], json!(false));
        assert_eq!(Remote::default().send(&p, "record", "pixel").unwrap_err().0, 503);
    }

    #[test]
    fn a_command_waits_for_the_tab() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        running_tab(&p);
        let remote = Arc::new(Remote::default());
        let t = tab(p.clone(), "ok");
        let s = remote.send(&p, "record", "pixel").unwrap();
        t.join().unwrap();
        assert_eq!((s["available"].as_bool(), s["recording"].as_bool()), (Some(true), Some(true)));
        assert_eq!(read(&p.command()).unwrap()["from"], json!("pixel"));
    }

    #[test]
    fn says_why_the_tab_said_no() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        running_tab(&p);
        let t = tab(p.clone(), "clips are off on the headset");
        let err = Remote::default().send(&p, "clip", "pixel").unwrap_err();
        t.join().unwrap();
        assert_eq!(err, (409, "clips are off on the headset".to_string()));
    }

    #[test]
    fn only_known_commands() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Remote::default().send(&paths(dir.path()), "explode", "pixel").unwrap_err().0, 400);
    }

    #[test]
    fn a_recording_keeps_counting() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        std::fs::create_dir_all(&p.state).unwrap();
        let s = json!({ "pid": std::process::id(), "ready": true, "recording": true, "running": true, "recorded_ms": 5000, "at": now_ms() - 2000, "clips": null, "clip_ready": false, "done": 0, "result": "ok" });
        write_atomic(&p.status(), s.to_string().as_bytes()).unwrap();
        let elapsed = status(&p)["elapsed_ms"].as_u64().unwrap();
        assert!((7000..8000).contains(&elapsed), "{elapsed}");
    }
}
