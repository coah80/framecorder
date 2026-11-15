#!/usr/bin/env python3

import argparse
import pathlib
import subprocess
import sys

import cv2
import numpy as np

CACHE = pathlib.Path.home() / ".cache/framecorder-promo"
PANELS = 2.0
SIZES = {"16:9": (1920, 1080), "1:1": (1440, 1440), "9:16": (1080, 1920)}


def fetch_lut(frame, recorder, view, shape, dominant):
    name = f"{view}-{shape.replace(':', 'x')}" + ("-left" if view == "both" and dominant == "left" else "")
    path = CACHE / f"{name}.lut"
    if path.exists():
        return path
    CACHE.mkdir(parents=True, exist_ok=True)
    remote = f"/tmp/framecorder-promo-{name}.lut"
    args = f"--view eye --eye {view} --aspect {shape}" + (" --dominant left" if "-left" in name else "")
    print(f"asking {frame} for the {view} view's grid", file=sys.stderr)
    subprocess.run(["ssh", frame, f"{recorder} {args} --dump-lut {remote}"], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run(["scp", "-q", f"{frame}:{remote}", str(path)], check=True)
    subprocess.run(["ssh", frame, f"rm -f {remote}"], check=True)
    return path


def read_lut(path):
    data = path.read_bytes()
    head, _, body = data.partition(b"\n")
    words = head.decode().split()
    if words[:2] != ["framecorder-lut", "1"]:
        sys.exit(f"{path} isn't a lookup grid this can read")
    out_w, out_h, grid_w, grid_h = map(int, words[2:6])
    step = float(words[6])
    per_channel, blended = words[7] == "1", words[8] == "1"
    points = np.frombuffer(body, dtype="<f4").reshape(-1, 2)
    count = grid_w * grid_h
    eyes = [points[e * count * 3 : (e + 1) * count * 3].reshape(grid_h, grid_w, 3, 2) for e in range(2 if blended else 1)]
    weight = points[count * 6 : count * 7, 0].reshape(grid_h, grid_w) if blended else None
    return out_w, out_h, step, per_channel, eyes, weight


def spread(grid, out_w, out_h, step):
    gx = (np.arange(out_w, dtype=np.float32) + 0.5) / step
    gy = (np.arange(out_h, dtype=np.float32) + 0.5) / step
    i, j = np.floor(gx).astype(int), np.floor(gy).astype(int)
    fx, fy = (gx - i)[None, :, None], (gy - j)[:, None, None]
    grid = grid.reshape(grid.shape[0], grid.shape[1], -1)
    a, b = grid[j][:, i], grid[j][:, i + 1]
    d, e = grid[j + 1][:, i], grid[j + 1][:, i + 1]
    value = a + (b - a) * fx + (d - a) * fy + (e - d - b + a) * fx * fy
    inside = np.minimum(np.minimum(a[..., 0], b[..., 0]), np.minimum(d[..., 0], e[..., 0])) >= 0
    return value, inside


def eye_maps(eye, per_channel, out_w, out_h, step, width, height):
    fit_w = min(width, height * PANELS)
    fit_h = fit_w / PANELS
    ox, oy = (width - fit_w) / 2, (height - fit_h) / 2
    maps, shown = [], None
    for c in range(3):
        uv, inside = spread(eye[:, :, c if per_channel else 1], out_w, out_h, step)
        x = (ox + uv[..., 0] * fit_w - 0.5).astype(np.float32)
        y = (oy + uv[..., 1] * fit_h - 0.5).astype(np.float32)
        maps.append(cv2.convertMaps(x, y, cv2.CV_16SC2))
        if c == 1:
            shown = inside
    return maps, shown


class View:
    def __init__(self, lut_path, width, height):
        self.out_w, self.out_h, step, per_channel, eyes, weight = read_lut(lut_path)
        self.eyes = [eye_maps(eye, per_channel, self.out_w, self.out_h, step, width, height) for eye in eyes]
        self.weight = None
        if weight is not None:
            w, _ = spread(weight, self.out_w, self.out_h, step)
            w = np.clip(w[..., 0], 0, 1)
            w[~self.eyes[0][1]] = 1
            w[~self.eyes[1][1]] = 0
            self.weight = w[..., None].astype(np.float32)

    def eye(self, frame, maps, shown):
        out = np.dstack([cv2.remap(frame[:, :, 2 - c], *maps[c], cv2.INTER_CUBIC) for c in (2, 1, 0)])
        out[~shown] = 0
        return out

    def develop(self, frame):
        left = self.eye(frame, *self.eyes[0])
        if self.weight is None:
            return left
        right = self.eye(frame, *self.eyes[1])
        return (left * (1 - self.weight) + right * self.weight + 0.5).astype(np.uint8)


def probe(path):
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,r_frame_rate", "-of", "csv=p=0", str(path)],
        check=True, capture_output=True, text=True,
    ).stdout.strip().split(",")
    return int(out[0]), int(out[1]), out[2]


