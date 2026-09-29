# framecorder-sync

Runs on the Steam Frame and hands finished recordings and clips to paired
devices on the same Wi-Fi (the framecorder app, see `../app`). It's meant to
sit there all day next to whatever you're playing, so it's built to do
nothing at all until someone asks for a file.

## Install (on the headset)

```sh
cd ~/framecorder/sync
./install.sh
```

That builds with `cargo build --release`, copies the binary to
`~/.local/bin/framecorder-sync`, installs `../packaging/framecorder-sync.service`
as a user unit and enables it. The unit is `WantedBy=default.target`, so it
runs whenever the headset is on, SteamVR or not, with `Nice=10`,
`IOSchedulingClass=idle`, `Restart=on-failure`.

Logs: `journalctl --user -u framecorder-sync -f`

### Building elsewhere

- x86_64 Linux, for trying it out: `cargo build --release`, then e.g.
  `HOME=/tmp/fake ./target/release/framecorder-sync --port 38619`
- aarch64 from x86_64 (what the Frame runs), in any Debian-ish container:
  ```sh
  apt install gcc-aarch64-linux-gnu libc6-dev-arm64-cross qemu-user
  rustup target add aarch64-unknown-linux-gnu
  export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
  cargo build --release --target aarch64-unknown-linux-gnu
  # tests run under qemu:
  CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER="qemu-aarch64 -L /usr/aarch64-linux-gnu" cargo test --target aarch64-unknown-linux-gnu
  ```

Flags, mostly for testing: `--videos DIR`, `--state DIR`, `--port N`, `--no-mdns`.

## Why it's built like this

Performance on the headset comes first, so the stack is as small as it gets:

- **Plain threads, blocking IO, no async runtime.** There are only ever a
  handful of connections (each device keeps one event stream open and
  downloads one file at a time). A thread parked in `read()` or on a channel
  costs no CPU, and pacing a transfer is just sleeping. tokio/hyper would
  bring a scheduler, a timer wheel and a few hundred KB of code for nothing.
- **rustls** (ring backend) for TLS, **httparse** for request heads, and ~150
  lines of our own HTTP/1.1 around them: keep-alive, `Content-Length` bodies
  up to 16 KB, no chunked uploads (nobody uploads anything).
- **inotify** directly (not `notify`): one thread blocked in `read()`,
  watching the two video folders and the state folder.
- **mdns-sd**, pure Rust, no avahi. IPv4 only, loopback off, checks for new
  network interfaces every 15 s so it notices Wi-Fi coming back after
  standby.
- The clip list lives in memory and is kept current by inotify, so `GET
  /clips` never touches the disk. MP4 durations come from `mvhd` (or `mehd`
  for fragmented files), reading only box headers on the way, so a multi-GB
  recording with `moov` at the end costs a few seeks.
- File transfers read 256 KB at a time with `pread`, and drop what they've
  sent from the page cache (`POSIX_FADV_DONTNEED`) every 8 MB, so streaming
  a 5 GB recording doesn't evict the game's assets.

Measured on the x86_64 dev box (release build, `/proc/<pid>/stat` and
`/status`):

| state | CPU | RSS |
| --- | --- | --- |
| idle, one device's event stream open, 30 s | 0 ticks (0.00%) | 5.0 MB |
| idle, no clients | 0.00% | 5.0 MB |
| transfer throttled to 8 MB/s ("game running") | 1.1% of one core | 5.0 MB |
| transfer at full speed over localhost (~1.5 GB/s) | ~98% of one core | 5.0 MB |

Full speed on loopback is TLS-bound; over Wi-Fi (tens of MB/s) that scales
down to a few percent. Threads: 4 idle (accept, inotify, game check, mDNS)
plus one per open connection. Binary is ~1.9 MB stripped. On-headset numbers
are still to be taken (the Frame was offline while this was written).

## Not getting in the way of games

Transfers go flat out by default: sending a file doesn't touch the GPU or
the encoder, so it doesn't slow a game down. If you stream PC VR to the Frame
over the same Wi-Fi and it stutters during a sync, set `game_rate_mb` in
settings.json (8 is a good start) and transfers get paced to that while a VR
game is running. At 0 the game check below never runs at all.

