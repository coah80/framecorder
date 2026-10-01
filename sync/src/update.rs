//! Updating framecorder from the apps: whether there's an update (what
//! `framecorder-setup --check` says), and starting one now (the same update
//! service the timer starts every few hours). No password needed, the panel
//! helper isn't part of it.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long one check holds, so a few apps asking doesn't mean a few trips
/// online. Starting an update forgets it.
const CHECK_FOR: Duration = Duration::from_secs(10 * 60);
const SERVICE: &str = "framecorder-update.service";

#[derive(Default)]
pub struct Updates {
    checked: Mutex<Option<(Instant, serde_json::Value)>>,
}

impl Updates {
    /// What the apps get: installed, latest, available, and whether an
    /// update's running right now.
    pub fn status(&self) -> Result<serde_json::Value, String> {
        let mut checked = self.checked.lock().unwrap();
        let fresh = checked.as_ref().filter(|(at, _)| at.elapsed() < CHECK_FOR).map(|(_, v)| v.clone());
        let mut status = match fresh {
            Some(v) => v,
            None => {
                let v = check()?;
                *checked = Some((Instant::now(), v.clone()));
                v
            }
        };
        status["updating"] = serde_json::Value::Bool(updating());
        Ok(status)
    }

    pub fn start(&self) -> Result<(), String> {
        *self.checked.lock().unwrap() = None;
        let out = Command::new("systemctl")
            .args(["--user", "--no-block", "start", SERVICE])
            .output()
            .map_err(|e| format!("couldn't run systemctl: {e}"))?;
        if !out.status.success() {
            return Err(format!("couldn't start the update: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        log::info!("starting a framecorder update, an app asked");
        Ok(())
    }
}

fn check() -> Result<serde_json::Value, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME isn't set")?;
    let out = Command::new(home.join(".local/bin/framecorder-setup"))
        .arg("--check")
        .output()
        .map_err(|e| format!("couldn't check for an update: {e}"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        return Err(format!("couldn't check for an update: {}", said.trim().lines().last().unwrap_or("")));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("odd answer from framecorder-setup: {e}"))
}

/// Whether the update service is going (it's a oneshot, so "activating"
/// while it runs).
fn updating() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", SERVICE])
        .output()
        .is_ok_and(|o| matches!(String::from_utf8_lossy(&o.stdout).trim(), "activating" | "active" | "reloading"))
}
