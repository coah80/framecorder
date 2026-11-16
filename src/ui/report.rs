//! Reporting an issue from the tab: what the user typed on SteamVR's
//! keyboard, what goes with it, and the sending. Gathering the details and
//! sending both block for seconds, so they run on their own thread and the
//! tab picks the result up on its next tick.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use serde_json::Value;

use crate::report;

/// The most a description can be, the same as the relay takes.
pub const MAX_CHARS: usize = 4000;

/// Where the report is, from the user's side.
pub enum Phase {
    Writing,
    Sending,
    Sent { number: u64, url: String },
    Failed(String),
}

/// The bits of the diagnostics the summary shows, in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// "0.2.0 · release · panels unlocked"
    pub framecorder: String,
    /// "SteamOS 3.7.8 · build 20251001"
    pub os: String,
    /// "2.12.5 · public"
    pub steamvr: String,
    pub errors: usize,
    /// The names of the logs that go along, without their extension.
    pub logs: Vec<String>,
}

impl Summary {
    pub fn from(d: &Value) -> Self {
        let text = |v: &Value| v.as_str().filter(|s| !s.trim().is_empty()).map(str::to_string);
        let fc = &d["framecorder"];
        let mut framecorder = text(&fc["version"]).unwrap_or_else(|| "unknown version".into());
        if fc["channel"] == "dev" {
            framecorder.push_str(" · dev build");
        }
        framecorder.push_str(if fc["unlocked"] == true { " · panels unlocked" } else { " · SteamVR's view" });

        let os_info = &d["os"];
        let mut os = text(&os_info["name"]).unwrap_or_else(|| "SteamOS".into());
        if let Some(v) = text(&os_info["version"]) {
            os.push_str(&format!(" {v}"));
        }
        if let Some(b) = text(&os_info["build_id"]) {
            os.push_str(&format!(" · build {b}"));
        }
        if let Some(branch) = text(&os_info["branch"]).filter(|b| b != "rel") {
            os.push_str(&format!(" · {branch}"));
        }

        let steam = &d["steam"];
        let steamvr = match (text(&steam["steamvr_version"]), text(&steam["steamvr_branch"])) {
            (Some(v), Some(b)) => format!("{v} · {b}"),
            (Some(v), None) => v,
            (None, Some(b)) => format!("version unknown · {b}"),
            (None, None) => "not found".into(),
        };

        let logs = d["logs"]
            .as_array()
            .map(|logs| logs.iter().filter_map(|l| text(&l["name"])).map(|n| n.trim_end_matches(".txt").trim_end_matches(".log").to_string()).collect())
            .unwrap_or_default();
        Self {
            framecorder,
            os,
            steamvr,
            errors: d["recent_errors"].as_array().map_or(0, Vec::len),
            logs,
        }
    }
}

/// What the tab wants to know after a tick.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Gathered,
    Sent { number: u64, url: String },
    Failed(String),
}

pub struct Report {
    pub text: String,
    /// SteamVR's keyboard is up, typing into this.
    pub typing: bool,
    pub summary: Option<Summary>,
    diagnostics: Option<Value>,
    phase: Phase,
    gathering: Option<Receiver<Value>>,
    sending: Option<Receiver<Result<report::Sent, String>>>,
}

impl Default for Report {
    fn default() -> Self {
        Self::new()
    }
}

impl Report {
    pub fn new() -> Self {
        Self { text: String::new(), typing: false, summary: None, diagnostics: None, phase: Phase::Writing, gathering: None, sending: None }
    }

