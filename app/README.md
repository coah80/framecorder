# framecorder app

Gets clips and recordings off your Steam Frame over Wi-Fi, onto your desktop
(Linux, Windows, macOS). It talks to `framecorder-sync` on the headset (see
`../sync`). The Android app is native, in `../android`; it runs the same sync
core from `src/core` through UniFFI.

**It only syncs while the Frame is on (awake), on the same Wi-Fi, and
framecorder is running on it.** If the app is closed, clips sync the next
time it's open and the Frame is reachable. The app says this on the main
screen, in pairing, and loudly whenever it can't reach the Frame.

## What it does

- Finds Frames with mDNS (`_framecorder._tcp`) and, for networks that drop
  multicast, by asking every address in the local /24 on port 38619
  (`discover::look`); or takes an address.
- Pairs with the 6-digit code from "pair a device" on the Frame. Pick the
  Frame from the list and type the code.
- Pins the Frame's certificate: TLS only succeeds against the SHA-256 we got
  from the QR code / mDNS at pairing time, no CA store involved. The server
  still has to prove it holds the key (handshake signatures are verified).
- Keeps an event stream open (SSE). New clip on the Frame -> download starts
  within a few ms. Reconnects with backoff (2 s up to 30 s, "try again" skips
  it); if the Frame's address changed it finds it again by fingerprint.
- On every (re)connect it diffs `GET /clips` against a local index, so
  nothing is missed and nothing downloads twice. Clips go first, newest first.
- Downloads go to a `.part` file and resume with `Range` after a drop or a
  kill, then get renamed into place. Existing files are never overwritten
  (`name (2).mp4`).
- If "delete after sync" is on for the Frame, it asks the Frame to delete a
  clip once it's safely here (the Frame refuses otherwise).

It has a tray icon (closing the window keeps syncing; tray menu has open /
sync now / quit), optional start-with-the-computer (starts hidden in the
tray), a notification per new clip, gallery with open / show in folder and a
thumbnail from the video itself when the webview can decode it. Files land in
`~/Videos/framecorder` (`~/Movies/framecorder` on macOS), clips in `clips/`.

## Layout

- `src/core/`: everything that isn't UI, no Tauri in it.
  `api.rs` client + SSE parser + resumable download, `tls.rs` the pinned
  verifier, `discover.rs` mDNS, `store.rs` paired hosts + index, `engine.rs`
  the sync loop, `pairlink.rs` the QR link.
- `src/headless.rs`: `--headless-sync`, the engine with no window.
- `src/gui/`: Tauri commands, tray, notifications, platform glue.
- `ui/`: plain HTML/CSS/JS, no build step (Catppuccin Mocha, Space Grotesk +
  Poppins, both OFL, licenses in `ui/fonts/`).

State (paired Frames, tokens, index) lives in the app config dir,
`com.framecorder.app` (e.g. `~/.config/com.framecorder.app/`), files 0600.

## Build

Desktop builds need webkit2gtk etc., so they run in a container
(`tools/Dockerfile`: Debian bookworm, Rust, webkit2gtk-4.1, appindicator,
clang/lld for cargo-xwin, tauri-cli):

```sh
docker build -t framecorder-app-dev app/tools
# run from the repo root; target/docker keeps it apart from host builds
alias fcdev='docker run --rm --user "$(id -u):$(id -g)" -e CARGO_HOME=/cargo -e HOME=/tmp \
  -e XWIN_CACHE_DIR=/cargo/xwin -e XWIN_ACCEPT_LICENSE=1 -e CARGO_TARGET_DIR=/proj/app/target/docker \
  -v "$HOME/.cache/fc-docker-cargo:/cargo" -v "$PWD:/proj" -w /proj/app framecorder-app-dev'

# Linux: bare binary, or .deb/.AppImage with `cargo tauri build`
fcdev cargo build --release            # -> app/target/docker/release/framecorder-app

# Windows (x86_64-pc-windows-msvc via cargo-xwin, WebView2 loader is static)
fcdev cargo xwin build --release --target x86_64-pc-windows-msvc
# -> app/target/docker/x86_64-pc-windows-msvc/release/framecorder-app.exe
# installer: fcdev cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
```

macOS needs a Mac, so it's built by GitHub Actions (`.github/workflows/app.yml`,
which also builds windows and linux). push a `v*` tag and the dmg lands on
that release as `framecorder-macos.dmg`. it's a universal build (apple
silicon + intel, macOS 11+), ad-hoc signed, so the first open needs system
settings, privacy & security, open anyway. add the `APPLE_*` secrets listed in
the workflow and it gets signed and notarized for real instead. on a Mac by
hand: `cargo tauri build --target universal-apple-darwin --bundles dmg`.
downloads go to `~/Movies/framecorder`, and `Info.plist` has the text macOS
shows when it asks for local network access (needed to find the Frame).

Only the sync core, no webkit needed (runs anywhere Rust does):

```sh
cargo test --no-default-features
cargo run --no-default-features -- --headless-sync ~/Videos/framecorder --pair 'framecorder://pair?...'
```

## Headless mode

```
framecorder-app --headless-sync <dir> [--state <dir>] [--pair <link> | --host <ip:port> --code <123456> [--fp <sha256>]] [--exit-after <secs>]
framecorder-app --discover [--seconds <n>]
```

Pairs if asked to, then syncs into `<dir>` until killed. Without `--fp`,
`--host` learns the fingerprint on first contact and pins it from then on.

## Tests

- `cargo test --no-default-features`: pairing link parsing, fingerprint
  normalisation, the pinned verifier, index diff/ordering and persistence,
  SSE parsing, Content-Range, safe file names, no-overwrite finishing, plus
  `tests/pinned_tls.rs` against a real rustls server (right fingerprint
  connects, wrong one fails before any HTTP is sent, capture mode).
- `tools/preview/shots.sh [dir]`: screenshots of every screen in headless chrome, with tauri mocked. `python3 tools/preview/serve.py` to click around in a browser instead, `?s=connected|syncing|unreachable|full|forgot|empty|pair|pair-found|settings`
- `tools/e2e.sh` (from the repo root): real daemon on a temp HOME + headless
  app. Wrong fingerprint refused (code not used up), pairing, catch-up of an
  existing file, `.part` -> `.mp4` rename synced in ~25 ms, a download killed
  mid-way while the fake SteamVR says a game is running resumes via Range.

## Known gaps

- Pairing a Frame picked from mDNS trusts the network during pairing (the QR
  code doesn't have that problem, it carries the fingerprint).
- Tokens are stored in the app's config dir, not the OS keychain.
- HEVC thumbnails depend on the platform: WebView2 needs the HEVC extension,
  WebKitGTK needs a GStreamer HEVC decoder. Without one you get a placeholder.
