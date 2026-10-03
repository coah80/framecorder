//! framecorder's desktop app, native on gpui-ce. the sync engine is the same
//! one the tauri app and the android app use, from app/src/core.

// a windowed program on windows, so no console window opens next to it (debug
// builds keep the console, for the log)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod appimage;
mod assets;
mod autostart;
mod clips;
mod demo;
mod format;
mod frame;
mod oldapp;
mod pair;
mod prefs;
mod remote;
mod search;
mod selfupdate;
mod settings;
mod sidebar;
mod sync;
mod theme;
mod thumbs;
mod tray;
mod widgets;

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    px, size, AnyWindowHandle, App, AppContext, Bounds, Entity, Global, QuitMode, TitlebarOptions, WindowBounds,
    WindowOptions,
};

use app::FrameApp;
use tray::TrayMsg;

const USAGE: &str = "\
usage:
  framecorder-desktop                  start the app
  framecorder-desktop --minimized      start in the tray, if it's set to keep running
  framecorder-desktop --demo <screen>  made up frames and clips, nothing syncs.
                                       screens: clips, list, syncing, unreachable, pair, settings,
                                       frame, frame-recording, frame-paused, frame-old, frame-closed,
                                       frame-unreachable";

/// the one app, and its window when it has one. the window can close and
/// come back from the tray, the app stays
struct Main {
    app: Entity<FrameApp>,
    window: Option<AnyWindowHandle>,
    icon: Option<Arc<image::RgbaImage>>,
}

impl Global for Main {}

fn init_log() {
    let mut log = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,naga=warn,wgpu=warn,tracing=warn"),
    );
    // a windowed program on windows has nowhere to print, so the log goes in a
    // file next to the state. appended, since a second launch writes here too
    // before it hands over to the first, and started over once it's big
    #[cfg(all(windows, not(debug_assertions)))]
    {
        let path = sync::state_dir().join("desktop.log");
        let _ = std::fs::create_dir_all(sync::state_dir());
        let big = std::fs::metadata(&path).is_ok_and(|m| m.len() > 2_000_000);
        let file = std::fs::OpenOptions::new().create(true).append(!big).write(true).truncate(big).open(&path);
        if let Ok(file) = file {
            log.target(env_logger::Target::Pipe(Box::new(file)));
        }
    }
    log.init();
}

