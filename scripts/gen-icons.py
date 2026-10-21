#!/usr/bin/env python3
"""Generate DeskCap icons the official way — do not hand-roll ICO.

Sources (Microsoft Learn + Tauri docs + Apple HIG):
- Win desktop/taskbar: multi-size ICO, 32-bit alpha, PNG layers; Tauri wants
  32px first. Source = rounded logo only; outer plate knocked out; transparent
  pixels MUST be RGBA(0,0,0,0) — leftover charcoal RGB with A=0 still paints a
  square plate on some Explorer desktop paths.
- Scale mark to ~92% canvas so taskbar/desktop read larger.
- macOS Dock: opaque full-bleed from icon-masters/icon-mac.png → icon.icns.

Usage:
  python3 scripts/gen-icons.py
"""
from __future__ import annotations

import shutil
import subprocess
import sys
from collections import deque
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
ICONS = ROOT / "src-tauri" / "icons"
MAC_MASTER = ROOT / "src-tauri" / "icon-masters" / "icon-mac.png"
WIN_MASTER = ROOT / "src-tauri" / "icon-masters" / "icon-win.png"
APP_ICON = ROOT / "app-icon.png"

# Mark slightly larger than natural (~75%), but keep visible transparent
# margin so 32px taskbar still reads as rounded (0.92 made it look square again).
TARGET_FILL = 0.80


