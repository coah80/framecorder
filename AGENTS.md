# AGENTS.md

Notes for anyone, person or agent, changing framecorder. Read this before you touch anything. `README.md` is the user-facing overview, `docs/how-it-works.md` the deep dive on the capture pipeline.

## Branches and PRs

- `dev` is where work happens. Every change goes to `dev`, as a PR or a push there
- `main` is what's released. The site deploys from it, and installs, updates and app downloads all come from its releases. Nothing goes on `main` except a release merged in from `dev`
- Pushing to `dev` changes nothing for anyone: no site deploy, no app build, no update reaches a headset
- Merge into `dev` with a merge commit or rebase (squash is fine for a single-author PR). Releases merge `dev` into `main` with a plain merge commit, never a squash, so every contributor's commits land on `main` with their author

## Project overview

framecorder records what the Steam Frame's panels actually show, with as little cost to the game as possible. It runs on the headset (SteamOS, arm64, Snapdragon 8 Gen 3 / Adreno 750) next to SteamVR.

The pieces:

| piece | where | what |
|---|---|---|
| recorder | `src/main.rs` + modules, binary `framecorder` | grabs the scanout buffer every vblank, undoes the lens in one compute shader, encodes on the hardware encoder, muxes with audio |
| dashboard tab | `src/ui/`, binary `framecorder-ui` | the tab in the SteamVR dashboard: record button, clips, settings, pairing. Starts and talks to the recorder |
| panel helper | `src/grab.rs`, binary `framecorder-grab` | the only part with a permission (`cap_sys_admin`). Turns a framebuffer id into a dmabuf for the recorder |
| setup and updater | `src/setup.rs`, binary `framecorder-setup` | installs a release, unlocks the panels (`--unlock`), checks for updates (`--check`) and installs them (`--update`, what the timer runs) |
| sync service | `sync/` (own crate), binary `framecorder-sync` | HTTPS + mDNS service on the headset that hands clips to paired devices |
| sync app | `app/` (own crate, Tauri 2) | desktop app (Windows, macOS, Linux) that pairs with the headset and downloads clips |
| native desktop app | `desktop/` (own crate, gpui-ce) | the desktop app rebuilt in native Rust, no Tauri or webview. Same sync core as `app/` (`app/src/core`, `default-features = false`) and the same state folder, so don't run both. Not shipped yet |
| installer | `installer/` (Bun + OpenTUI) | the terminal installer `site/install` downloads and runs |
| site | `site/` | framecorder.coah80.com, static, no build step |

Key tech: Rust everywhere on the headset, Vulkan (`ash`) for the compute shader, DRM/KMS (`drm`) for the scanout, V4L2 (`v4l2r`) for the encoder, FFmpeg (`ffmpeg-sys-next`) for AAC and muxing, PipeWire for audio, OpenVR through function tables (no bindings crate, see `src/openvr.rs`, `src/overlay.rs`, `src/input.rs`, `src/apps.rs`).

### How the recorder works, in one breath

