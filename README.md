# framecorder

a recorder for the steam frame that grabs what the headset panels ACTUALLY show, without eating the headset's performance. the point is footage that looks like the frame really feels, not the laggy stuff other recorders make.

it runs right on the headset. there's a tab in the steamvr dashboard with a big record button, or you can drive it over ssh.

## what it costs

measured on a steam frame (snapdragon 8 gen 3, adreno 750), 1920x1080 hevc:

- gpu: about 0.6 ms per recorded frame at 500 MHz, a lot less at gameplay clocks, and it runs on a LOW priority queue so the compositor always goes first
- cpu: about 5-7% of ONE core with game audio + mic, out of 8
- ram: about 10 MB of its own, the rest is shared libraries
- the dashboard tab: under 1% of a core, sleeps when the menu is closed, only redraws when something changes
- video encoding: the snapdragon's hardware encoder, so no cpu or gpu time there

## how it works

pixels never touch the cpu.

1. wait for the display's vblank, look at which buffer the vr compositor is scanning out to the panels
2. import that buffer into vulkan as-is (zero copy, compressed ubwc tiling and all)
3. one small compute shader crops it, undoes the lens distortion and color fringing (using steamvr's own lens data), scales it and converts it to nv12, writing straight into a buffer the encoder reads
4. the hardware encoder (qcom iris, v4l2) makes hevc or h264
5. a separate thread muxes that with aac audio from pipewire into an mp4

so it's what's on the panels, passthrough, overlays, dashboard and all, NOT steamvr's mirror window.

## views and shapes

- 16:9 (default), 1:1 and 9:16: one eye un-warped from what the panels show, level, centered on straight ahead and as wide as it goes without black edges (about 77° square, 64° x 96° tall). read with a sharp catmull-rom filter so it holds up next to steam's recorder. `--fov` overrides the width
- both eyes (`--view raw`): both panels exactly as scanned out, distortion included

`--source headset` records SteamVR's headset view instead, the picture Steam's own recorder saves (16:9 left eye only on the frame). steamvr only renders it while someone's recording it, so it costs the game about 0.45 ms of gpu per frame and a few reprojected frames. the panel source costs the game nothing measurable, so it's the default.

the frame rate defaults to the panel's refresh rate (72 or 90), or half of it at 120/144, so every frame is evenly spaced. `--fps 60` forces 60, tho it'll judder a bit if the panel's at 72.

## audio

game audio is on by default, and the tab records the mic too (toggle it off in the tab, or `--mic` on the command line). the mic gets mixed into the main track so every player plays it; `--mic-track` adds a mic only track for editing. the frame mutes its own mic whenever the headset's off your head, so tests on the desk come out silent.

## clips

clips are on by default (the switch is under the record button, the length is in settings, clips: 15 s, 30 s, 1 min, 2 min). framecorder keeps that much of the last few seconds in memory the whole time. then any of these saves it to `~/Videos/framecorder/clips`:

- hold the left thumbstick down (rebind it in steamvr's controller settings under "framecorder")
- the clip button in the tab

it's always on, no recording needed. a little note floats up in front of you and the controller buzzes when it's saved. the buffer is just the already encoded video: about 2% of a core, plus bitrate × length of ram (~190 MB for 30 s at high). starting a recording turns the buffer off and frees that ram, since the recording has it all anyway, and it comes back on when you stop.

on the command line: `framecorder --replay 30 --control`, then type `clip` (or `record <file>`, `stop-record`, `pause`, `resume`, `quit`).

## sync to your phone or computer

`framecorder-sync` runs on the headset and hands finished clips and recordings to the framecorder app (windows, macos, linux, android) over wi-fi, as soon as they're saved. pair from the tab: settings, sync, "pair a device" shows a qr code and a code to type. it only syncs while the frame is on, on the same wi-fi, with framecorder running. transfers go full speed, sending a file doesn't touch the gpu or the encoder. if you stream pc vr over the same wi-fi and it stutters during a sync, set `game_rate_mb` in `~/.config/framecorder/sync/settings.json` (8 is a good start). the app's in `app/`, the service in `sync/`, both have their own readmes.

## install

the easy way is [frame drop](https://framedropvr.com): pair your frame with it, press install on the framecorder site, then open framecorder from your library on the headset once. that runs `framecorder-setup`, which puts everything in place and starts it. running it again is how updates get installed.

the recorder needs one permission (`cap_sys_admin`, to read what's on the display). the installer takes it if the headset lets it without a password. if it can't, the tab says so, with the one command to run over ssh.

making a release for frame drop, on the headset: `packaging/release.sh <where the files will be downloaded from>`, then upload the two files in `dist/` there.

or build it yourself. on the headset, with developer mode on and ssh working:

```sh
curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
git clone <this repo> framecorder && cd framecorder
./install.sh
```

it builds, installs `framecorder` and `framecorder-ui` to `~/.local/bin`, gives the recorder `cap_sys_admin` (it needs that to read another process's framebuffers, nothing else gets it), and sets up a user service that starts the dashboard tab together with steamvr.

the tab only ever attaches to a steamvr that's already running. connecting as an overlay app is allowed to launch steamvr's server itself, and a server started from outside steamvr's own launcher leaves the headset stuck on passthrough, so it checks first and waits.

needs gcc, clang, glslc, ffmpeg, pipewire and vulkan headers, which the frame's image already has.

## using it

open the steamvr dashboard, there's a framecorder tab. the first screen is the record button, clips, and four tiles saying what's set up (video, audio, clips, sync). tap a tile or "settings" to change things, one section at a time. hit record, then switch to another tab or close the menu and it starts. it pauses by itself whenever the framecorder tab is on screen, so the controls never end up in your video, just come back to it and hit stop. files go to `~/Videos/framecorder`.

(it goes by the tab and not the whole dashboard because on the frame the steam menu counts as open the whole time you're in home)

or from ssh:

```sh
framecorder                          # left eye, 16:9, game audio, until ctrl+c
framecorder --aspect 9:16 --mic      # vertical with the mic on its own track
framecorder --view raw clip.mkv      # both panels, distortion and all
framecorder --duration 30 --qp 20    # 30 seconds at a fixed quality
kill -USR1 <pid> / kill -USR2 <pid>  # pause / resume
```

`framecorder --help` has the rest.

## notes

- mp4s get finished when the recording stops cleanly, which includes ctrl+c, ssh dropping, and the dashboard tab quitting. if the headset dies mid recording the mp4 is toast, use `.mkv` if you want recordings that survive that
- raw view costs about 3x the gpu of eye view since it has to read all 37 MB of both panels every frame
- the fonts in the dashboard tab (montserrat, poppins, space grotesk) are under the sil open font license, see `assets/fonts`
