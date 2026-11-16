# framecorder sync client

Gets clips and recordings off your Steam Frame over Wi-Fi. It talks to
`framecorder-sync` on the headset (see `../sync`). There's no UI in here:
the desktop app (`../desktop`) and the Android app (`../android`, through
UniFFI) both run this same sync core from `src/core`, and `framecorder-app`
is the same engine headless.

**It only syncs while the Frame is on (awake), on the same Wi-Fi, and
framecorder is running on it.** If nothing's running on the computer or
phone, clips sync the next time it is and the Frame is reachable.

## What it does

- Finds Frames with mDNS (`_framecorder._tcp`) and, for networks that drop
  multicast, by asking every address in the local /24 on port 38619
  (`discover::look`); or takes an address.
- Pairs with the 6-digit code from "pair a device" on the Frame.
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

## Layout

- `src/core/`: `api.rs` client + SSE parser + resumable download, `tls.rs`
  the pinned verifier, `discover.rs` mDNS, `store.rs` paired hosts + index,
  `engine.rs` the sync loop, `pairlink.rs` the QR link.
- `src/headless.rs`: `--headless-sync`, the engine with no window.

State (paired Frames, tokens, index) lives in the config dir
`com.framecorder.app` (e.g. `~/.config/com.framecorder.app/`), files 0600.
The name is the old Tauri app's, kept so pairings carry over to the desktop
app.

## Build

Plain Rust, runs anywhere Rust does:

```sh
cargo test
cargo run -- --headless-sync ~/Videos/framecorder --pair 'framecorder://pair?...'
```

## Headless mode

```
framecorder-app --headless-sync <dir> [--state <dir>] [--pair <link> | --host <ip:port> --code <123456> [--fp <sha256>]] [--exit-after <secs>]
framecorder-app --discover [--seconds <n>]
```

Pairs if asked to, then syncs into `<dir>` until killed. Without `--fp`,
`--host` learns the fingerprint on first contact and pins it from then on.

## Tests

- `cargo test`: pairing link parsing, fingerprint normalisation, the pinned
  verifier, index diff/ordering and persistence, SSE parsing, Content-Range,
  safe file names, no-overwrite finishing, plus `tests/pinned_tls.rs`
  against a real rustls server (right fingerprint connects, wrong one fails
  before any HTTP is sent, capture mode).
- `tools/e2e.sh` (from the repo root): real daemon on a temp HOME + headless
  mode. Wrong fingerprint refused (code not used up), pairing, catch-up of an
  existing file, `.part` -> `.mp4` rename synced in ~25 ms, a download killed
  mid-way while the fake SteamVR says a game is running resumes via Range.

## Known gaps

- Pairing a Frame picked from mDNS trusts the network during pairing (the QR
  code doesn't have that problem, it carries the fingerprint).
- Tokens are stored in the config dir, not the OS keychain.
