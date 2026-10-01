# working on framecorder

notes for anyone (person or agent) changing framecorder.

## branches

- `dev` is where work happens. commit and push there
- `main` is what's released. the site deploys from it, and installs, updates and app downloads all come from its releases. nothing goes on `main` except a release

pushing to `dev` changes nothing for anyone: no site deploy, no app build, no update.

## trying a dev build on a frame

put what's on `dev` up as the dev build (a prerelease called `dev-build`, it never counts as the latest release):

```sh
packaging/dev.sh
```

it needs `gh`, `bun`, and the headset over ssh (`FRAME`, `frame` by default). it builds the headset release on the headset and the installer here, then uploads both.

then on the frame, in konsole (desktop mode) or over ssh:

```sh
curl -fsSL https://raw.githubusercontent.com/coah80/framecorder/dev/site/install | FRAMECORDER_DL=https://github.com/coah80/framecorder/releases/download/dev-build sh
```

same installer, pointed at the dev build. a frame installed that way stays on dev: its updates come from the dev build too (`~/.local/share/framecorder/source` says where from). to go back to releases, run the normal command and pick update:

```sh
curl -fsSL https://framecorder.coah80.com/install | sh
```

(releases up to 0.1.1 don't know about dev builds yet, so until 0.1.2 is out, also `rm ~/.local/share/framecorder/source` after that.)

## releasing

1. merge `dev` into `main`
2. bump the version in `Cargo.toml`, `sync/Cargo.toml`, `app/Cargo.toml` and `app/tauri.conf.json` (and the `Cargo.lock` next to each)
3. build the desktop apps: `gh workflow run app.yml --ref main`
4. on the headset, `packaging/release.sh`
5. `gh release create vX.Y.Z` on `main` with the headset release (`framecorder-arm64.tar.gz` and its `.sha256`), the desktop apps from step 3, and `framecorder-android.apk` (built locally, see `app/README.md`)

publishing the release redeploys the site, which is what the installer and every frame's updater download from.

## good to know

- the recorder never holds a permission. `framecorder-grab` does, installed once by the installer (root owned, `~/.local/lib/framecorder`), so updates don't touch it. see `src/grab.rs`
- the frame's desktop mode is a nested plasma session with its own session bus. anything calling `systemctl --user` has to point at `/run/user/<uid>`, see `use_user_manager` in `src/setup.rs`
- steamvr counts every `framecorder-ui` process as the same app, `coah80.framecorder`. a second one must never connect to steamvr (it leaving takes the first one's tab down), which is what the lock in `src/ui/mod.rs` is for
