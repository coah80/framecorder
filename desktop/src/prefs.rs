//! the app's own settings, in the same prefs.json the tauri app uses, next
//! to the pairings.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "prefs.json";

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// closing the window leaves it syncing from the tray. off: closing quits,
    /// and there's no tray icon
    pub background: bool,
    /// clips as a grid, or a list
    pub grid: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { background: true, grid: true }
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
