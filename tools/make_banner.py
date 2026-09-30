"""framecorder README banner: soft mauve background, white pill, app icon + "framecorder".

    python tools/make_banner.py

Writes assets/framecorder-banner.png (1500x500). Uses the app icon from app/icons and the
site's Montserrat, so it stays in step with both.
"""

from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ICON = ROOT / "app" / "icons" / "icon.png"
FONT = ROOT / "site" / "fonts" / "Montserrat-Bold.ttf"
OUT = ROOT / "assets" / "framecorder-banner.png"
INK = (30, 30, 46, 255)          # catppuccin mocha base, the icon's own background
SS = 4                           # supersampling for smooth edges


def background(size):
    """blurry colour field in the site's palette: mauve and lavender with a little blue"""
    W, H = size
    rng = np.random.default_rng(7)
    img = Image.new("RGB", size, (214, 190, 245))
    d = ImageDraw.Draw(img)
    blobs = [((203, 166, 247), 0.12, 0.30, 0.55), ((180, 190, 254), 0.85, 0.25, 0.60),
             ((245, 194, 231), 0.55, 0.95, 0.50), ((137, 180, 250), 0.95, 0.90, 0.45),
             ((198, 160, 246), 0.35, 0.05, 0.45), ((230, 215, 250), 0.60, 0.40, 0.40)]
    for color, cx, cy, r in blobs:
        rx, ry = r * W * 0.55, r * H * 1.1
        x, y = cx * W, cy * H
        d.ellipse([x - rx, y - ry, x + rx, y + ry], fill=color)
    img = img.filter(ImageFilter.GaussianBlur(120))
    # a whisper of grain so the gradient doesn't band
    grain = rng.normal(0, 2.2, (H, W, 1))
    arr = np.clip(np.asarray(img, float) + grain, 0, 255).astype(np.uint8)
    return Image.fromarray(arr).convert("RGBA")


def banner(size=(1500, 500)):
    W, H = size
    bg = background(size)
    font = ImageFont.truetype(str(FONT), 132)
    text = "framecorder"
    tb = ImageDraw.Draw(bg).textbbox((0, 0), text, font=font)
    tw, th = tb[2] - tb[0], tb[3] - tb[1]
    icon = Image.open(ICON).convert("RGBA").resize((170, 170), Image.LANCZOS)
    gap, pad_x, pad_y = 34, 64, 40

    # pill hugs the content, centred on the banner
    pw = icon.width + gap + tw + 2 * pad_x
    ph = max(icon.height, th) + 2 * pad_y
    px0, py0 = (W - pw) // 2, (H - ph) // 2
    px1, py1 = px0 + pw, py0 + ph
    radius = ph * 0.28

    shadow = Image.new("RGBA", size, (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle([px0, py0 + 6, px1, py1 + 6], radius=radius, fill=(60, 30, 110, 50))
    bg = Image.alpha_composite(bg, shadow.filter(ImageFilter.GaussianBlur(14)))
    s = SS
    layer = Image.new("RGBA", (W * s, H * s), (0, 0, 0, 0))
    ImageDraw.Draw(layer).rounded_rectangle([px0 * s, py0 * s, px1 * s, py1 * s], radius=radius * s,
                                           fill=(255, 255, 255, 250))
    bg = Image.alpha_composite(bg, layer.resize(size, Image.LANCZOS))

    left, mid = px0 + pad_x, (py0 + py1) // 2
    bg.alpha_composite(icon, (left, mid - icon.height // 2))
    ImageDraw.Draw(bg).text((left + icon.width + gap - tb[0], mid - th // 2 - tb[1]), text, font=font, fill=INK)
    bg.convert("RGB").save(OUT)


if __name__ == "__main__":
    banner()
    print("wrote", OUT)
