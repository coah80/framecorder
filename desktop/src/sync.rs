//! the sync engine from app/src/core, and the bridge that carries what it
//! says over to the ui thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use framecorder_app_lib::core::api::{About, Remote};
use framecorder_app_lib::core::engine::{DirSink, Engine, Listener, Progress, Status};
use framecorder_app_lib::core::store::Entry;
use framecorder_app_lib::headless::default_state_dir;

/// what the engine says, in the order it says it
pub enum Msg {
    Status,
    Progress(Progress),
    Synced(Entry),
    Busy(bool),
    Removed,
    /// what a frame's tab is up to, by fingerprint. None once we've lost touch
    Remote(String, Option<Remote>),
    /// a frame's battery and storage, by fingerprint. None once we've lost touch
    About(String, Option<About>),
}

struct Bridge(async_channel::Sender<Msg>);

impl Listener for Bridge {
    // the engine stores a status before it calls this, so the ui just re-reads them all
    fn status(&self, _status: &Status) {
        let _ = self.0.try_send(Msg::Status);
    }

    fn progress(&self, progress: &Progress) {
        let _ = self.0.try_send(Msg::Progress(progress.clone()));
    }

    fn synced(&self, entry: &Entry) {
        let _ = self.0.try_send(Msg::Synced(entry.clone()));
    }

    fn busy(&self, busy: bool) {
        let _ = self.0.try_send(Msg::Busy(busy));
    }

    fn removed(&self, _host: &str, _id: &str) {
        let _ = self.0.try_send(Msg::Removed);
    }

    fn remote(&self, host: &str, remote: Option<&Remote>) {
        let _ = self.0.try_send(Msg::Remote(host.to_string(), remote.cloned()));
    }

    fn about(&self, host: &str, about: Option<&About>) {
        let _ = self.0.try_send(Msg::About(host.to_string(), about.cloned()));
    }
}

#[derive(Clone)]
pub struct Core {
    pub engine: Arc<Engine>,
    pub rt: tokio::runtime::Handle,
    pub download_dir: PathBuf,
    pub state_dir: PathBuf,
}

/// where clips land: ~/Videos/framecorder (~/Movies/framecorder on a mac)
fn download_dir() -> Result<PathBuf, String> {
    let videos = dirs::video_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(if cfg!(target_os = "macos") { "Movies" } else { "Videos" })))
        .ok_or("can't find a videos folder")?;
    Ok(videos.join("framecorder"))
}

/// the same state folder as the tauri app and headless mode, so pairings carry over
pub fn state_dir() -> PathBuf {
    default_state_dir()
}

/// starts syncing every paired frame. a demo gets an empty state and folder of
/// its own, so nothing real is touched
pub fn start(rt: tokio::runtime::Handle, demo: bool) -> Result<(Core, async_channel::Receiver<Msg>), String> {
    let (state_dir, download_dir) = if demo {
        let tmp = std::env::temp_dir().join("framecorder-demo");
        (tmp.clone(), tmp.join("clips"))
    } else {
        (state_dir(), download_dir()?)
    };
    std::fs::create_dir_all(&state_dir).map_err(|e| format!("can't use {}: {e}", state_dir.display()))?;
    std::fs::create_dir_all(&download_dir).map_err(|e| format!("can't use {}: {e}", download_dir.display()))?;

    let (tx, rx) = async_channel::unbounded();
    let engine = Engine::new(
        rt.clone(),
        &state_dir,
        Arc::new(DirSink { root: download_dir.clone() }),
        Arc::new(Bridge(tx)),
        &framecorder_app_lib::core::device_name(),
    );
    if !demo {
        engine.start_all();
    }
    Ok((Core { engine, rt, download_dir, state_dir }, rx))
}

/// a clip, the way the ui shows it
#[derive(Clone)]
pub struct Clip {
    pub key: String,
    pub name: String,
    pub is_clip: bool,
    pub size: u64,
    pub created: i64,
    pub duration_s: Option<f64>,
    pub location: PathBuf,
}

impl Clip {
    pub fn from(e: &Entry) -> Self {
        Self {
            key: e.key(),
            name: e.name.clone(),
            is_clip: e.kind == "clip",
            size: e.size,
            created: e.created,
            duration_s: e.duration_s,
            location: PathBuf::from(&e.location),
        }
    }
}

/// the library: every clip whose file is still here, newest first. one
/// deleted or moved out of the folder drops out of it, and the engine still
/// remembers syncing it, so it never comes down again
pub fn library(engine: &Engine) -> Vec<Clip> {
    let mut clips: Vec<Clip> =
        engine.clips().iter().filter(|e| Path::new(&e.location).exists()).map(Clip::from).collect();
    clips.sort_by_key(|c| std::cmp::Reverse(c.created));
    clips
}

/// only one of us syncs at a time, two would download everything twice
pub fn single_instance(state_dir: &Path) -> Result<std::fs::File, String> {
    let path = state_dir.join("desktop.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("can't open {}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => {
            // the running one watches for this and brings its window up
            let _ = std::fs::write(show_marker(state_dir), b"");
            Err("framecorder is already open, bringing it up".into())
        }
        Err(std::fs::TryLockError::Error(e)) => {
            log::warn!("couldn't lock {}: {e}, carrying on", path.display());
            Ok(file)
        }
    }
}

/// a file the second launch leaves, so the first one knows to show itself
pub fn show_marker(state_dir: &Path) -> PathBuf {
    state_dir.join("show")
}