def knock_out_outer_plate(src: Image.Image, tol: int = 2) -> Image.Image:
    """Remove the solid square canvas outside the rounded mark (flood from corners)."""
    im = src.convert("RGBA")
    w, h = im.size
    px = im.load()
    bg = px[0, 0][:3]
    visited: set[tuple[int, int]] = set()
    q: deque[tuple[int, int]] = deque(
        [
            (0, 0),
            (w - 1, 0),
            (0, h - 1),
            (w - 1, h - 1),
            (w // 2, 0),
            (0, h // 2),
            (w - 1, h // 2),
            (w // 2, h - 1),
        ]
    )
    while q:
        x, y = q.popleft()
        if (x, y) in visited or not (0 <= x < w and 0 <= y < h):
            continue
        r, g, b, _a = px[x, y]
        if abs(r - bg[0]) > tol or abs(g - bg[1]) > tol or abs(b - bg[2]) > tol:
            continue
        visited.add((x, y))
        q.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)])

    out = im.copy()
    opx = out.load()
    for x, y in visited:
        # Critical: zero RGB, not (charcoal, A=0) — desktop can ignore alpha and
        # still draw the leftover plate color as a sharp square.
        opx[x, y] = (0, 0, 0, 0)

    if opx[0, 0][3] > 10:
        print("ERROR: corner still opaque after knock-out", file=sys.stderr)
        sys.exit(1)
    if opx[400, 400][3] < 200:
        print("ERROR: knock-out ate the mark body", file=sys.stderr)
        sys.exit(1)
    return out


def opaque_bbox(im: Image.Image) -> tuple[int, int, int, int]:
    px = im.load()
    w, h = im.size
    xs: list[int] = []
    ys: list[int] = []
    for y in range(h):
        for x in range(w):
            if px[x, y][3] > 20:
                xs.append(x)
                ys.append(y)
    if not xs:
        raise SystemExit("ERROR: no opaque pixels after knock-out")
    return min(xs), min(ys), max(xs), max(ys)


def zero_transparent_rgb(im: Image.Image) -> Image.Image:
    """Force A==0 pixels to (0,0,0,0) so Explorer desktop cannot paint leftover RGB."""
    im = im.convert("RGBA")
    r, g, b, a = im.split()
    z = Image.new("L", im.size, 0)
    keep = a.point(lambda p: 255 if p else 0)
    r = Image.composite(r, z, keep)
    g = Image.composite(g, z, keep)
    b = Image.composite(b, z, keep)
    return Image.merge("RGBA", (r, g, b, a))


def enlarge_mark(im: Image.Image, fill: float = TARGET_FILL) -> Image.Image:
    """Crop to mark bbox and scale up so the logo reads larger on taskbar/desktop."""
    im = im.convert("RGBA")
    x0, y0, x1, y1 = opaque_bbox(im)
    crop = im.crop((x0, y0, x1 + 1, y1 + 1))
    cw, ch = crop.size
    canvas = im.size[0]
    target = max(1, int(canvas * fill))
    scale = target / max(cw, ch)
    nw = max(1, int(round(cw * scale)))
    nh = max(1, int(round(ch * scale)))
    scaled = crop.resize((nw, nh), Image.Resampling.LANCZOS)
    out = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    out.paste(scaled, ((canvas - nw) // 2, (canvas - nh) // 2), scaled)
    return zero_transparent_rgb(out)


def ensure_win_app_icon() -> None:
    if not MAC_MASTER.exists():
        print(f"missing {MAC_MASTER}", file=sys.stderr)
        sys.exit(1)
    win = enlarge_mark(knock_out_outer_plate(Image.open(MAC_MASTER)))
    WIN_MASTER.parent.mkdir(parents=True, exist_ok=True)
    win.save(WIN_MASTER)
    win.save(APP_ICON)
    bb = opaque_bbox(win)
    fill = (bb[2] - bb[0] + 1) / win.size[0]
    print(f"wrote {WIN_MASTER} + {APP_ICON} (fill={fill:.2f}, corner={win.getpixel((0,0))})")


def mac_icns_hig() -> None:
    mac = Image.open(MAC_MASTER).convert("RGBA")
    iconset = Path("/tmp/DeskCapMacHIG.iconset")
    if iconset.exists():
        shutil.rmtree(iconset)
    iconset.mkdir()
    for s, name in [
        (16, "icon_16x16.png"),
        (32, "diana.v@example.org"),
        (32, "icon_32x32.png"),
        (64, "ivan.p@example.net"),
        (128, "icon_128x128.png"),
        (256, "wendy.h@example.net"),
        (256, "icon_256x256.png"),
        (512, "wendy.h@example.net"),
        (512, "icon_512x512.png"),
        (1024, "walt.e@example.net"),
    ]:
        mac.resize((s, s), Image.Resampling.LANCZOS).save(iconset / name, format="PNG")
    subprocess.check_call(
        ["iconutil", "-c", "icns", str(iconset), "-o", str(ICONS / "icon.icns")]
    )
    print("wrote icon.icns (Apple HIG opaque full-bleed)")


def scrub_transparent_rgb(path: Path) -> None:
    """Ensure every A==0 pixel is (0,0,0,0) in generated PNGs (desktop-safe)."""
    if path.suffix.lower() != ".png" or not path.exists():
        return
    im = Image.open(path).convert("RGBA")
    cleaned = zero_transparent_rgb(im)
    if cleaned.tobytes() != im.tobytes():
        cleaned.save(path)
        print(f"scrubbed transparent RGB in {path.name}")


def verify_ico_corners() -> None:
    import io
    import struct

    data = (ICONS / "icon.ico").read_bytes()
    _reserved, _typ, count = struct.unpack_from("<HHH", data, 0)
    for i in range(count):
        w, h, _c, _r, _p, _b, size, offset = struct.unpack_from("<BBBBHHII", data, 6 + i * 16)
        w = 256 if w == 0 else w
        blob = data[offset : offset + size]
        layer = Image.open(io.BytesIO(blob)).convert("RGBA")
        corner = layer.getpixel((0, 0))
        # Lanczos can leave A=1 fringe; RGB must be 0 so desktop never paints a plate.
        if corner[0] != 0 or corner[1] != 0 or corner[2] != 0 or corner[3] > 5:
            print(f"ERROR: ICO {w}px corner={corner}", file=sys.stderr)
            sys.exit(1)
        print(f"ICO {w}px corner OK {corner}")


def main() -> int:
    ensure_win_app_icon()
    corner = Image.open(APP_ICON).convert("RGBA").getpixel((0, 0))
    if corner != (0, 0, 0, 0):
        print(f"ERROR: app-icon corner={corner}", file=sys.stderr)
        return 1
    subprocess.check_call(
        ["pnpm", "tauri", "icon", str(APP_ICON), "-o", str(ICONS)],
        cwd=ROOT,
    )
    for p in ICONS.glob("*.png"):
        scrub_transparent_rgb(p)
    shutil.copyfile(ICONS / "32x32.png", ICONS / "tray.png")
    # WebView / window may surface public/favicon.png; keep it identical to clean Win mark
    # (old favicon still had charcoal plate RGB — desktop looked square).
    public_favicon = ROOT / "public" / "favicon.png"
    public_favicon.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ICONS / "32x32.png", public_favicon)
    print(f"wrote {public_favicon}")
    mac_icns_hig()
    out = subprocess.check_output(["identify", str(ICONS / "icon.ico")], text=True)
    print(out)
    if "PNG" not in out:
        print("ERROR: ICO not PNG-compressed", file=sys.stderr)
        return 1
    verify_ico_corners()
    print("PASS: enlarged mark + transparent RGB scrubbed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
