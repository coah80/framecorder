//! What the UI can ask for.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::platform::{view, ClipView};
use super::AppState;
use crate::core::engine::Status;
use crate::core::{discover, pairlink};

#[derive(Serialize)]
pub struct Overview {
    platform: &'static str,
    hosts: Vec<Status>,
    clips: Vec<ClipView>,
    download_dir: String,
    autostart: Option<bool>,
    /// Desktop: whether closing the window leaves it running in the tray.
    background: Option<bool>,
}

fn platform() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

#[tauri::command]
pub fn overview(app: AppHandle, state: State<'_, AppState>) -> Overview {
    #[cfg(desktop)]
    let autostart = {
        use tauri_plugin_autostart::ManagerExt;
        app.autolaunch().is_enabled().ok()
    };
    #[cfg(mobile)]
    let autostart = {
        let _ = &app;
        None
    };
    let background = cfg!(desktop).then(|| state.background.load(std::sync::atomic::Ordering::SeqCst));
    Overview {
        background,
        platform: platform(),
        hosts: state.engine.statuses(),
        clips: state.engine.clips().iter().map(view).collect(),
        download_dir: state.download_dir.to_string_lossy().into_owned(),
        autostart,
    }
}

/// Called once the page is up. On Android this asks for notifications and
/// starts the foreground service that keeps syncing in the background.
#[tauri::command]
pub async fn app_ready(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_framesync::FrameSyncExt;
        use tauri_plugin_notification::{NotificationExt, PermissionState};
        let granted = app.notification().permission_state().map_err(|e| e.to_string())?;
        if granted != PermissionState::Granted {
            let _ = app.notification().request_permission();
        }
        tauri::async_runtime::spawn_blocking(move || app.framesync().start_service())
            .await
            .map_err(|e| e.to_string())??;
    }
    #[cfg(not(target_os = "android"))]
    let _ = app;
    Ok(())
}

/// Looks for Frames for a few seconds, over mDNS and by asking every
/// address nearby, emitting `discovered` for each one.
#[tauri::command]
pub async fn discover(app: AppHandle, seconds: Option<u64>) -> Result<(), String> {
    let window = Duration::from_secs(seconds.unwrap_or(6).clamp(1, 30));
    discover::look(window, true, |found| {
        let _ = app.emit("discovered", &found);
        true
    })
    .await;
    Ok(())
}

#[tauri::command]
pub async fn pair(
    state: State<'_, AppState>,
    addr: String,
    fingerprint: Option<String>,
    code: String,
) -> Result<Status, String> {
    let code = code.trim().to_string();
    if !pairlink::is_code(&code) {
        return Err("the code is the 6 digits shown on your frame".into());
    }
    let addr = addr.trim().to_string();
    if addr.is_empty() {
        return Err("which frame? pick one or type its address".into());
    }
    let addr = if addr.contains(':') && !addr.ends_with(']') { addr } else { format!("{addr}:38619") };
    let fp = match fingerprint.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
        Some(f) => Some(crate::core::tls::normalize_fingerprint(f).ok_or("that fingerprint doesn't look right")?),
        None => None,
    };
    let host = state.engine.pair(&addr, fp.as_deref(), &code, None).await?;
    Ok(status_of(&state, &host.fingerprint))
}

#[tauri::command]
pub async fn pair_link(state: State<'_, AppState>, link: String) -> Result<Status, String> {
    let l = pairlink::parse(&link)?;
    let host = state.engine.pair(&l.addr, Some(&l.fingerprint), &l.code, None).await?;
    Ok(status_of(&state, &host.fingerprint))
}

fn status_of(state: &AppState, fingerprint: &str) -> Status {
    state
        .engine
        .statuses()
        .into_iter()
        .find(|s| s.fingerprint == fingerprint)
        .expect("just paired")
}

#[tauri::command]
pub fn unpair(state: State<'_, AppState>, fingerprint: String) -> Result<(), String> {
    state.engine.unpair(&fingerprint)
}

/// Has a Frame install its framecorder update now.
#[tauri::command]
pub async fn start_update(state: State<'_, AppState>, fingerprint: String) -> Result<(), String> {
    state.engine.start_update(&fingerprint).await
}

#[tauri::command]
pub fn retry_now(state: State<'_, AppState>) {
    state.engine.retry_now();
}

fn location(state: &AppState, key: &str) -> Result<String, String> {
    state.engine.clip(key).map(|e| e.location).ok_or_else(|| "that clip isn't here anymore".into())
}

#[tauri::command]
pub fn open_clip(app: AppHandle, state: State<'_, AppState>, key: String) -> Result<(), String> {
    let loc = location(&state, &key)?;
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_framesync::FrameSyncExt;
        app.framesync().open(&loc)
    }
    #[cfg(not(target_os = "android"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener().open_path(loc, None::<&str>).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn reveal_clip(app: AppHandle, state: State<'_, AppState>, key: String) -> Result<(), String> {
    let loc = location(&state, &key)?;
    #[cfg(desktop)]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener().reveal_item_in_dir(loc).map_err(|e| e.to_string())
    }
    #[cfg(mobile)]
    {
        let _ = (app, loc);
        Err("not on this platform".into())
    }
}

#[tauri::command]
pub fn share_clip(app: AppHandle, state: State<'_, AppState>, key: String) -> Result<(), String> {
    let loc = location(&state, &key)?;
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_framesync::FrameSyncExt;
        app.framesync().share(&loc)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, loc);
        Err("sharing is only on android".into())
    }
}

#[tauri::command]
pub fn open_folder(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener().open_path(state.download_dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
    }
    #[cfg(mobile)]
    {
        let _ = (app, state);
        Err("not on this platform".into())
    }
}

/// Whether closing the window keeps it syncing from the tray, or quits.
#[tauri::command]
pub fn set_background(app: AppHandle, state: State<'_, AppState>, enabled: bool) -> Result<bool, String> {
    #[cfg(desktop)]
    {
        let prefs = super::prefs::Prefs { background: enabled };
        super::prefs::save(&state.config_dir, &prefs).map_err(|e| format!("couldn't save that: {e}"))?;
        state.background.store(enabled, std::sync::atomic::Ordering::SeqCst);
        super::tray::set_visible(&app, enabled);
        Ok(enabled)
    }
    #[cfg(mobile)]
    {
        let _ = (app, state, enabled);
        Err("not on this platform".into())
    }
}

/// Closes the app for real, tray and all.
#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::ManagerExt;
        let auto = app.autolaunch();
        if enabled { auto.enable() } else { auto.disable() }.map_err(|e| e.to_string())?;
        auto.is_enabled().map_err(|e| e.to_string())
    }
    #[cfg(mobile)]
    {
        let _ = (app, enabled);
        Err("not on this platform".into())
    }
}
