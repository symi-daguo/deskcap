#!/usr/bin/env python3
"""Generate DeskCapIME menu-bar icons (template-safe) + AppIcon.icns.

Menu bar: transparent canvas + opaque glyph alpha, multi-rep TIFF 16+32.
Brand mark is 「亖」(first character) — readable at 16pt; "SYMI" four letters vanish.
"""
from __future__ import annotations

import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "Resources"
GLYPH = "亖"
# PingFang / Hiragino for CJK; Arial has no 亖.
FONTS = (
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/Supplemental/Songti.ttc",
)


def _font(size: int) -> ImageFont.FreeTypeFont:
    for path in FONTS:
        p = Path(path)
        if not p.exists():
            continue
        try:
            return ImageFont.truetype(str(p), size, index=0)
        except OSError:
            continue
    return ImageFont.load_default()


def _fit_font(draw: ImageDraw.ImageDraw, size: int, fill_w: float, fill_h: float):
    lo, hi = max(8, size // 3), size
    best = None
    for _ in range(28):
        mid = (lo + hi) // 2
        f = _font(mid)
        bbox = draw.textbbox((0, 0), GLYPH, font=f)
        w, h = bbox[2] - bbox[0], bbox[3] - bbox[1]
        if w <= size * fill_w and h <= size * fill_h:
            best = (f, bbox)
            lo = mid + 1
        else:
            hi = mid - 1
    if not best:
        f = _font(max(10, size * 2 // 3))
        return f, draw.textbbox((0, 0), GLYPH, font=f)
    return best


def menu_glyph(size: int) -> Image.Image:
    im = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(im)
    f, bbox = _fit_font(draw, size, 0.86, 0.86)
    w, h = bbox[2] - bbox[0], bbox[3] - bbox[1]
    x = (size - w) / 2 - bbox[0]
    y = (size - h) / 2 - bbox[1]
    draw.text((x, y), GLYPH, fill=(0, 0, 0, 255), font=f)
    if size <= 32:
        a = im.split()[-1].filter(ImageFilter.MaxFilter(3))
        im = Image.merge("RGBA", (*im.split()[:3], a))
    return im


def dock_tile(size: int) -> Image.Image:
    im = Image.new("RGBA", (size, size), (0, 0, 0, 255))
    draw = ImageDraw.Draw(im)
    f, bbox = _fit_font(draw, size, 0.78, 0.78)
    w, h = bbox[2] - bbox[0], bbox[3] - bbox[1]
    x = (size - w) / 2 - bbox[0]
    y = (size - h) / 2 - bbox[1]
    draw.text((x, y), GLYPH, fill=(255, 255, 255, 255), font=f)
    return im


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for s in (16, 18, 32, 64, 128):
        menu_glyph(s).save(OUT / f"app_logo_{s}.png")
    menu_glyph(16).save(OUT / "app_logo.png")

    with tempfile.TemporaryDirectory() as td:
        t = Path(td)
        menu_glyph(16).save(t / "16.png")
        menu_glyph(32).save(t / "32.png")
        subprocess.check_call(
            [
                "tiffutil",
                "-cathidpicheck",
                str(t / "16.png"),
                str(t / "32.png"),
                "-out",
                str(OUT / "app_logo.tiff"),
            ]
        )

        iconset = t / "AppIcon.iconset"
        iconset.mkdir()
        pairs = [
            ("icon_16x16.png", 16),
            ("icon_16x16@2x.png", 32),
            ("icon_32x32.png", 32),
            ("icon_32x32@2x.png", 64),
            ("icon_128x128.png", 128),
            ("icon_128x128@2x.png", 256),
            ("icon_256x256.png", 256),
            ("icon_256x256@2x.png", 512),
            ("icon_512x512.png", 512),
            ("icon_512x512@2x.png", 1024),
        ]
        for name, s in pairs:
            dock_tile(s).save(iconset / name)
        subprocess.check_call(
            ["iconutil", "-c", "icns", str(iconset), "-o", str(OUT / "AppIcon.icns")]
        )

    # Sanity: glyph present but not a solid fill (solid tiles stretch past 22pt menu slot).
    a = Image.open(OUT / "app_logo_16.png").split()[-1]
    bbox = a.getbbox()
    assert bbox is not None, "empty menu glyph"
    opaque = sum(1 for p in a.getdata() if p > 10)
    assert opaque < 16 * 16 * 0.92, f"menu glyph too solid: {opaque} px"
    print("gen-icons ok", GLYPH, OUT / "app_logo.tiff")


if __name__ == "__main__":
    main()