def main():
    ap = argparse.ArgumentParser(description="Makes eye views out of promo footage's recording of both panels.")
    ap.add_argument("panels", type=pathlib.Path, help="a <name>-panels.mp4 or .mkv")
    ap.add_argument("--views", nargs="+", default=["left", "right", "both"], choices=["left", "right", "both"])
    ap.add_argument("--shape", default="16:9", choices=list(SIZES))
    ap.add_argument("--dominant", default="right", choices=["left", "right"], help="the eye in the middle of a both-eyes view")
    ap.add_argument("--frame", default="frame", help="the headset over ssh, for grids not fetched yet")
    ap.add_argument("--recorder", default="~/.local/bin/framecorder", help="the recorder on the headset, to try one that isn't installed")
    ap.add_argument("--crf", type=int, default=14)
    ap.add_argument("--seconds", type=float, help="only this much from the start")
    ap.add_argument("-o", "--out", type=pathlib.Path, help="folder to write to, the recording's by default")
    args = ap.parse_args()

    width, height, rate = probe(args.panels)
    stem = args.panels.stem.removesuffix("-panels")
    folder = args.out or args.panels.parent
    views = [(v, View(fetch_lut(args.frame, args.recorder, v, args.shape, args.dominant), width, height)) for v in args.views]

    limit = ["-t", str(args.seconds)] if args.seconds else []
    reader = subprocess.Popen(
        ["ffmpeg", "-v", "error", *limit, "-i", str(args.panels), "-vf", f"fps={rate},scale=in_color_matrix=bt709:in_range=limited:out_range=full,format=bgr24", "-f", "rawvideo", "-"],
        stdout=subprocess.PIPE,
    )
    writers = []
    for name, view in views:
        out = folder / f"{stem}-{name}{args.panels.suffix}"
        writers.append(subprocess.Popen(
            ["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "bgr24", "-s", f"{view.out_w}x{view.out_h}", "-r", rate, "-i", "-",
             *limit, "-i", str(args.panels), "-map", "0:v", "-map", "1:a?", "-c:a", "copy",
             "-vf", "scale=out_color_matrix=bt709:in_range=full:out_range=limited,format=yuv420p",
             "-c:v", "libx264", "-preset", "fast", "-crf", str(args.crf),
             "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv", str(out)],
            stdin=subprocess.PIPE,
        ))
        print(f"writing {out}", file=sys.stderr)

    size = width * height * 3
    done = 0
    while True:
        raw = reader.stdout.read(size)
        if len(raw) < size:
            break
        frame = np.frombuffer(raw, dtype=np.uint8).reshape(height, width, 3)
        for (_, view), writer in zip(views, writers):
            writer.stdin.write(view.develop(frame).tobytes())
        done += 1
        if done % 720 == 0:
            print(f"{done} frames", file=sys.stderr)
    failed = reader.wait() != 0
    for writer in writers:
        writer.stdin.close()
        failed |= writer.wait() != 0
    print(f"{done} frames", file=sys.stderr)
    sys.exit(1 if failed or done == 0 else 0)


if __name__ == "__main__":
    main()