Whether a game is running is only checked **while a transfer is active**,
every 5 s. Idle, the checker sleeps on a condvar and never wakes up. The
check dlopens `libopenvr_api.so` (`/opt/steamvr/bin/linuxarm64/`, or
`$FRAMECORDER_OPENVR_LIB`), connects as a **background** app (which SteamVR
never starts a server for, so this can't launch SteamVR), asks
`IVRApplications::GetSceneApplicationState()` (slot 25 in `_008` and `_007`),
and disconnects. Any scene app counts, SteamVR Home included: if something is
rendering, you're probably wearing it. No SteamVR, or no library, means no
game. A transfer that starts after a quiet spell assumes a game until the
first check comes back a few ms later.

On top of that the unit runs with `Nice=10` and `IOSchedulingClass=idle`.

## Protocol

HTTPS only, with the self-signed certificate from `cert.pem` (clients pin its
SHA-256). Everything except `/hello` and `/pair` needs
`Authorization: Bearer <token>`. Default port 38619 (`port` in settings.json;
if it's taken another one is picked and written to info.json).

| | |
| --- | --- |
| `GET /hello` | `{"name","version":1,"fingerprint"}` |
| `POST /pair` `{"code","device_name"}` | `{"token","device_id"}`. 403 with a reason for a wrong/expired/missing code, 429 + `Retry-After` after 5 wrong codes in a minute (then even the right code waits) |
| `GET /clips` | `[{"id","kind":"clip"\|"recording","name","size","duration_s"\|null,"created"}]`, newest first. `id` is `c-<stem>` / `r-<stem>` (hex after `~` for odd names), URL-safe and stable |
| `GET\|HEAD /clips/{id}/file` | the MP4. `Range: bytes=` (single range, open-ended and suffix too) gets a 206 with `Content-Range`; unsatisfiable gets 416. `ETag` + `If-Range` supported. `Content-Type: video/mp4` |
| `GET /events` | Server-Sent Events: `event: new` + clip JSON when a file finishes, `event: removed` + `{"id"}`, `: keepalive` every 25 s. One stream per device; a new one replaces the old |
| `DELETE /clips/{id}` | 204 only if `delete_after_sync` is true in settings.json, else 403. The app calls this after a verified download, so the setting means what it says |

A file counts as finished when it's renamed (or written and closed) to
`*.mp4` in `~/Videos/framecorder/` or `.../clips/`. `*.part`, `*.perf.csv`,
`*.log` are ignored. Files already there at startup count as finished.

## Shared state (`~/.config/framecorder/sync/`)

- `info.json`, written at startup: `{"name","port","fingerprint","version":1}`.
  `name` is `name` from settings.json, else the hostname, else "Steam Frame".
- `pairing.json`, written by the tab: `{"code":"123456","expires":<unix s>}`.
  Read on every `/pair`. Deleted after one successful pairing.
- `devices.json`, ours: `[{"id","name","token_sha256","paired_at","last_seen"}]`.
  Only token hashes are stored. Watched with inotify: if the tab removes an
  entry, that device is locked out right away and its event stream closes
  within 25 s. Our own writes re-read the file first, so a revoke can't get
  undone by a `last_seen` update.
- `settings.json`, optional: `delete_after_sync` (bool), `port`, `name`,
  `game_rate_mb`.
- `cert.pem` / `key.pem` (0600): made with rcgen on first run, reused after.

The pairing QR the tab shows:
`framecorder://pair?host=<ip>&port=<port>&fp=<fingerprint>&code=<code>&name=<urlencoded name>`

## Firewall

The daemon listens on TCP 38619 (all interfaces) and uses UDP 5353 for mDNS.
Whether SteamOS on the Frame filters those is still to be checked on the
headset. If it does, both need to be let in on the Wi-Fi interface; this
doesn't change any firewall config itself.

## Security notes

- Tokens are 256-bit random, compared by hash; pairing codes compare in
  constant time and are rate-limited globally (5 wrong per minute).
- Pairing with a Frame picked from mDNS (desktop) trusts the network at that
  moment: someone spoofing mDNS during the pairing window could get the code.
  The QR code (Android) carries the fingerprint, so it doesn't have that gap.
- 32 connections at most, 8 KB request heads, 16 KB bodies, 30 s idle
  timeout, and `TCP_USER_TIMEOUT` of 60 s so dead Wi-Fi clients don't linger.

## Tests

`cargo test`: Range parsing, request parsing limits, pairing (right/wrong/
expired code, one use, rate limit), token checks, revoking via devices.json,
the scanner (ignores `.part` and friends, mvhd/mehd durations), the SSE hub,
an SSE `new` event within a second of a `.part` rename, pacing, and the game
checker only running during transfers. The end to end test with the app is
`../app/tools/e2e.sh`.
