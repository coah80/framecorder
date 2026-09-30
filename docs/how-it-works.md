# how framecorder records the frame

this is the long version of "how it works" from the readme: how framecorder gets the picture off the frame's panels, turns it into a normal flat video, and writes it out with audio, without costing the game anything you'd notice.

the short version: it reads the exact buffer the vr compositor is sending to the panels, un-warps one eye with a single compute shader, and hands the result straight to the hardware encoder. pixels never touch the cpu, and nothing gets copied.

```
 vr compositor ──► scanout buffer (what the panels show, both eyes, lens-warped)
                          │  borrowed, zero copy (kms + dmabuf)
                          ▼
                 vulkan compute shader  ◄── lookup grid (steamvr's lens data, built once)
                          │  un-warp, fix color fringing, scale, rgb → nv12
                          ▼
                 hardware encoder (qcom iris, v4l2) ──► hevc / h264 packets
                                                           │
 pipewire: game audio + mic ──► mix ──► aac ───────────────┤
                                                           ▼
                                       session thread ──► recording (.mp4)
                                                      └─► replay buffer ──► clips
```

## 1. finding what the panels show

most vr recorders record steamvr's mirror window, or ask the game for its eye textures. both give you something that isn't quite what you saw: no passthrough, no overlays, different timing, sometimes a different frame entirely.

framecorder goes one level lower, to the display itself (`src/kms.rs`). on linux the panels are driven through kms, and whatever's on them is a framebuffer attached to a plane on the display's crtc. the vr compositor owns the display (it's drm master), but anyone can *look* at it:

1. open `/dev/dri/card0`, turn on universal planes, and find the crtc with an active mode. that gives the panel's resolution and exact refresh rate (worked out from the mode's pixel clock, not the rounded number)
2. find the plane on that crtc with the biggest framebuffer. that's the compositor's output, not a cursor or anything small
3. every frame, wait for vblank, then ask the plane which framebuffer it's scanning out right now

getting from a framebuffer id to the actual memory behind it is the one privileged thing framecorder does. the kernel only hands out buffer handles for someone else's framebuffer to processes with `CAP_SYS_ADMIN`, which is why the installer asks for your password once and puts that capability on the recorder binary (and nothing else). with it, the buffer gets exported as a dmabuf, a file descriptor for a chunk of gpu memory.

a couple of details that matter:

- **framebuffer ids get recycled.** when the compositor rebuilds its swapchain, the same id can point at a different buffer. so framecorder identifies buffers by the dmabuf's inode instead, since every export of one buffer shares the same dma_buf file
- **the compositor might still be drawing.** polling the dmabuf waits on its fence, so we never read a half-drawn frame
- **the compositor only flips between a few buffers.** each one gets imported once, near the start, and after that every frame is just "which of the ones we already know is on screen"

the frame's scanout is both eyes side by side, 2160 px per eye, with the lens distortion and chromatic aberration correction already baked in. that's about 37 MB per frame, which is why nothing about it should ever go near the cpu.

## 2. borrowing it in vulkan

the dmabuf gets imported into vulkan as an image (`src/gpu.rs`), exactly as it is:

- **same memory, no copy.** `VK_EXT_external_memory_dma_buf` imports the fd as device memory, and the image is bound straight onto it
- **same tiling.** the adreno keeps scanout buffers ubwc-compressed. `VK_EXT_image_drm_format_modifier` with the buffer's own modifier means vulkan reads the compressed layout directly, with no decompress pass
- **borrowed, then handed back.** every dispatch does a queue family ownership transfer from `VK_QUEUE_FAMILY_FOREIGN_EXT` to us and back again, in `GENERAL` layout both ways. so the driver never decompresses, discards or rearranges the image behind the compositor's back

the command buffers for every (scanout buffer, encoder buffer) pair are recorded once at import. per frame, framecorder just submits one of them and waits on a fence.

## 3. un-warping one eye

