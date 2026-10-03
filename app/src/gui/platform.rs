//! Where clips go and how the UI hears about things.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::core::engine::{Listener, Progress, Sink, Status};
use crate::core::store::Entry;

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
    let exists = Path::new(&e.location).exists();
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

pub fn sink() -> Result<(Arc<dyn Sink>, PathBuf), Box<dyn std::error::Error>> {
    let videos = dirs::video_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Videos")))
        .ok_or("can't find a videos folder")?;
    let root = videos.join("framecorder");
    std::fs::create_dir_all(&root)?;
    Ok((Arc::new(crate::core::engine::DirSink { root: root.clone() }), root))
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
        let _ = self.app.emit("busy", busy);
    }

    fn removed(&self, host: &str, id: &str) {
        let _ = self.app.emit("removed", serde_json::json!({ "host": host, "id": id }));
    }
}
