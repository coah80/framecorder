# framecorder

framecorder records what the Steam Frame's panels actually show, without taking frames from the game. It runs on the headset next to SteamVR: a recorder that reads the display's scanout buffer, a tab in the SteamVR dashboard to drive it, and a sync service that hands clips to a desktop or phone app on the same Wi-Fi. It's free, open source and donationware.

## Before you start

1. Work on `dev`. `main` only takes releases (see [Branches](#branches)).
2. Read [the five ways to hurt yourself](#the-five-ways-to-hurt-yourself). Most of them are about a real person's headset.
3. Find the surfaces your change touches in [Hit every surface](#hit-every-surface) before you write code, not after.
4. Before you call it done, [clean up after yourself](#clean-up-after-yourself).

## What we never compromise on

### 1. The game comes first

framecorder runs on a phone chip next to games that already use all of it. Every millisecond of GPU and every percent of CPU we take is taken from the game. Measure before and after (the `perf summary` log line, or the `.perf.csv` next to a recording) and put the numbers in the commit. Nothing busy-waits, nothing repaints continuously, the tab sleeps when nobody's looking at it, and big buffers go to disk, not RAM.

### 2. It records what you saw

The point is footage that looks like the headset felt: the real panels, level, centered where you look, at a steady frame rate, untorn, with sound. A recording that's tilted, choppy, torn or silent is a broken product even when nothing errored. Judge capture changes by looking at what comes out.

### 3. Seamless for normal people

Most users aren't developers and won't open a terminal again after installing. One command installs everything and asks for the password once, and updates arrive on their own. If a user has to do a step, ask whether the software could do it instead. Messages say what happened and what to do next, in plain words.

### 4. One product, many surfaces

The headset tab, the installer, the sync service, the desktop app, the Android app and the site are one product. They should look like it and behave like it, and a feature on one usually belongs on the others.

### 5. Private and safe by default

Clips never leave the user's network: sync is local Wi-Fi only, pinned at pairing, paired devices only. One tiny helper holds the one permission framecorder needs. We never touch a user's videos unless they ask.

## How we like to work

Simple systems that feel obvious. Find the real constraint, then the smallest change that makes the right behavior unsurprising. Don't keep complexity because it's already there (Frame Drop and the flatpak went the day the one-line installer replaced them), and don't add machinery because it looks impressive. Measure twice, cut once, and YAGNI.

Do what the maintainer asked, in the smallest realistic way. Don't quietly widen the task or quietly shrink it. If something outside the task matters, finish the task, then mention it once.

When you talk to us, lead with the next action, number the steps, give real time estimates ("about 10 minutes"), and say plainly what works now. When you need someone in the headset, say exactly what to do and for how long, then do the analysis yourself.

Everything here is a good default, not a law. A maintainer's call in the moment wins.

## A small glossary

- **you**: the agent reading this and changing framecorder.
- **we, maintainers**: coah and the people building framecorder. Who you're talking to.
- **user**: the person wearing the Frame and using framecorder.
- **the Frame, the headset**: Valve's Steam Frame, running SteamOS and SteamVR.
- **recorder**: the `framecorder` binary. **tab**: the dashboard tab, `framecorder-ui`. **helper**: `framecorder-grab`, the panel helper.
- **panels**: capture from the display's scanout, the real thing. **SteamVR's view**: the fallback (`--source headset`), SteamVR's level mirror, for when the panels aren't unlocked.
- **unlocked**: the helper is installed, root owned, with its permission.
- **clip**: the last N seconds, from the replay buffer. **recording**: started and stopped by hand.
- **sync service**: `framecorder-sync` on the headset. **app**: the desktop or Android client that pairs with it. **paired device**: a client holding a token.
- **release**: a `v*` GitHub release from `main`, what the site serves. **dev build**: the `dev-build` prerelease, from `dev`.
- **cant**: the Frame's displays are rotated (~10.7° roll each way, ~5° yaw and pitch). The recorder undoes it.

## The five ways to hurt yourself

1. **Breaking the live headset.** The Frame you reach over ssh is a real person's, often with someone wearing it. Reading anything is fine. Don't uninstall framecorder, remove the helper, wipe `~/.config/framecorder` (their pairings) or delete anything in `~/Videos/framecorder` unless asked. Copy config to `/tmp` before a test that removes things, and put it back after. Try new builds from `target/release/` or the dev build instead of overwriting the install, and ask before anything that interrupts someone playing: restarting SteamVR, restarting the tab mid-recording, changing the refresh rate.
2. **Killing by pattern.** Never `pkill -f` or `pgrep | kill` by name over ssh. The pattern is in your own ssh command's arguments, so you kill your own shell, and you can hit the user's installer or recorder too. Kill only a PID you captured at spawn.
3. **Starting SteamVR yourself.** Never start SteamVR, and never start `framecorder-ui.service` while SteamVR is off (it's bound to `steamvr.service` and pulls it up). A SteamVR started outside its own launcher leaves the headset stuck in passthrough. Never connect a second `framecorder-ui` to SteamVR either: SteamVR counts every copy as the same app, and one leaving takes the other's tab down.
4. **Spending the user's password.** `sudo` on the headset needs the user's password, and it's theirs. Use it only when a maintainer said so for this task. Never write it anywhere: the repo, scripts, logs, commit messages, notes or memory.
5. **Shipping by accident.** Anything on `main`, any asset on a `v*` release, and any run of `site.yml` reaches every headset within 6 hours through the updater. Only do those when a maintainer says "release". Test builds go to the dev build.

## Hit every surface

The most common defect here is a change that works on the path you tested and is missing everywhere else. Before calling work done, walk this list and say which entries applied:

- **Surfaces.** Recorder flags (`framecorder --help`), the tab (home tiles, settings, setup flow, footer notes), the sync API, the desktop app (`app/`, and the native `desktop/` that's replacing it), the Android app, the installer, the site, `README.md` and `docs/how-it-works.md`.
- **Capture paths.** Panels and SteamVR's view. Eye view and both-eyes raw view. 16:9, 1:1 and 9:16. Left and right eye. 72, 90, 120 and 144 Hz.
- **Recorder states.** Idle with clips on, recording, paused (the tab's on screen), recording while clips are on, clips off, display off (headset off a head).
- **Lifecycle.** Fresh install, update by the timer (no password, the tab restarts itself when idle), update from the installer or the app, dev build and back, close and reopen, uninstall, SteamVR restarting, PipeWire restarting, the headset sleeping.
- **Old versions.** An update is installed by the previous release's updater and setup, and old apps talk to new headsets and the other way round. Change the install, update or sync protocol only in ways the old side copes with.
- **Reverse states.** A way in needs a way out and a way to see it: pair and unpair, close and reopen, install and remove, unlock and the note when it's lost.
- **Docs.** Does the change make the README, `docs/how-it-works.md` or this file wrong? Fix it (see [Documentation](#documentation)).

## Branches

- `dev` is where work happens. Commit and push there as you go.
- `main` is what's released. The site deploys from it, and installs, updates and app downloads all come from its releases. It takes nothing but a release merged in from `dev`.
- Pushing to `dev` changes nothing for anyone: no site deploy, no app build, no update.
- Merge PRs into `dev` with a merge commit or a rebase. Merge `dev` into `main` with a plain merge commit, never a squash, so every contributor's commits land on `main` under their name.

## Developing

**On the headset** (developer mode and ssh on). SteamOS has the compilers and headers already:

```sh
curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
git clone https://github.com/coah80/framecorder && cd framecorder && git switch dev
cargo build --release
```

**Off the headset.** The headset crate needs FFmpeg 7.1, PipeWire, libdrm and glslc. The dev container has them:

```sh
docker build -t framecorder-dev tools/dev
docker run --rm -v "$PWD":/src -v framecorder-target:/src/target -v framecorder-cargo:/root/.cargo/registry framecorder-dev cargo test
```

The sync service (`sync/`) builds anywhere. The desktop app (`app/`) needs WebKitGTK on Linux (`app/tools/Dockerfile`), or let CI build it. The native desktop app (`desktop/`) needs `libxkbcommon-dev libxkbcommon-x11-dev libfontconfig-dev libfreetype-dev` and a Vulkan driver on Linux, nothing extra on macOS or Windows. Its thumbnails need `ffmpeg` on the PATH; without it the grid shows plain tiles.

**The installer** (`installer/`, Bun and OpenTUI). `--cpu='*' --os='*'` pulls every platform's native package, so it cross-compiles for the headset from x86:

```sh
cd installer && bun install --frozen-lockfile --cpu='*' --os='*' && bunx tsc -p .
installer/build.sh dist     # arm64 binary, zstd'd, with its sha256
```

## Verifying

- **Smallest proof that the change works.** Run the tests for what you touched, not everything:

  | touched | run |
  |---|---|
  | recorder, tab, setup, helper | `cargo test --release --lib` (headset or dev container) |
  | sync service | `cd sync && cargo test` |
  | app core | `cd app && cargo test`, and `app/tools/e2e.sh` for real daemon-to-client sync |
  | native desktop app | `cd desktop && cargo test` |
  | installer | `cd installer && bunx tsc -p .` |

- **Capture changes need a recording.** On the headset: `framecorder --duration 4 --no-audio /tmp/t.mp4`, then look at it (`ffprobe`, or a frame with `ffmpeg -ss 1 -i /tmp/t.mp4 -frames:v 1 f.png`). A test recorder can run next to the tab's. Keep them short, in `/tmp`, and delete them after.
- **Measure, don't guess.** The recorder logs fps, dropped frames and GPU time every 5 s, the tab's recorder into `~/.local/state/framecorder/recorder.log`. For frame pacing in a file, look at the gaps between packet timestamps (`ffprobe -show_entries packet=pts_time`).
- **Some bugs only show up under a real game** (GPU contention, the encoder falling behind, PipeWire restarts). Ask a maintainer to play something heavy, with exact steps, then pull the logs and files and do the analysis yourself.
- **Screens.** The tab: `framecorder-ui --preview out.rgba <state>`, a raw 1280x760 RGBA frame (`ffmpeg -f rawvideo -pix_fmt rgba -s 1280x760 -i out.rgba out.png`). The app: `app/tools/preview/shots.sh`. The native desktop app: `cd desktop && cargo run -- --demo <screen>` (`clips`, `list`, `syncing`, `unreachable`, `pair`, `settings`), which runs on its own temporary state. The site: headless Chrome against `python3 -m http.server -d site`. Look at what you made before you call it done.
- **Installer and update flows** get tested with the dev build, never the public release. The installer runs in a terminal and takes clicks (SGR mouse) as well as keys, so it can be driven through a pty.
- **Tests stay offline, fast and deterministic.** No headset for `--lib`, no sleeps standing in for synchronization, and every new test passes [the four questions](#clean-up-after-yourself).

## Clean up after yourself

Leave the code, the headset and your machine the way you'd want to find them. Before you call work done:

**The code**
- When your change leaves something unused, delete it in the same change: code, flags, settings, files, docs. No aliases kept for a compatibility nobody needs, no commented-out code, no paths kept for later. Frame Drop and the flatpak went the day the installer replaced them.
- Take out what you added only to debug or test: extra logging, environment switches, and exports or hooks that no real caller uses.
- Prefer the change that leaves less behind. A fix that removes more than it adds is a good fix.

**The tests you add.** Answer all four first. A missing answer means don't add it yet:

1. What behavior does it protect, that a user or another part of framecorder relies on?
2. What realistic regression makes it fail?
3. Why doesn't an existing test catch that already? Extend a table or an existing case before adding a near-copy.
4. Does it need an export, flag or hook that only the test uses? Then test at the real boundary instead.

A regression test has to fail on the code before the fix, for the right reason. Skip tests without a real assertion, tests whose expected value comes from the code under test, tests that grep the source, and mocks that do the thing being asserted.

**What you leave running or lying around**
- On the headset: stop the processes you started (by the PID you kept, see [rule 2](#the-five-ways-to-hurt-yourself)), delete your test recordings, binaries and logs from `/tmp`, put back any config you moved, and leave the install as you found it. If you leave a dev build installed, say so.
- On your machine: stop the servers you started, remove the worktrees and branches you made (`git worktree list`, `git branch`), and delete scratch files.
- Don't commit plans, research notes, test recordings, screenshots or scratch files. Keep them in `/tmp`. `dist/` is gitignored. A merged commit is the record of the work.

**The report.** End with what changed for the user, what you ran to prove it, which surfaces applied, what you removed, and anything you left on purpose, with why.

## Shipping

**Dev build.** Puts `dev` up as the `dev-build` prerelease (needs `gh`, `bun`, and the headset over ssh as `FRAME`, `frame` by default):

```sh
packaging/dev.sh
```

Install it on a headset, in Konsole (desktop mode) or over ssh. That headset then takes its updates from the dev build too, until the normal install command is run again:

```sh
curl -fsSL https://raw.githubusercontent.com/coah80/framecorder/dev/site/install | FRAMECORDER_DL=https://github.com/coah80/framecorder/releases/download/dev-build sh
curl -fsSL https://framecorder.coah80.com/install | sh     # back to releases
```

**Release** (only when a maintainer says so):

1. Merge `dev` into `main` with a merge commit. `main` has its own short `AGENTS.md`; keep `main`'s on a conflict.
2. Bump the version in `Cargo.toml`, `sync/Cargo.toml`, `app/Cargo.toml` and `app/tauri.conf.json`, and the `Cargo.lock` next to each.
3. `gh workflow run app.yml --ref main` builds the desktop apps. Download the artifacts.
4. On the headset, `packaging/release.sh` makes `dist/framecorder-arm64.tar.gz` and its `.sha256`.
5. `gh release create vX.Y.Z --target main` with the headset tarball and checksum, the three desktop apps and the Android APK.

Publishing redeploys the site, which serves the release to the installer and every headset's updater. If the site job didn't run: `gh workflow run site.yml --ref main`. Check it's live with `curl -fsSL https://framecorder.coah80.com/dl/framecorder-arm64.tar.gz.sha256`.

**CI.** `site.yml` builds the installer, copies the latest release into `site/dl` and deploys Pages, on a push to `main` touching `site/` or `installer/`, a published non-pre release, or by hand (Pages only accepts `main` and `v*` tags). `app.yml` builds the desktop apps on a `v*` tag or by hand.

## Pull requests and commits

- Don't open, merge or close PRs unless a maintainer asks. PRs go to `dev`.
- Conventional commits, lowercase, saying what changed for the user: `fix: recordings come out level on the frame's canted displays`. Types: `feat`, `fix`, `perf`, `docs`, `chore`, `refactor`, `test`, with a scope for the apps (`feat(app):`).
- The body says why, with the numbers: fps before and after, ms of GPU, MB of RAM.
- One concern per PR. If the description says "also", split it.
- Visual changes need before and after screenshots. Capture changes need before and after numbers, or frames. Say what you tested on (headset, SteamOS version, phone).

## Documentation

- `README.md` is for users: what framecorder does, how to install, how to use it. Same voice as the product, no contributor tooling.
- `docs/how-it-works.md` explains the capture pipeline and the reasons behind it.
- This file holds what a contributor would get wrong without it. If reading the code answers the question, leave it out. No file catalogs, no feature lists, no PR summaries.
- When something documented changes, rewrite or remove the old text. Don't append a second account.

## How it works

Every vblank, the recorder finds the plane the VR compositor scans out and exports that framebuffer as a dmabuf, directly if it holds the permission, otherwise through the helper. Vulkan imports it as-is (UBWC compressed), and one compute shader (`shaders/convert.comp`) crops it, undoes the lens per color channel with SteamVR's own distortion data, undoes the cant, scales it and converts it to NV12 straight into the hardware encoder's input. HEVC or H.264 comes out of the encoder, AAC from PipeWire, and a writer thread muxes them. Clips are cut from a replay buffer of already encoded packets, kept in segment files on disk. The tab owns the recorder process and talks to it over stdin and stdout. The sync service watches `~/Videos/framecorder` and serves new files over HTTPS to paired devices, announcing itself over mDNS (`_framecorder._tcp`).

## Where code lives

- `src/main.rs` and its modules: the recorder (`kms.rs` scanout, `gpu.rs` Vulkan, `lut.rs` lens and cant, `encoder.rs` V4L2, `audio.rs` PipeWire, `mux.rs` and `writer.rs` files, `replay.rs` clips).
- `src/ui/`: the tab. Drawn by hand into a pixel canvas (`paint.rs`, `text.rs`), no UI toolkit, so every effect costs CPU.
- `src/grab.rs`, `src/setup.rs`, `src/apps.rs`: the helper and its protocol, install, unlock, update and uninstall, SteamVR app registration. Their binaries are in `src/bin/`.
- `src/openvr.rs`, `src/overlay.rs`, `src/input.rs`: OpenVR through `FnTable:` interface tables, no bindings crate. Slot indices come from `openvr_capi.h`, as named consts next to the interface version.
- `sync/`: the sync service, its own crate. `app/`: the desktop app (Tauri 2), `app/src/core/` is the sync client with no UI in it. `desktop/`: the desktop app rebuilt natively on gpui-ce, not shipped yet. It runs the same sync core (`app/src/core`, `default-features = false`) and the same state folder as `app/`, so pairings carry over, and you must never run both at once. `installer/`: the terminal installer. `site/`: the website. `packaging/`: services and the build scripts.

## What the code can't tell you

The traps below come from the Frame itself, and each one cost real time to find.

**The display**
- The compositor draws every frame into the one buffer the panels are showing. Read it late and you get torn frames or green UBWC garbage. That's why the recorder's GPU queue is high priority. Don't lower the default.
- SteamVR tells apps the eyes are level (`GetEyeToHeadTransform`). The compositor applies the cant itself, from the headset's factory calibration: `~/.config/openvr/config/cv/*/config.json`, `tracking_to_eye_transform[eye].eye_to_head` (what `vrcmd --info` shows as "compositor residual"). The calibration's x and y point the other way from ours.
- The game's render target and hidden area mesh are level. The distortion data is canted. Check visibility in the right one, or rotated corners come out black.
- The lens shows more above than below, so the video centers ~16° above straight ahead, like SteamVR's own view.

**SteamVR**
- framecorder is registered as `coah80.framecorder`. In a `.vrmanifest` on this headset the binary key is `binary_path_linux_arm`, and SteamVR skips the app without it.
- Overlay error 17 is `KeyInUse`.
- The app bar's "launch program" lists desktop entries, not SteamVR apps. That's what `framecorder.desktop` is for.

**SteamOS**
- Desktop mode is a nested Plasma session with its own `XDG_RUNTIME_DIR` and session bus. `systemctl --user` from Konsole there misses the real user session unless pointed at `/run/user/<uid>` (`use_user_manager` in `src/setup.rs`).
- A file capability lives on the file, and replacing the file drops it. That's why the permission is on the helper, which updates never touch, and not on the recorder.
- No polkit rule allows `setcap` without a password, and SteamOS has no password until the user sets one. The installer asks for it, and helps set one.
- `/home` survives SteamOS updates. `/usr` is read-only, and `/var` belongs to one OS slot.
- Starting a game can restart PipeWire. Anything holding a stream has to notice and reconnect (`src/audio.rs`).
- Memory is tight and swap is compressed. Holding hundreds of MB lets the kernel swap out PipeWire's realtime thread, which gets it killed and silences the whole headset. Big buffers go to disk at the disk's pace.
- The headset doesn't answer ping. Check port 22, and if it's gone, it's asleep: ask someone to wake it.

**Tooling**
- `gh release` in a script waits for input unless given `</dev/null`.
- `bun build --compile` straight into `/tmp` once produced a broken binary. Build into the project's `dist/`.
- Frame Drop and the flatpak are gone on purpose. Neither could ask for the password the panels need, so don't bring them back.

## Taste

- Complexity lives at the edges: OpenVR and DRM wrappers, the helper protocol, the sync API. The capture loop stays straight-line and the tab stays dumb.
- Errors use `anyhow` with context that reads well in a log (`.context("reading the scanout plane")`). No `unwrap()` on anything that can fail at runtime.
- Comments say why, in plain words, and move with the code. Doc comments are full sentences. Match the file you're in.
- User-facing words are plain and friendly: mostly lowercase on the site and in the installer, sentence case in the tab, no jargon, no emoji. Say what happened and what to do.
- Few dependencies. Reach for a crate only when it's clearly better than the small thing you'd write.
- The brand is one look everywhere: dark, mauve `#cba6f7`, Montserrat for headings, Poppins for text, Space Grotesk for numbers.
- If a rule here fights the task, say so plainly and get a maintainer's sign-off before breaking it.