the panels show the picture *after* the lens pre-distortion, so a straight crop looks like a fisheye with colored fringes. to get a normal flat video, framecorder undoes exactly what steamvr did, using steamvr's own lens data (`src/lut.rs`).

this all happens once, at startup:

1. **read the lens model.** ask steamvr's `ComputeDistortion` on a 97×97 grid over each eye's panel. for every panel spot, that says which point of the eye's render target it shows, separately for red, green and blue (the compositor shifts the channels differently to cancel the lens's color fringing)
2. **work out the widest clean view.** steamvr's hidden area mesh says which parts of the render target are never drawn, and the distortion grid says which parts land on the panel at all. a binary search finds the widest view at your aspect ratio (16:9, 1:1 or 9:16) whose edges are all actually drawn, so there are no black corners. that comes out around 77° square or 64°×96° tall, then pulls in 3% from the edge, where the lens is blurriest
3. **invert the lens, per output pixel.** for every point on a grid 8 output pixels apart: turn it into a view direction, undo the eye's roll (so the video stays level with your head), turn that into a render target spot, then solve for the panel position that shows it with newton's method on the distortion grid. that runs three times, once per color channel
4. **decide how hard the shader has to work.** if the color fringes are narrower than a third of a video pixel everywhere, the per-channel correction gets turned off entirely. if the video shrinks the panel by more than 1.5×, supersampling gets turned on so it doesn't alias

one gotcha: the frame's compositor scans each eye out turned 180° inside its own half, while steamvr describes the panels upright. that was only found by recording on a real headset and getting upside-down videos. the shader flips each half back per sample, which keeps the seam between the eyes clean.

the result is a small lookup grid (a few thousand points) uploaded to the gpu once. per frame, the gpu does no lens math at all.

## 4. the one shader

every recorded frame is one compute dispatch of `shaders/convert.comp`, reading the scanout and writing the encoder's input:

- **tiles.** each invocation writes a 4×2 block of pixels, and a workgroup covers 32×16 pixels, exactly 4×2 grid cells. the workgroup loads the grid points it needs into shared memory once, and each pixel's source position is a bilinear blend of its cell's corners
- **sharp reads.** each sample is catmull-rom in 5 bilinear taps instead of plain bilinear. the panel image has already been resampled once by the compositor, and a soft filter on top of that looked noticeably blurrier than steam's own recorder
- **color fringing, only where it matters.** near the middle of the lens the three channels line up, so a pixel gets one read. only cells where red or blue drift more than 0.3 px from green do three separate reads
- **straight to nv12.** colors go through bt.709 limited range. the scanout is already gamma-encoded, which is what video expects, so there's no transfer conversion. luma and interleaved chroma get packed into 32-bit words and written straight into the encoder's input buffer (a dmabuf the encoder owns), with no byte stores and no atomics

that costs about 0.6 ms of gpu per recorded frame at 500 mhz, less at gameplay clocks.

it runs on its own vulkan queue with a global priority you pick (`--gpu-priority`). `low` only runs when the game leaves the gpu idle, which is the nicest to the game. but the compositor reuses the scanout buffer a frame later, so in a really heavy game `low` can miss frames, and `medium` or `high` trade a little of the game's gpu time for a steady capture.

## 5. timing

capture is paced off the display itself:

- the frame rate defaults to the panel's refresh rate (72 or 90), or half of it at 120 or 144, so every recorded frame is the same distance apart
- each loop waits for vblank, and only grabs a frame when the next slot on the frame grid is due (with half a refresh of tolerance). so 60 fps on a 72 hz panel still works, it just judders a bit
- timestamps come from the vblank's own `CLOCK_MONOTONIC` time, the moment the frame actually went to the panels. audio uses the same clock, which is what keeps them in sync
- if the encoder is still busy with all its input buffers, that frame gets dropped instead of blocking. the loop never waits on anything but vblank and its own shader

## 6. encoding

the snapdragon's hardware encoder (qcom iris) is driven directly through the v4l2 stateful encoder interface (`src/encoder.rs`). nothing is in between: no gstreamer, no ffmpeg for the video.

