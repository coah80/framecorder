//! The parts that differ between desktop and Android: where clips go, how
//! the UI hears about things, what we call ourselves.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

#[cfg(mobile)]
use crate::core::api::RemoteClip;
use crate::core::engine::{Listener, Progress, Sink, Status};
use crate::core::store::Entry;
#[cfg(mobile)]
use crate::core::store::Host;

#[derive(Clone, Serialize)]
pub struct ClipView {
    pub key: String,
    pub host: String,
    pub name: String,
    pub kind: String,
    pub size: u64,
    pub created: i64,
    pub duration_s: Option<f64>,
    pub location: String,
    /// False once the file's been moved or deleted on this device.
    pub exists: bool,
}

pub fn view(e: &Entry) -> ClipView {
    let exists = e.location.starts_with("content://") || Path::new(&e.location).exists();
    ClipView {
        key: e.key(),
        host: e.host.clone(),
        name: e.name.clone(),
        kind: e.kind.clone(),
        size: e.size,
        created: e.created,
        duration_s: e.duration_s,
        location: e.location.clone(),
        exists,
    }
}

#[cfg(desktop)]
pub fn sink(_app: &AppHandle) -> Result<(Arc<dyn Sink>, PathBuf), Box<dyn std::error::Error>> {
    let videos = dirs::video_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Videos")))
        .ok_or("can't find a videos folder")?;
    let root = videos.join("framecorder");
    std::fs::create_dir_all(&root)?;
    Ok((Arc::new(crate::core::engine::DirSink { root: root.clone() }), root))
}

#[cfg(mobile)]
pub fn sink(app: &AppHandle) -> Result<(Arc<dyn Sink>, PathBuf), Box<dyn std::error::Error>> {
    use tauri::Manager;
    let cache = app.path().app_cache_dir()?.join("downloads");
    std::fs::create_dir_all(&cache)?;
    Ok((Arc::new(GallerySink { app: app.clone(), cache }), PathBuf::from("Movies/framecorder")))
}

/// Downloads into the app's cache, then hands finished files to MediaStore
/// so they show up in the gallery under Movies/framecorder.
#[cfg(mobile)]
struct GallerySink {
    app: AppHandle,
    cache: PathBuf,
}

#[cfg(mobile)]
impl Sink for GallerySink {
    fn staging(&self, host: &Host, clip: &RemoteClip) -> PathBuf {
        let short = &host.fingerprint[..8.min(host.fingerprint.len())];
        self.cache.join(format!("{short}-{}.part", crate::core::engine::safe_name(&clip.name)))
    }

    /// The download in our cache, and the gallery's copy of it.
    fn copies(&self) -> u64 {
        2
    }

    fn finish(&self, staged: &Path, _host: &Host, clip: &RemoteClip) -> Result<String, String> {
        #[cfg(target_os = "android")]
        {
            use tauri_plugin_framesync::FrameSyncExt;
            let subdir = if clip.kind == "clip" { "clips" } else { "" };
            let name = crate::core::engine::safe_name(&clip.name);
            let path = staged.to_string_lossy().into_owned();
            // the copy can take a while for a long recording
            tokio::task::block_in_place(|| self.app.framesync().save_to_gallery(&path, &name, subdir))
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (&self.app, staged, clip);
            Err("no gallery on this platform".into())
        }
    }
}

pub fn device_name(app: &AppHandle) -> String {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_framesync::FrameSyncExt;
        if let Ok(name) = app.framesync().device_name() {
            return name;
        }
    }
    let _ = app;
    crate::core::device_name()
}

pub struct UiListener {
    app: AppHandle,
}

impl UiListener {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl Listener for UiListener {
    fn status(&self, status: &Status) {
        let _ = self.app.emit("status", status);
        #[cfg(desktop)]
        super::tray::set_status(&self.app, status);
    }

    fn progress(&self, progress: &Progress) {
        let _ = self.app.emit("progress", progress);
    }

    fn synced(&self, entry: &Entry) {
        let _ = self.app.emit("synced", view(entry));
        let what = if entry.kind == "clip" { "new clip" } else { "new recording" };
        let _ = self
            .app
            .notification()
            .builder()
            .title(format!("{what} from your frame"))
            .body(&entry.name)
            .show();
    }

    fn busy(&self, busy: bool) {
        #[cfg(target_os = "android")]
        {
            use tauri_plugin_framesync::FrameSyncExt;
            if let Err(e) = self.app.framesync().set_busy(busy) {
                log::warn!("wake lock: {e}");
            }
        }
        let _ = self.app.emit("busy", busy);
    }

    fn removed(&self, host: &str, id: &str) {
        let _ = self.app.emit("removed", serde_json::json!({ "host": host, "id": id }));
    }
}