Wait for vblank, find the plane the VR compositor scans out, export that framebuffer as a dmabuf (directly if the recorder has the permission, otherwise through `framecorder-grab`), import it into Vulkan as-is (UBWC compressed), run `shaders/convert.comp` to crop, undistort (per channel, with SteamVR's lens data from `ComputeDistortion`), undo the display cant, scale and convert to NV12 straight into the encoder's input buffer. HEVC or H.264 from the hardware encoder, AAC from PipeWire, muxed into MP4/MKV by a separate writer thread. Clips come from a replay buffer of already encoded packets, kept in segment files on disk.

## Setup

### On the headset (needs developer mode and ssh)

```sh
curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
git clone https://github.com/coah80/framecorder && cd framecorder && git switch dev
cargo build --release            # needs gcc, clang, glslc, ffmpeg, pipewire and vulkan headers, which SteamOS has
cargo test --release --lib
```

`./install.sh` builds and installs from source (older path, the installer is the normal one).

### Off the headset

The headset crate won't build on a stock desktop without FFmpeg, PipeWire, libdrm and glslc dev packages. Use the dev container, Debian trixie has FFmpeg 7.1 like SteamOS:

```sh
docker build -t framecorder-dev tools/dev
docker run --rm -v "$PWD":/src -v framecorder-target:/src/target -v framecorder-cargo:/root/.cargo/registry framecorder-dev cargo test
```

The sync service builds anywhere: `cd sync && cargo build --release`. The app needs WebKitGTK and friends on Linux (see `app/tools/Dockerfile`), or use CI.

The native desktop app (`desktop/`) needs `libxkbcommon-dev libxkbcommon-x11-dev libfontconfig-dev libfreetype-dev` and a Vulkan driver on Linux, nothing extra on macOS or Windows. Thumbnails need `ffmpeg` on the PATH, without it the grid shows plain tiles.

### Installer

```sh
cd installer && bun install --frozen-lockfile --cpu='*' --os='*'
bunx tsc -p .                    # typecheck
installer/build.sh dist          # compiles for arm64 Linux, zstd'd, with its sha256
```

`--cpu='*' --os='*'` pulls every platform's OpenTUI native package, so it can cross-compile for the headset from an x86 machine.

## Testing

| what | command | where |
|---|---|---|
| recorder, tab, setup, grab | `cargo test --release --lib` | headset or dev container |
| sync service | `cd sync && cargo test` | anywhere |
| app core | `cd app && cargo test` (includes `tests/pinned_tls.rs`) | Linux with the app's deps, or CI |
| sync end to end | `app/tools/e2e.sh` | one Linux box: real daemon against a temp HOME, the app headless as the client |
| installer | `cd installer && bunx tsc -p .` | anywhere |
| tab screens | `framecorder-ui --preview out.rgba <state>` | headset. States: `idle`, `recording`, `saved`, `toast`, `video`, `audio`, `clips`, `sync`, `pair`, `locked`, `relocked`, `setup-*`. Raw RGBA 1280x760: `ffmpeg -f rawvideo -pix_fmt rgba -s 1280x760 -i out.rgba out.png` |
| app screens | `app/tools/preview/shots.sh [dir]` | anywhere with Chrome, mocks the Tauri backend (`app/tools/preview/mock.js`) |
| native desktop app | `cd desktop && cargo test`, and `cargo run -- --demo <screen>` to look at it | anywhere with a display. Screens: `clips`, `list`, `syncing`, `unreachable`, `pair`, `settings`. The demo uses its own temp state, nothing real is touched |

Recording itself can only be tested on a headset: `framecorder --duration 4 --no-audio /tmp/t.mp4` and look at it (`ffprobe`, pull a frame with `ffmpeg -ss 1 -i /tmp/t.mp4 -frames:v 1 f.png`). The log line every 5 s says fps, dropped frames and GPU time; the `.perf.csv` next to the file has the details. Things that only show up under a real game (GPU contention, encoder falling behind) need someone in the headset playing something heavy.

Add or update tests for what you change. Keep tests offline and fast, no headset needed for `--lib`.

## Code style

- Rust 2021, `cargo fmt`. No `unwrap()` on anything that can fail at runtime, `anyhow` with context (`.context("reading the scanout plane")`) for errors
- Comments say why, in plain words, not what. Doc comments are full sentences. Match the density of the file you're in
- User-facing words (tab, installer, site, README, commit messages, log lines) are plain, friendly and mostly lowercase on the site and installer, sentence case in the tab. No jargon where a normal word works, no emoji
- Keep dependencies few. OpenVR is called through `FnTable:` interface tables with slot numbers from `openvr_capi.h`, not a bindings crate. Write the slot index as a named const with the interface version next to it
- Small files and modules by job. New headset features usually mean a new module in `src/` and a line in `lib.rs` or `main.rs`
- Never make the recorder heavier without measuring: `perf summary` in the log, or the `.perf.csv`

## Commits and PRs

- Conventional commits, lowercase: `feat:`, `fix:`, `docs:`, `chore:`, `perf:`, `refactor:`, `test:` (`feat(app):` for the sync app). The subject says what changed for someone using it
- The body explains why, with the numbers if there are any (fps before and after, ms of GPU, MB of RAM)
- PRs go to `dev`. Say what you tested and on what (headset model, SteamOS version, phone), and attach screenshots for anything visual
- Before opening one: `cargo test` for the crates you touched, `bunx tsc -p .` if you touched the installer

## Build, release and deploy

### Dev builds (trying `dev` on a headset)

```sh
packaging/dev.sh                 # needs gh, bun, and the headset over ssh (FRAME=frame by default)
```

Builds the headset release on the headset, the installer locally, and uploads both to the `dev-build` prerelease (it never counts as the latest release). Install it on a headset, in Konsole (desktop mode) or over ssh:

```sh
curl -fsSL https://raw.githubusercontent.com/coah80/framecorder/dev/site/install | FRAMECORDER_DL=https://github.com/coah80/framecorder/releases/download/dev-build sh
```

A headset installed that way stays on dev: its updates come from the dev build (`~/.local/share/framecorder/source` says where from). Going back to releases is the normal command, then update:

```sh
curl -fsSL https://framecorder.coah80.com/install | sh
```

### Releasing

1. Merge `dev` into `main` with a merge commit (`main` has its own short `AGENTS.md`, keep `main`'s on a conflict)
2. Bump the version in `Cargo.toml`, `sync/Cargo.toml`, `app/Cargo.toml` and `app/tauri.conf.json`, and the `Cargo.lock` next to each
3. Build the desktop apps: `gh workflow run app.yml --ref main`, then download the artifacts
4. On the headset: `packaging/release.sh` (makes `dist/framecorder-arm64.tar.gz` and its `.sha256`)
5. `gh release create vX.Y.Z --target main` with the headset tarball and checksum, the three desktop apps, and the Android APK

Publishing the release redeploys the site (`.github/workflows/site.yml`), which copies the release into `site/dl/` and builds the installer. That's what the install command and every headset's updater download from. If the site job didn't run, `gh workflow run site.yml --ref main`. Check it's live: `curl -fsSL https://framecorder.coah80.com/dl/framecorder-arm64.tar.gz.sha256`.

### CI

- `.github/workflows/site.yml`: on push to `main` touching `site/` or `installer/`, on a published (non-pre)release, or by hand. Builds the installer, copies the latest release's headset tarball into `site/dl`, deploys GitHub Pages. Pages only accepts `main` and `v*` tags
- `.github/workflows/app.yml`: on a `v*` tag or by hand. Builds the desktop app for macOS, Windows and Linux, attaches them to the tag's release

## What's installed on a headset

| path | what |
|---|---|
| `~/.local/bin/framecorder`, `framecorder-ui`, `framecorder-sync`, `framecorder-setup` | the programs, replaced by updates |
| `~/.local/lib/framecorder/framecorder-grab` | the panel helper, root owned, `cap_sys_admin+ep`, installed once by unlocking, never touched by updates |
| `~/.config/systemd/user/framecorder-{ui,sync,update}.service`, `framecorder-update.timer` | services: the tab starts with SteamVR, sync runs whenever the headset's on, the update timer every 6 hours |
| `~/.config/framecorder/ui.conf` | the tab's settings |
| `~/.config/framecorder/sync/` | the sync service's certificate, key and paired devices |
| `~/.local/share/framecorder/` | the unpacked release, `installed.sha256`, `source` (dev builds), `relock`, the SteamVR manifest and input bindings |
| `~/.local/state/framecorder/recorder.log` | the recorder's log (the tab's is in the journal: `journalctl --user -u framecorder-ui`) |
| `~/.local/share/applications/framecorder.desktop` | so it's in the app bar's "launch program" and desktop mode's menu |
| `~/Videos/framecorder/`, `clips/` | recordings and clips |

Remove everything but the videos: `framecorder-ui --uninstall`, or the installer's remove.

## Security

- Only `framecorder-grab` holds a permission, and it only exports scanout buffers of a `/dev/dri/card*` device, nothing else. Keep it that small. Updates never replace it, the installer does, with the user's password
- The sync service is HTTPS with a self-signed certificate that devices pin at pairing (no CA). Every route but `/hello` and `/pair` needs a paired device's bearer token. Anything that can start a recording or change settings over the network must stay behind that check and take only whitelisted values
- Never commit keys, keystores or tokens, or anything from a real headset's config (certificates, paired devices, calibration)
- Downloads (installer, release, updates) are checked against their sha256 before anything runs

## Gotchas (learned the hard way on the Frame)

**The display**
- The compositor draws every frame into one scanout buffer (front buffer rendering). Read it late and you get torn frames, or green UBWC garbage. The recorder's GPU queue is high priority for that reason, don't lower the default
- The displays are canted (~10.7° roll each way, ~5° yaw and pitch). SteamVR doesn't tell apps: `GetEyeToHeadTransform` is level, the compositor applies the cant itself from the headset's factory calibration (`~/.config/openvr/config/cv/*/config.json`, `tracking_to_eye_transform[eye].eye_to_head`, what `vrcmd --info` calls the compositor residual). `src/lut.rs` undoes it. The calibration's x and y point the other way from ours
- The hidden area mesh and render target are the game's (level) space, the distortion is the compositor's (canted) space. Check visibility in the right one, or rotated corners come out black
- The lens shows more up than down, so center the video ~16° above straight ahead, like SteamVR's own headset view

**SteamVR**
- Never start SteamVR from outside its own launcher. Connecting as an overlay app can launch `vrserver`, and a server started that way leaves the headset stuck in passthrough. The tab waits for SteamVR instead
- framecorder is registered as `coah80.framecorder` (`src/apps.rs`). SteamVR counts every `framecorder-ui` process as that app, and one leaving takes the other's dashboard tab with it. A second launch must never connect to SteamVR: the lock in `src/ui/mod.rs` makes it signal the running one to show its tab and leave
- In a `.vrmanifest` on this headset the binary key is `binary_path_linux_arm` (SteamVR skips the app without it)
- Overlay error 17 is `KeyInUse`
- The app bar's "launch program" lists desktop entries, not SteamVR apps. That's what `framecorder.desktop` is for

**SteamOS**
- Desktop mode is a nested Plasma session with its own `XDG_RUNTIME_DIR` and session bus. `systemctl --user` from Konsole there can't find the user's systemd unless pointed at `/run/user/<uid>` (`use_user_manager` in `src/setup.rs`)
- A permission lives on a file, and replacing the file drops it. That's why the permission is on the helper, not the recorder
- There's no polkit rule that allows `setcap` without a password, and SteamOS has no password until the user sets one. The installer asks for it (and helps set one)
- `/home` survives SteamOS updates. `/usr` is read-only, `/var` is per OS slot
- Starting a game can restart PipeWire. Anything holding a PipeWire stream has to notice and reconnect (`src/audio.rs`)
- Memory is tight and swap is compressed. Big buffers (the clip buffer, files being written) go to disk at the disk's pace, not into RAM, or PipeWire's realtime thread gets swapped out and killed
- The headset doesn't answer ping, check port 22 instead

**Tooling**
- `pkill -f <pattern>` over ssh can match the ssh command's own shell and kill it. Use `pgrep` with a `[b]racket` trick or keep the PID
- `gh release` in scripts: give it `</dev/null` or it can sit waiting for input
- `bun build --compile` straight into `/tmp` produced a broken binary once, build into the project's `dist/`
- Frame Drop and the flatpak are gone on purpose, don't bring them back: neither can ask for the password the panels need
