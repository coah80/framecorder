//! The desktop app's own settings, kept next to its pairings.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "prefs.json";

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Closing the window leaves it syncing from the tray. Off: closing quits,
    /// and there's no tray icon.
    pub background: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { background: true }
    }
}

fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

pub fn load(dir: &Path) -> Prefs {
    std::fs::read(path(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(dir: &Path, prefs: &Prefs) -> std::io::Result<()> {
    std::fs::write(path(dir), serde_json::to_vec_pretty(prefs)?)
}
