# framecorder desktop

the desktop app, native rust on [gpui-ce](https://github.com/gpui-ce/gpui-ce) instead of tauri and a webview. it runs the exact same sync core as the tauri app (`../app/src/core`, pulled in with `default-features = false` so none of tauri comes along), so pairing, the pinned tls, the event stream and resumable downloads are all the same code.

this is the new design from the redesign canvas: a sidebar with your frame and updates, a grid of clips by day, pairing on its own screen, settings.

## running it

```sh
cargo run --release
```

linux needs a few dev packages to build (`libxkbcommon-dev libxkbcommon-x11-dev libfontconfig-dev libfreetype-dev`, plus x11/wayland headers on a minimal box) and a vulkan driver to run. mac and windows need nothing extra.

no frame around? the demo fills it with made up frames and clips, nothing syncs and nothing's saved:

```sh
cargo run -- --demo clips      # or list, syncing, unreachable, pair, settings
```

## what's the same as the tauri app

- same state folder (`~/.config/com.framecorder.app` on linux), so it picks up pairings you already have. don't run both at once, they'd both download everything
- clips land in `~/Videos/framecorder` (`~/Movies/framecorder` on a mac), clips in `clips/`
- a notification per new clip
- "start with the computer"
- frame updates, same button as before, now a link in the frame's card

## what's different

- thumbnails come from `ffmpeg` when it's installed (one frame a second in, cached in `~/.cache/com.framecorder.app/thumbs`). gpui can't decode video, so without ffmpeg the grid shows plain tiles
- no tray icon yet, gpui doesn't have one. closing the window quits, clips catch up next time it's open
- it updates itself: it checks the latest github release for `framecorder-desktop-linux` / `framecorder-desktop-windows.exe`, and the pill in the sidebar downloads it then restarts into it. on a mac it opens the release page. nothing's offered until a release actually has those files
- only one copy runs at a time (a lock file in the state folder). opening it again while it's running just says so, it can't bring the window forward yet

## layout

- `main.rs` args, the tokio runtime, the window
- `sync.rs` the engine, and the bridge from its listener to the ui thread
- `app.rs` all the state and everything you can do
- `sidebar.rs`, `clips.rs`, `pair.rs`, `settings.rs` the screens
- `widgets.rs`, `theme.rs`, `assets.rs` buttons, colors, the fonts and icons (baked in)
- `thumbs.rs`, `selfupdate.rs`, `autostart.rs`, `format.rs`, `demo.rs` the rest
