//! The Tauri app around the sync core: window, tray, notifications.

mod commands;
mod platform;
mod prefs;
mod tray;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::Manager;

use crate::core::engine::Engine;

pub struct AppState {
    pub engine: Arc<Engine>,
    /// Where clips land.
    pub download_dir: PathBuf,
    pub tray: AtomicBool,
    pub told_about_tray: AtomicBool,
    /// Closing the window leaves it in the tray, or quits.
    pub background: AtomicBool,
    pub config_dir: PathBuf,
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::overview,
            commands::discover,
            commands::pair,
            commands::unpair,
            commands::retry_now,
            commands::start_update,
            commands::open_clip,
            commands::reveal_clip,
            commands::open_folder,
            commands::set_autostart,
            commands::set_background,
            commands::quit,
        ])
        .setup(|app| {
            setup(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // closing the window keeps syncing from the tray, if there is one
                if tray::hide_instead_of_close(window.app_handle()) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("couldn't start framecorder");
    app.run(|_app, event| {
        // mac: clicking the dock icon while the window's hidden in the menu bar
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event {
            show_main(_app);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = event;
    });
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init();
    let handle = app.handle().clone();
    let state_dir = app.path().app_config_dir()?;
    std::fs::create_dir_all(&state_dir)?;

    let (sink, download_dir) = platform::sink()?;
    // lets the gallery load a frame of each clip for its thumbnail
    app.asset_protocol_scope().allow_directory(&download_dir, true)?;
    let listener = Arc::new(platform::UiListener::new(handle.clone()));
    let device_name = crate::core::device_name();
    let rt = tauri::async_runtime::handle().inner().clone();
    let engine = Engine::new(rt, &state_dir, sink, listener, &device_name);
    let background = prefs::load(&state_dir).background;
    app.manage(AppState {
        engine: engine.clone(),
        download_dir,
        tray: AtomicBool::new(false),
        told_about_tray: AtomicBool::new(false),
        background: AtomicBool::new(background),
        config_dir: state_dir.clone(),
    });
    engine.start_all();

    let ok = match tray::create(&handle) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("no tray icon, closing the window will quit: {e}");
            false
        }
    };
    app.state::<AppState>().tray.store(ok, std::sync::atomic::Ordering::SeqCst);
    tray::set_visible(&handle, background);
    // started with the computer: straight to the tray, if it's staying there
    let minimized = std::env::args().any(|a| a == "--minimized");
    if !(minimized && ok && background) {
        show_main(&handle);
    }
    Ok(())
}

pub fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