- the encoder's input queue takes the nv12 dmabufs the shader writes into, so the frame goes gpu → encoder without anyone copying it
- hevc (default) or h264, 40 mbit/s by default, no b-frames, a keyframe every 2 seconds, and the profile, level and tier worked out from the resolution, fps and bitrate
- starting a recording mid-session forces a keyframe, so the file doesn't open on a wait for the next one

encoded packets come out of mmapped buffers and go off to the session thread. the capture loop itself never touches a file.

## 7. audio

audio comes from pipewire (`src/audio.rs`):

- **game audio** is a capture stream with `stream.capture.sink`, which listens to the monitor of whatever the default output is. it's marked passive, so listening in doesn't keep the speakers awake when nothing's playing
- **the mic** is a normal capture stream. the frame mutes it whenever the headset's off your head, so desk tests come out silent

every chunk gets stamped with `CLOCK_MONOTONIC`, the same clock the video uses. the mixer puts both sources on the video's timeline and mixes them into one track, so every player plays the mic. `--mic-track` adds a mic-only track too. each track is aac-encoded with ffmpeg (`src/aac.rs`):

- small timing wobbles get bridged
- anything more than 50 ms off gets resynced by padding silence or dropping samples, so it can't slowly drift out of sync
- a source that goes quiet for a second gets padded with silence, so files always have continuous audio

## 8. files, pausing and clips

one session thread (`src/mux.rs`) takes the video and audio packets and sends them wherever they're wanted. one encoder feeds everything, so recording and clipping at once never means two encodes.

- **recordings** go to a writer thread (`src/writer.rs`, ffmpeg's muxer) as `name.mp4.part`, which gets renamed to `name.mp4` once it's finished. a slow disk can't hold up capture, and the sync service never picks up half a file. mp4s get finished on any clean stop: ctrl+c, ssh dropping, the tab quitting. use `.mkv` if you want recordings that survive the headset dying
- **pausing.** the tab pauses the recording whenever the framecorder tab itself is on screen, so the controls never end up in your video. paused time is cut out of the timeline, so the video just continues from where it paused
- **clips** come from a replay buffer (`src/replay.rs`): the last N seconds of already-encoded packets, kept in ram. nothing gets re-encoded. saving a clip finds the last keyframe far enough back, rebases the timestamps to zero and writes those packets out, on its own thread. that's about 2% of a core plus bitrate × length of ram (~190 mb for 30 s at high). starting a recording turns the buffer off and frees that ram, since the recording has it all anyway

## 9. the other source: steamvr's headset view

`--source headset` records steamvr's headset view instead, the same picture steam's own recorder saves (`src/headset_view.rs`). it's steamvr's flat render of the game's eye images, before any lens distortion, so it's only resampled once.

framecorder shares its vulkan device with steamvr through `IVROverlayView`, and every frame it acquires the newest view texture. it copies that into an image of its own and runs it through the same shader, with a plain fit-to-frame grid.

the catch is that steamvr only renders that view while something's recording it. that costs the game about 0.45 ms of gpu per frame and the odd reprojected frame. on the frame it's also fixed at 1920×1080 of the left eye, and asking for another size or eye is silently ignored. the panel source costs the game nothing measurable, which is why it's the default.

## what it all costs

measured on a steam frame (snapdragon 8 gen 3, adreno 750), 1920×1080 hevc:

| | |
|---|---|
| gpu | ~0.6 ms per recorded frame at 500 mhz, less at gameplay clocks |
| cpu | ~5-7% of one core (out of 8) with game audio + mic |
| ram | ~10 mb of its own, plus the replay buffer if clips are on |
| encoding | the hardware encoder, no cpu or gpu time |
| the dashboard tab | under 1% of a core, sleeps when the menu's closed |

every recording writes a `<video>.perf.csv` next to it with the cpu, gpu and memory use, so you can check it in your own games (`--no-perf-log` turns that off).
