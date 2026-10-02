//! framecorder's desktop app, native on gpui-ce. the sync engine is the same
//! one the tauri app and the android app use, from app/src/core.

mod app;
mod assets;
mod autostart;
mod clips;
mod demo;
mod format;
mod pair;
mod selfupdate;
mod settings;
mod sidebar;
mod sync;
mod theme;
mod thumbs;
mod widgets;

use gpui::{px, size, App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions};

const USAGE: &str = "\
usage:
  framecorder-desktop                  start the app
  framecorder-desktop --demo <screen>  made up frames and clips, nothing syncs.
                                       screens: clips, list, syncing, unreachable, pair, settings";

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,naga=warn,wgpu=warn,tracing=warn"),
    )
    .init();

    let mut demo: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => demo = Some(args.next().unwrap_or_else(|| "clips".into())),
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

    // a demo gets its own empty state, so it can't touch real pairings
    let state_dir = match &demo {
        Some(_) => std::env::temp_dir().join("framecorder-demo"),
        None => sync::state_dir(),
    };
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

    let (core, rx) = match sync::start(rt.handle().clone(), demo.is_some()) {
        Ok(started) => started,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    gpui_platform::application().with_assets(assets::Assets).run(move |cx: &mut App| {
        assets::load_fonts(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(920.), px(680.)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions { title: Some("framecorder".into()), ..Default::default() }),
                app_id: Some("com.framecorder.desktop".into()),
                window_min_size: Some(size(px(720.), px(520.))),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| {
                    let mut app = app::FrameApp::new(core, rx, demo.is_some(), cx);
                    if let Some(screen) = &demo {
                        demo::seed(&mut app, screen);
                    }
                    app
                })
            },
        );
        if let Err(e) = opened {
            eprintln!("couldn't open a window: {e}");
            cx.quit();
        }
        cx.activate(true);
    });
    rt.shutdown_background();
}
