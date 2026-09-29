//! Runs the recorder as its own process and steers it over stdin, so the UI
//! never touches video and a crash in either one can't take down both. With
//! clipping on it stays up the whole time (the replay buffer lives in it),
//! otherwise it runs for one recording at a time.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::settings::Settings;

/// How long a quitting recorder gets to finish its files before we give up on it.
const STOP_GRACE: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Recording,
    Clip,
}

/// What the recorder says on stdout.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// A recording actually started writing into this file.
    Started(PathBuf),
    Saved { kind: Kind, path: PathBuf, bytes: u64, secs: f64 },
    /// A clip was asked for mid recording; the recording has it anyway.
    ClipWhileRecording,
    Failed { kind: Option<Kind>, message: String },
}

pub fn parse_event(line: &str) -> Option<Event> {
    let (word, rest) = line.trim_end().split_once(' ')?;
    let kind = |k: &str| match k {
        "recording" => Some(Kind::Recording),
        "clip" => Some(Kind::Clip),
        _ => None,
    };
    match word {
        "recording" => Some(Event::Started(PathBuf::from(rest))),
        "saved" => {
            let mut parts = rest.splitn(4, ' ');
            let kind = kind(parts.next()?)?;
            let bytes = parts.next()?.parse().ok()?;
            let secs = parts.next()?.parse().ok()?;
            Some(Event::Saved { kind, path: PathBuf::from(parts.next()?), bytes, secs })
        }
        "busy" if rest == "recording" => Some(Event::ClipWhileRecording),
        "failed" => {
            let (k, message) = rest.split_once(' ').unwrap_or((rest, ""));
            Some(Event::Failed { kind: kind(k), message: message.to_string() })
        }
        _ => None,
    }
}

pub struct Recorder {
    child: Child,
    stdin: Option<ChildStdin>,
    events: Receiver<Event>,
    log: PathBuf,
    /// What it was started with, to tell when settings changed underneath it.
    pub args: Vec<String>,
    pub replay: Option<u32>,
    paused: bool,
    quitting: Option<Instant>,
}

impl Recorder {
    /// Starts paused; `set_paused(false)` once our tab is out of view.
    pub fn start(settings: &Settings) -> Result<Self> {
        let exe = std::env::current_exe()?.with_file_name("framecorder");
        let log = log_path()?;
        let log_file = File::create(&log).with_context(|| format!("creating {}", log.display()))?;
        let args = settings.recorder_args();
        let replay = settings.clipping();

        let mut cmd = Command::new(&exe);
        if let Some(secs) = replay {
            cmd.args(["--replay", &secs.to_string()]);
        }
        cmd.args(&args).args(["--control", "--start-paused"]);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(log_file).env("RUST_LOG", "info");
        unsafe {
            cmd.pre_exec(|| {
                // If the UI dies, the recorder still finishes its files.
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGINT);
                Ok(())
            });
        }
        let mut child = cmd.spawn().with_context(|| format!("starting {}", exe.display()))?;
        log::info!("recorder started (pid {}, clipping {:?})", child.id(), replay);

