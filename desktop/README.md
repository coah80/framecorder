# framecorder desktop

the desktop app, native rust on [gpui-ce](https://github.com/gpui-ce/gpui-ce) instead of tauri and a webview. it runs the exact same sync core as the tauri app (`../app/src/core`, pulled in with `default-features = false` so none of tauri comes along), so pairing, the pinned tls, the event stream and resumable downloads are all the same code.

this is the new design from the redesign canvas: a sidebar with your frame and updates, a grid of clips by day, pairing on its own screen, settings.

## running it

```sh
cargo run --release
```

linux needs a few dev packages to build (`libxkbcommon-dev libxkbcommon-x11-dev libfontconfig-dev libfreetype-dev libwayland-dev`) and a vulkan driver to run. mac and windows need nothing extra. on a tag, ci makes `framecorder-desktop-linux`, `framecorder-setup.exe` (an installer, from `windows/installer.iss`: per user, no admin, start menu and desktop shortcuts, listed in installed apps) and `framecorder-desktop-macos.dmg`: a universal `framecorder.app` (apple silicon and intel) with the Info.plist from `macos/`, which has the local network keys macos needs before it lets the app find the frame. the site's download buttons point at these.

no frame around? the demo fills it with made up frames and clips, nothing syncs and nothing's saved:

```sh
cargo run -- --demo clips      # or list, syncing, unreachable, pair, settings
```

## what's the same as the tauri app

- same state folder (`~/.config/com.framecorder.app` on linux), so it picks up pairings you already have. don't run both at once, they'd both download everything
- clips land in `~/Videos/framecorder` (`~/Movies/framecorder` on a mac), clips in `clips/`
- a notification per new clip
- the tray icon (menu bar on a mac): open, sync now, quit. closing the window leaves it syncing there, the first time a notification says so. "keep running when closed" in settings turns that off, then closing quits
- "start with the computer" (`--minimized` starts it straight in the tray)
- frame updates, same button as before, now a link in the frame's card
- the same `prefs.json` as the tauri app, plus the grid or list choice

## what's different

- thumbnails come from `ffmpeg` when it's installed (one frame a second in, cached in `~/.cache/com.framecorder.app/thumbs`). gpui can't decode video, so without ffmpeg the grid shows plain tiles
- the tray on linux is a StatusNotifierItem over dbus (no gtk), which kde, and gnome with the appindicator extension, show. no tray means closing quits, same as turning keep running off
- it updates itself: it checks the latest github release for `framecorder-desktop-linux` / `framecorder-setup.exe` (ci builds them on a tag now), and the pill in the sidebar downloads it then restarts into it. on windows that's the installer, run silently, which closes the app, installs over it and starts it again. on a mac it opens the release page
- only one copy runs at a time. opening it again while it's running brings the running one's window up and leaves

## layout

- `main.rs` args, the tokio runtime, the window
- `sync.rs` the engine, and the bridge from its listener to the ui thread
- `app.rs` all the state and everything you can do
- `sidebar.rs`, `clips.rs`, `pair.rs`, `settings.rs` the screens
- `widgets.rs`, `theme.rs`, `assets.rs` buttons, colors, the fonts and icons (baked in)
- `tray.rs`, `prefs.rs`, `thumbs.rs`, `selfupdate.rs`, `autostart.rs`, `format.rs`, `demo.rs` the rest
