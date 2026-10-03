# framecorder for Android

The phone side of framecorder: gets clips and recordings off your Steam Frame
over Wi-Fi, into the phone's gallery. Native Jetpack Compose with Material 3
Expressive, on top of the same Rust sync core the desktop app runs
(`../app/src/core`), through UniFFI.

**It only syncs while the Frame is on, on the same Wi-Fi, with framecorder
running on it.** Anything saved meanwhile waits on the Frame and catches up.

## What it does

- **Finding the Frame**, three ways at once, so a router that drops multicast
  doesn't stop it:
  - Android's own mDNS (`NsdManager`), which needs no multicast lock and keeps
    working in the background.
  - A sweep of the local /24 on port 38619 that says hello to whatever opens,
    and only counts a framecorder-sync whose certificate matches the
    fingerprint it claims (`discover::sweep` in the shared core, so desktop
    gets it too).
  - Its address typed in, or the `framecorder://pair?...` link pasted.
- **Pairing**, all in the frame tab: with nothing paired the tab is the
  pairing screen. Scan the QR code (Google's code scanner, so no camera
  permission), or pick the Frame and type its code: it pairs on the sixth
  digit. The QR link also opens the app straight from the camera app (deep
  link). Android 17 asks for the local network permission first. Pairing
  a Frame again after it got a new certificate (a reinstall) keeps
  everything it already sent: nothing downloads twice.
- **Syncing**: the core keeps an event stream open, downloads new clips within
  a moment of them being saved, resumes with `Range` after a drop or a kill,
  and hands finished files to MediaStore (`Movies/framecorder`, clips in
  `/clips`). A foreground service (`dataSync`) keeps it going with the app
  closed, with a partial wake lock only while a download runs.
- **Library**: just the clips, laid out like Google Photos. Square
  thumbnails the phone makes itself, packed tight in rounded blocks by day,
  with only the length in the corner (and a red dot for recordings). Two
  small chips filter clips and recordings, plus search, the current
  download while one runs (wavy progress, size, data rate), and pull to
  retry. Scrolled, a frosted pill says which day you're on, and a fast
  scroller notched by day shows up at the side. A Frame that can't be
  reached shows as a small pill that leads to the frame tab.
- **Frame tab**: the Frame's name and state, with its battery (the
  percentage inside the icon, like the phone's own) and free space as
  chips; scrolled, that becomes a frosted pill in the middle up top with
  the battery, or the recording's timer. Once a page scrolls, what goes
  under the status bar blurs progressively (Haze) instead of ending at a
  hard edge. Then the remote (record and stop
  with a running timer, save clip, the mic), the settings for its next
  recording (shape, clip length, quality, frame rate, game audio and mic,
  saved on the headset through framecorder-sync), what's on this phone from
  it, the connection check, try again, unpair and pair another.
- **Outside the app**: while a Frame records, the syncing notification
  counts along with a stop button, and offers save clip otherwise. Quick
  settings tiles for save clip and record.
- **Player**: Media3, with resolution, frame rate, codec and shape read from
  the file; share, open elsewhere, delete. Opening a clip grows its tile
  into the player (a shared element, corners and all), back shrinks it home.
  Swiping goes through the clips in the library's order and filter, and back
  brings the last one's tile into view first. Play at the end plays it again.
  **Trim** shows a
  strip of frames with a handle at each end, loops the part that's kept, and
  shares just that, cut with Media3 Transformer (only the first moments get
  re-encoded).
- **Connection check**: runs the real checks one by one (Wi-Fi, permission,
  the last address with its certificate, mDNS, the sweep) and says what each
  one saw.
- While syncing, a notification with a segment per file (Android 16's
  progress style).
- A notification per clip that lands, with its picture, share and open; wallpaper
  colors on Android 12+; a vector adaptive icon with a themed (monochrome)
  layer.

## Layout

- `rust/ffi`: the UniFFI face of the sync core: a `Core` object, plain
  records, and three things Kotlin implements (`Gallery` for MediaStore,
  `Events`, and `Finder` for NSD).
- `rust/bindgen`: the `uniffi-bindgen` binary that writes the Kotlin side.
- `app/src/main/kotlin/com/framecorder/app/`
  - `sync/`: `SyncHub` (owns the core, turns its events into `StateFlow`s,
    works out the data rate), `MediaGallery`, `NsdFinder`, `SyncService`,
    `Notifier`.
  - `ui/`: one folder per screen; `App.kt` is the Navigation 3 back stack,
    the floating nav and the shared element scope; `common/` has the icons
    (the design's own stroke paths), shapes and rows; `theme/` the colors and
    the type (Montserrat, Poppins, Space Grotesk, all OFL).

State (paired Frames, tokens, what's been synced) lives in the app's files
dir under `sync/`. It isn't carried over from the Tauri app, so pair once
after updating.

## Build

Needs the Android SDK (platform 37), the NDK, a JDK 17+, Rust with
`rustup target add aarch64-linux-android x86_64-linux-android`, and
`cargo install cargo-ndk`.

```sh
cd android
./gradlew assembleDebug        # -> app/build/outputs/apk/debug/app-debug.apk
./gradlew installDebug         # onto a phone or emulator
./gradlew assembleRelease      # -> app/build/outputs/apk/release/app-release.apk
```

Gradle builds the core with cargo-ndk (arm64 for phones, x86_64 for the
emulator) and writes the Kotlin bindings from a host build before compiling
anything. `-PskipRust` reuses what's already in `app/build`.

Release builds are signed with `keystore.properties` (`storeFile`,
`storePassword`, `keyAlias`, `keyPassword`) when there is one, otherwise with
the debug key, like the old app's "debugsigned" release.

Material 3 Expressive (button groups, floating toolbars, wavy progress,
shapes) only ships in the material3 1.5 alphas so far, so that one
dependency is an alpha; the rest is stable.

## Trying it without a Frame

`framecorder-sync` runs on any Linux, WSL included. The emulator reaches the
computer at `10.0.2.2`, which the sweep finds by itself:

```sh
cd sync && cargo build --release
mkdir -p /tmp/fc/Videos/framecorder/clips /tmp/fc/.config/framecorder/sync
HOME=/tmp/fc ./target/release/framecorder-sync --no-mdns &
# a pairing code, the way the headset tab writes one
echo "{\"code\":\"123456\",\"expires\":$(( $(date +%s) + 600 ))}" > /tmp/fc/.config/framecorder/sync/pairing.json
# a new clip, the way the recorder saves one
cp some.mp4 /tmp/fc/Videos/framecorder/clips/a.mp4.part && mv /tmp/fc/Videos/framecorder/clips/a.mp4.part /tmp/fc/Videos/framecorder/clips/a.mp4
```

## Not yet

- Thumbnails made on the headset, and which game a clip is from.
- Deleting from the Frame only works once the headset's
  `delete_after_sync` gets set from somewhere; nothing writes it yet.