        let (tx, events) = mpsc::channel();
        let stdout = child.stdout.take().context("no stdout from the recorder")?;
        std::thread::Builder::new().name("recorder-events".into()).spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
                match parse_event(&line) {
                    Some(e) => {
                        if tx.send(e).is_err() {
                            break;
                        }
                    }
                    None => log::debug!("recorder said: {line}"),
                }
            }
        })?;

        Ok(Self { stdin: child.stdin.take(), child, events, log, args, replay, paused: true, quitting: None })
    }

    fn send(&mut self, line: &str) {
        let ok = self.stdin.as_mut().is_some_and(|s| writeln!(s, "{line}").and_then(|_| s.flush()).is_ok());
        if !ok {
            log::warn!("couldn't tell the recorder to {line}");
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        if paused != self.paused && self.quitting.is_none() {
            self.paused = paused;
            self.send(if paused { "pause" } else { "resume" });
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn record(&mut self) -> Result<()> {
        let path = output_path("")?;
        self.send(&format!("record {}", path.display()));
        Ok(())
    }

    pub fn stop_record(&mut self) {
        self.send("stop-record");
    }

    pub fn clip(&mut self) {
        self.send("clip");
    }

    /// Asks it to wrap up and exit; anything being written gets finished.
    pub fn quit(&mut self) {
        if self.quitting.is_none() {
            self.send("quit");
            self.stdin = None;
            self.quitting = Some(Instant::now());
        }
    }

    pub fn quitting(&self) -> bool {
        self.quitting.is_some()
    }

    pub fn events(&mut self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    /// Whether it has exited: Ok when it was asked to, Err with its last
    /// complaint otherwise.
    pub fn exited(&mut self) -> Option<std::result::Result<(), String>> {
        if self.quitting.is_some_and(|s| s.elapsed() > STOP_GRACE) {
            log::warn!("recorder didn't stop in time, killing it");
            let _ = self.child.kill();
        }
        let status = match self.child.try_wait() {
            Ok(Some(status)) => status,
            Ok(None) => return None,
            Err(e) => return Some(Err(format!("lost track of the recorder: {e}"))),
        };
        if status.success() && self.quitting.is_some() {
            return Some(Ok(()));
        }
        if let Some(sig) = std::os::unix::process::ExitStatusExt::signal(&status) {
            return Some(Err(format!("The recorder was killed (signal {sig})")));
        }
        Some(Err(self.last_error()))
    }

    /// What it cost, from the recorder's perf summary line, shortened for the
    /// tab: "cpu 4.1% · gpu 2.3%".
    pub fn cost(&self) -> Option<String> {
        let text = std::fs::read_to_string(&self.log).ok()?;
        let line = text.lines().rev().find(|l| l.contains("perf summary:"))?;
        let pick = |key: &str| {
            let rest = &line[line.find(key)? + key.len()..];
            Some(rest.split_whitespace().next()?.trim_end_matches(',').to_string())
        };
        Some(format!("cpu {} · gpu {}", pick("recorder cpu ")?, pick("gpu ")?))
    }

    /// The recorder's last complaint, for showing in the UI.
    fn last_error(&self) -> String {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        let line = text
            .lines()
            .rev()
            .find(|l| l.contains("ERROR"))
            .or_else(|| text.lines().last())
            .unwrap_or("the recorder stopped unexpectedly");
        line.split_once("] ").map_or(line, |(_, m)| m).trim().to_string()
    }

    /// Stops and waits, for when the UI itself is shutting down.
    pub fn finish(mut self) {
        self.quit();
        let deadline = Instant::now() + STOP_GRACE;
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).context("HOME isn't set")
}

/// ~/Videos/framecorder/<sub>/<time>.mp4
pub fn output_path(sub: &str) -> Result<PathBuf> {
    let mut t: libc::tm = unsafe { std::mem::zeroed() };
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    unsafe { libc::localtime_r(&now, &mut t) };
    let name = format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}.mp4",
        t.tm_year + 1900,
        t.tm_mon + 1,
        t.tm_mday,
        t.tm_hour,
        t.tm_min,
        t.tm_sec
    );
    Ok(home()?.join("Videos/framecorder").join(sub).join(name))
}

fn log_path() -> Result<PathBuf> {
    let dir = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .map_or_else(|| home().map(|h| h.join(".local/state")), Ok)?
        .join("framecorder");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("recorder.log"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_the_recorder_says() {
        assert_eq!(parse_event("recording /home/a/x.mp4"), Some(Event::Started("/home/a/x.mp4".into())));
        assert_eq!(
            parse_event("saved clip 1234 30.5 /home/a/b c.mp4\n"),
            Some(Event::Saved { kind: Kind::Clip, path: "/home/a/b c.mp4".into(), bytes: 1234, secs: 30.5 })
        );
        assert_eq!(
            parse_event("failed recording no video was recorded"),
            Some(Event::Failed { kind: Some(Kind::Recording), message: "no video was recorded".into() })
        );
        assert_eq!(
            parse_event("failed command bad clip length"),
            Some(Event::Failed { kind: None, message: "bad clip length".into() })
        );
        assert_eq!(parse_event("busy recording"), Some(Event::ClipWhileRecording));
        assert_eq!(parse_event("hello"), None);
    }
}