fn main() {
    init_log();

    let mut demo: Option<String> = None;
    let mut minimized = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => demo = Some(args.next().unwrap_or_else(|| "clips".into())),
            "--minimized" => minimized = true,
            // the old version starting us after an update, nothing to do
            "--after-update" => {}
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => {
                eprintln!("unknown argument {other}\n{USAGE}");
                std::process::exit(2);
            }
        }
    }
    if let Some(screen) = &demo {
        if !demo::SCREENS.contains(&screen.as_str()) {
            eprintln!("no demo screen called {screen}\n{USAGE}");
            std::process::exit(2);
        }
    }

    let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("couldn't start: {e}");
            std::process::exit(1);
        }
    };

    // only one of us, the second launch just brings the first one up. this
    // comes before the engine starts, so the second copy never syncs anything
    let state_dir = if demo.is_some() { std::env::temp_dir().join("framecorder-demo") } else { sync::state_dir() };
    let _ = std::fs::create_dir_all(&state_dir);
    let _lock = if demo.is_none() {
        match sync::single_instance(&state_dir) {
            Ok(lock) => Some(lock),
            Err(e) => {
                eprintln!("{e}");
                return;
            }
        }
    } else {
        None
    };
    // "start with the computer" goes by the same name as the tauri app's, so
    // after switching it can still start the old app. pointing it here means
    // only this one comes up after the next login
    if demo.is_none() && autostart::is_enabled() == Some(true) {
        if let Err(e) = autostart::set(true) {
            log::warn!("couldn't point start with the computer at this app: {e}");
        }
    }
    // the old app syncs from the same state, so it goes before we start
    let closed_old = demo.is_none() && oldapp::close();
    #[cfg(target_os = "linux")]
    if demo.is_none() {
        appimage::integrate(assets::APP_ICON_PNG);
    }
    let (core, rx) = match sync::start(rt.handle().clone(), demo.is_some()) {
        Ok(started) => started,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let state_dir = core.state_dir.clone();
    let is_demo = demo.is_some();

    gpui_platform::application().with_assets(assets::Assets).run(move |cx: &mut App| {
        assets::load_fonts(cx);
        let icon = assets::app_icon().map(Arc::new);

        let app = cx.new(|cx| {
            let mut app = FrameApp::new(core, rx, is_demo, cx);
            if let Some(screen) = &demo {
                demo::seed(&mut app, screen);
            }
            if closed_old {
                app.toast("the old framecorder app was still running, so it's closed now. this one does it all", cx);
            }
            app
        });
        cx.set_global(Main { app: app.clone(), window: None, icon: icon.clone() });

        // the tray, unless this is a demo. it needs the window system up, so after the app
        let mut has_tray = false;
        if !is_demo {
            if let Some(icon) = &icon {
                let (tx, trx) = async_channel::unbounded::<TrayMsg>();
                match tray::create(icon, tx) {
                    Ok(t) => {
                        has_tray = true;
                        app.update(cx, |app, _| app.attach_tray(t));
                        cx.spawn(async move |cx| {
                            while let Ok(msg) = trx.recv().await {
                                cx.update(|cx| match msg {
                                    TrayMsg::Open => open_main(cx),
                                    TrayMsg::SyncNow => cx.global::<Main>().app.read(cx).core.engine.retry_now(),
                                    TrayMsg::Quit => cx.quit(),
                                });
                            }
                        })
                        .detach();
                    }
                    Err(e) => log::warn!("no tray icon, closing the window will quit: {e}"),
                }
            }
            // a second launch leaves a note, we answer by showing the window
            let marker = sync::show_marker(&state_dir);
            cx.spawn(async move |cx| loop {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                if marker.exists() {
                    let _ = std::fs::remove_file(&marker);
                    cx.update(open_main);
                }
            })
            .detach();
        }

        // gpui quits by itself when the last window goes, we decide that ourselves
        cx.set_quit_mode(QuitMode::Explicit);
        cx.on_window_closed(|cx, id| {
            let main = cx.global_mut::<Main>();
            if main.window.is_some_and(|w| w.window_id() == id) {
                main.window = None;
            }
            if cx.windows().is_empty() {
                let keep = cx.global::<Main>().app.clone();
                let keep = keep.update(cx, |app, cx| app.keep_running_on_close(cx));
                if !keep {
                    cx.quit();
                }
            }
        })
        .detach();

        let background = app.read(cx).background;
        if !(minimized && has_tray && background) {
            open_main(cx);
        }
    });
    rt.shutdown_background();
}

/// shows the window: brings it up if there is one, else opens it
fn open_main(cx: &mut App) {
    let (existing, app, icon) = {
        let main = cx.global::<Main>();
        (main.window, main.app.clone(), main.icon.clone())
    };
    log::info!("showing the window");
    if let Some(handle) = existing {
        if handle
            .update(cx, |_, window, _| {
                window.activate_window();
            })
            .is_ok()
        {
            return;
        }
    }
    let bounds = Bounds::centered(None, size(px(920.), px(680.)), cx);
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some("framecorder".into()), ..Default::default() }),
            app_id: Some("com.framecorder.desktop".into()),
            window_min_size: Some(size(px(720.), px(520.))),
            icon,
            ..Default::default()
        },
        |_, _| app,
    );
    match opened {
        Ok(handle) => {
            log::info!("opened a new window");
            cx.global_mut::<Main>().window = Some(handle.into());
            cx.activate(true);
        }
        Err(e) => {
            log::error!("couldn't open a window: {e}");
            if cx.windows().is_empty() {
                cx.quit();
            }
        }
    }
}
