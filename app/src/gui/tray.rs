//! Tray icon, so syncing carries on with the window closed.

use std::sync::atomic::Ordering;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use super::AppState;
use crate::core::engine::{State, Status};

const TRAY_ID: &str = "main";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open framecorder", true, None::<&str>)?;
    let sync = MenuItem::with_id(app, "sync", "Sync now", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &sync, &sep, &quit])?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("framecorder")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => super::show_main(app),
            "sync" => app.state::<AppState>().engine.retry_now(),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                super::show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn set_status(app: &AppHandle, status: &Status) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let text = match status.state {
        State::Connected => format!("framecorder: connected to {}", status.name),
        State::Connecting => format!("framecorder: looking for {}", status.name),
        State::Unreachable => format!("framecorder: can't reach {}, is it on?", status.name),
        State::Full => "framecorder: this device is out of space".to_string(),
        State::Unpaired | State::WrongFingerprint => format!("framecorder: {} needs pairing again", status.name),
    };
    let _ = tray.set_tooltip(Some(text));
}

pub fn set_visible(app: &AppHandle, visible: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(visible);
    }
}

/// Whether closing the window should just hide it. Only with a tray icon to
/// get it back from, and when it's meant to keep running.
pub fn hide_instead_of_close(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    if !state.tray.load(Ordering::SeqCst) || !state.background.load(Ordering::SeqCst) {
        return false;
    }
    if !state.told_about_tray.swap(true, Ordering::SeqCst) {
        let _ = app
            .notification()
            .builder()
            .title("framecorder is still syncing")
            .body(if cfg!(target_os = "macos") {
                "it's in the menu bar, so new clips keep coming in. to close it for real, click it there and pick quit, or turn off \"keep running\" in settings."
            } else {
                "it's in the tray, so new clips keep coming in. to close it for real, right-click it there and pick quit, or turn off \"keep running\" in settings."
            })
            .show();
    }
    true
}
