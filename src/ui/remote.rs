//! Recording and clipping for paired phones. framecorder-sync writes what a
//! phone asked for to command.json; the tab does it and writes status.json,
//! which also tells phones what's going on, recording or not.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// A phone that's waited longer than this has given up on its command.
const STALE_MS: u64 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Record,
    Stop,
    Clip,
}

/// What phones see, written only when it changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// The setup's done, so the recorder may run.
    pub ready: bool,
    pub recording: bool,
    /// Recording and not paused (it pauses while the tab's on screen).
    pub running: bool,
    pub recorded_ms: u64,
    /// The clip length while clipping is on.
    pub clips: Option<u32>,
    /// A clip can be saved right now.
    pub clip_ready: bool,
}

pub struct Remote {
    seen: Option<SystemTime>,
    /// The last command done, and how it went.
    done: u64,
    result: String,
    published: Option<(Status, u64)>,
}

fn path(name: &str) -> Option<PathBuf> {
    Some(super::pairing::dir()?.join(name))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn modified(name: &str) -> Option<SystemTime> {
    std::fs::metadata(path(name)?).ok()?.modified().ok()
}

fn command_file() -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path("command.json")?).ok()?).ok()
}

impl Remote {
    /// Whatever was asked for before we started is old news.
    pub fn new() -> Self {
        let done = command_file().and_then(|c| c["id"].as_u64()).unwrap_or(0);
        Self { seen: modified("command.json"), done, result: "ok".into(), published: None }
    }

    /// A new command and who sent it, if one's come in.
    pub fn poll(&mut self) -> Option<(Command, String)> {
        let at = modified("command.json");
        if at.is_none() || at == self.seen {
            return None;
        }
        self.seen = at;
        let c = command_file()?;
        let id = c["id"].as_u64()?;
        if id <= self.done {
            return None;
        }
        self.done = id;
        if now_ms().saturating_sub(c["at"].as_u64().unwrap_or(0)) > STALE_MS {
            self.result = "that took too long, try again".into();
            return None;
        }
        let from = c["from"].as_str().unwrap_or("your phone").to_string();
        let command = match c["do"].as_str()? {
            "record" => Command::Record,
            "stop" => Command::Stop,
            "clip" => Command::Clip,
            other => {
                self.result = format!("don't know how to {other}");
                return None;
            }
        };
        Some((command, from))
    }

    /// How the last command went: "ok", or why not.
    pub fn answer(&mut self, result: impl Into<String>) {
        self.result = result.into();
    }

    /// Writes status.json when something phones care about changed.
    pub fn publish(&mut self, status: &Status) {
        // the recorded time moves on its own; phones work it out from `at`
        let key = Status { recorded_ms: 0, ..status.clone() };
        if self.published.as_ref().is_some_and(|(s, done)| *s == key && *done == self.done) {
            return;
        }
        // nothing to tell before framecorder-sync has been set up
        let Some(file) = path("status.json").filter(|f| f.parent().is_some_and(|d| d.is_dir())) else { return };
        let body = json!({
            "pid": std::process::id(),
            "ready": status.ready,
            "recording": status.recording,
            "running": status.running,
            "recorded_ms": status.recorded_ms,
            "at": now_ms(),
            "clips": status.clips,
            "clip_ready": status.clip_ready,
            "done": self.done,
            "result": self.result,
        });
        let tmp = file.with_extension("json.tmp");
        match std::fs::write(&tmp, body.to_string()).and_then(|_| std::fs::rename(&tmp, &file)) {
            Ok(()) => self.published = Some((key, self.done)),
            Err(e) => log::warn!("couldn't write {}: {e}", file.display()),
        }
    }
}
