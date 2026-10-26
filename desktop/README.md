# framecorder desktop

the desktop app, native rust on [gpui-ce](https://github.com/gpui-ce/gpui-ce). it runs the same sync core as the android app (`../app/src/core`), so pairing, the pinned tls, the event stream and resumable downloads are all the same code.

this is the new design from the redesign canvas: a sidebar with your frame and updates, a grid of clips by day, pairing on its own screen, settings.

## running it

```sh
cargo run --release
```

linux needs a few dev packages to build (`libxkbcommon-dev libxkbcommon-x11-dev libfontconfig-dev libfreetype-dev libwayland-dev`) and a vulkan driver to run. mac and windows need nothing extra. on a tag, ci makes `framecorder-x86_64.AppImage` (built on ubuntu 22.04 for older glibcs, run on fedora, arch, debian, ubuntu and opensuse; on first run it adds itself to the app menu), `framecorder-setup.exe` (an installer, from `windows/installer.iss`: per user, no admin, start menu and desktop shortcuts, listed in installed apps) and `framecorder-desktop-macos.dmg`: a universal `framecorder.app` (apple silicon and intel) with the Info.plist from `macos/`, which has the local network keys macos needs before it lets the app find the frame. the site's download buttons point at these.

no frame around? the demo fills it with made up frames and clips, nothing syncs and nothing's saved:

```sh
cargo run -- --demo clips      # `--help` lists the screens, demo.rs makes them
```

## what it does

- clips land in `~/Videos/framecorder` (`~/Movies/framecorder` on a mac), clips in `clips/`. a clip in the library opens in the system's player, shows in its folder, or goes to the trash (the recycle bin on windows)
- several at once, like the android app: ctrl-click (cmd on a mac) picks a clip, shift-click the range from the last one clicked, and a drag across the library picks everything it crosses, scrolling at the edges. while anything's picked a plain click picks or lets go instead of opening, ctrl+a picks everything on screen and esc lets go. the bar at the bottom says "3 selected · 195 MB", shows them in their folder, or moves them to the trash after asking once
- a notification per new clip, with open and show in folder. settings can turn them off
- pairing finds the frame by name (mdns) and by knocking on every nearby address, for routers that drop multicast, and takes its address typed in or a `framecorder://pair` link pasted. it pairs on the sixth digit
- the frame page: record, stop, save clip and the mic, how it's doing (battery, storage, a download coming in), the settings for its next recording, what's here from it, and a connection check that runs the real checks one by one and says what each saw
- the tray icon (menu bar on a mac): open, sync now, quit. closing the window leaves it syncing there, the first time a notification says so. "keep running when closed" in settings turns that off, then closing quits. the tray on linux is a StatusNotifierItem over dbus (no gtk), which kde, and gnome with the appindicator extension, show. no tray means closing quits, same as turning keep running off
- "start with the computer" (`--minimized` starts it straight in the tray)
- frame updates, a link in the frame's card
- thumbnails come from `ffmpeg` when it's installed (one frame a second in, cached in `~/.cache/com.framecorder.app/thumbs`). gpui can't decode video, so without ffmpeg the grid shows plain tiles
- it updates itself: it checks the latest github release for `framecorder-x86_64.AppImage` / `framecorder-setup.exe` / `framecorder-desktop-macos.dmg`, and the pill in the sidebar downloads it then restarts into it. on windows that's the installer, run silently, which closes the app, installs over it and starts it again. on linux the new AppImage is renamed over the old one (only from an AppImage, a build of your own doesn't update). on a mac the new `framecorder.app` is copied out of the dmg and swapped in for the old one, which reopens once it has quit (only from a `framecorder.app`)
- only one copy runs at a time. opening it again while it's running brings the running one's window up and leaves

## coming from the old tauri app

the old 0.1.x tauri app can't update itself. download the new app once from [the site](https://framecorder.coah80.com/) and open or install it. your pairings and settings carry over, and the new app closes the old one for you:

- same state folder (`~/.config/com.framecorder.app` on linux) and the same `prefs.json`, so pairings and settings carry over
- "start with the computer" goes by the old app's name, so if it was on, it gets pointed here on start
- if the old app is running, it gets closed on start (`oldapp.rs`), otherwise both would download everything

## layout

- `main.rs` args, the tokio runtime, the window
- `sync.rs` the engine, and the bridge from its listener to the ui thread
- `app.rs` all the state and everything you can do
- `sidebar.rs`, `clips.rs`, `frame.rs`, `check.rs`, `pair.rs`, `settings.rs` the screens; `remote.rs` what a frame's tab is up to
- `widgets.rs`, `theme.rs`, `assets.rs` buttons, colors, the fonts and icons (baked in)
- `tray.rs`, `prefs.rs`, `thumbs.rs`, `selfupdate.rs`, `autostart.rs`, `oldapp.rs`, `format.rs`, `demo.rs` the rest