    /// Starts gathering what gets sent, once. The full thing lands next to
    /// the recorder's log so anyone can read exactly what went.
    pub fn open(&mut self) {
        if self.diagnostics.is_some() || self.gathering.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.gathering = Some(rx);
        std::thread::Builder::new()
            .name("report-gather".into())
            .spawn(move || {
                let diagnostics = report::collect();
                if let Some(path) = saved_copy() {
                    if let Err(e) = std::fs::write(&path, diagnostics.to_string()) {
                        log::warn!("couldn't keep a copy of the report at {}: {e}", path.display());
                    }
                }
                let _ = tx.send(diagnostics);
            })
            .map_err(|e| log::warn!("couldn't gather the report: {e}"))
            .ok();
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// What SteamVR's keyboard has so far, while it's still up.
    pub fn typed(&mut self, text: String) {
        self.text = cut(text);
        self.typing = true;
    }

    /// Done was pressed: the field is the text, even if that's nothing.
    pub fn finished(&mut self, text: String) {
        self.text = cut(text);
        self.typing = false;
    }

    /// The keyboard went away without Done. An empty field then means it
    /// was dismissed, not cleared, so the draft stays.
    pub fn closed(&mut self, text: String) {
        if !text.is_empty() {
            self.text = cut(text);
        }
        self.typing = false;
    }

    /// Something's written, the details are in, and nothing's on its way.
    pub fn can_send(&self) -> bool {
        !self.text.trim().is_empty() && self.diagnostics.is_some() && matches!(self.phase, Phase::Writing | Phase::Failed(_))
    }

    pub fn send(&mut self) {
        if !self.can_send() {
            return;
        }
        let Some(diagnostics) = self.diagnostics.clone() else { return };
        let text = self.text.clone();
        let (tx, rx) = mpsc::channel();
        match std::thread::Builder::new().name("report-send".into()).spawn(move || {
            let _ = tx.send(report::send(&text, &diagnostics).map_err(|e| format!("{e:#}")));
        }) {
            Ok(_) => {
                self.sending = Some(rx);
                self.phase = Phase::Sending;
            }
            Err(e) => self.phase = Phase::Failed(format!("couldn't start sending: {e}")),
        }
    }

    /// Picks up what the threads finished, if anything.
    pub fn poll(&mut self) -> Option<Outcome> {
        if let Some(rx) = &self.gathering {
            match rx.try_recv() {
                Ok(diagnostics) => {
                    self.summary = Some(Summary::from(&diagnostics));
                    self.diagnostics = Some(diagnostics);
                    self.gathering = None;
                    return Some(Outcome::Gathered);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.gathering = None;
                    self.phase = Phase::Failed("couldn't gather the details, close this and try again".into());
                    return Some(Outcome::Failed("couldn't gather the details".into()));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let rx = self.sending.as_ref()?;
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Err("sending stopped partway, try again".into()),
        };
        self.sending = None;
        Some(match result {
            Ok(sent) => {
                self.phase = Phase::Sent { number: sent.number, url: sent.url.clone() };
                Outcome::Sent { number: sent.number, url: sent.url }
            }
            Err(why) => {
                self.phase = Phase::Failed(why.clone());
                Outcome::Failed(why)
            }
        })
    }
}

fn cut(text: String) -> String {
    if text.chars().count() <= MAX_CHARS {
        text
    } else {
        text.chars().take(MAX_CHARS).collect()
    }
}

/// `~/.local/state/framecorder/report.json`, the report as last gathered.
pub fn saved_copy() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?
        .join("framecorder");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("report.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summary_reads_what_the_collector_found_and_copes_with_what_it_did_not() {
        let full = json!({
            "framecorder": {"version": "0.2.0", "channel": "dev", "unlocked": true},
            "os": {"name": "SteamOS", "version": "3.7.8", "build_id": "20251001.1", "branch": "beta"},
            "steam": {"steamvr_version": "2.12.5", "steamvr_branch": "public"},
            "recent_errors": ["a", "b", "c"],
            "logs": [{"name": "recorder.log", "text": ""}, {"name": "vrserver.txt", "text": ""}]
        });
        let s = Summary::from(&full);
        assert_eq!(s.framecorder, "0.2.0 · dev build · panels unlocked");
        assert_eq!(s.os, "SteamOS 3.7.8 · build 20251001.1 · beta");
        assert_eq!(s.steamvr, "2.12.5 · public");
        assert_eq!(s.errors, 3);
        assert_eq!(s.logs, ["recorder", "vrserver"]);

        // A broken install is exactly when people report: nulls everywhere.
        let bare = json!({"framecorder": {"version": null, "channel": "release", "unlocked": false}, "os": {}, "steam": {}, "recent_errors": null, "logs": null});
        let s = Summary::from(&bare);
        assert_eq!(s.framecorder, "unknown version · SteamVR's view");
        assert_eq!(s.os, "SteamOS");
        assert_eq!(s.steamvr, "not found");
        assert_eq!((s.errors, s.logs.len()), (0, 0));
    }

    #[test]
    fn nothing_goes_without_words_and_details_and_closing_the_keyboard_keeps_the_draft() {
        let mut r = Report::new();
        assert!(!r.can_send(), "nothing typed, nothing gathered");
        r.diagnostics = Some(json!({}));
        assert!(!r.can_send(), "nothing typed");
        r.typed("   ".into());
        assert!(r.typing && !r.can_send(), "only spaces");
        r.typed("the clip came out tilted".into());
        assert!(r.typing && r.can_send());
        // Closing the keyboard with an empty field keeps what was there.
        r.closed(String::new());
        assert!(!r.typing);
        assert_eq!(r.text, "the clip came out tilted");
        // Done with an empty field means it was cleared on purpose.
        r.finished(String::new());
        assert!(r.text.is_empty() && !r.can_send());
        r.finished("x".repeat(MAX_CHARS + 50));
        assert_eq!(r.text.chars().count(), MAX_CHARS);
    }

    #[test]
    fn a_failed_send_comes_back_as_something_to_try_again() {
        // A relay that hangs up without answering, so curl gets nothing.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/report", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                drop(stream);
            }
        });
        std::env::set_var("FRAMECORDER_REPORT_URL", &url);
        let mut r = Report::new();
        r.diagnostics = Some(json!({}));
        r.finished("sync stopped".into());
        r.send();
        assert!(matches!(r.phase, Phase::Sending));
        assert!(!r.can_send(), "no double sends");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let outcome = loop {
            if let Some(o) = r.poll() {
                break o;
            }
            assert!(std::time::Instant::now() < deadline, "the send never came back");
            std::thread::yield_now();
        };
        std::env::remove_var("FRAMECORDER_REPORT_URL");
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        assert!(matches!(r.phase, Phase::Failed(_)));
        assert!(r.can_send(), "the draft is still there to send again");
    }
}
