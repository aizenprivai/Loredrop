#!/usr/bin/env python3
"""Draw the calculator app icon (55x56, the size NumWorks requires).

An isometric grass block built from the PC game's own grass and dirt
textures, on the calculator's white home-screen background, with a thin
gold crack line (Loredrop's signature colour).

    python3 numworks/tools/gen_icon.py
    npx --yes -- nwlink@1.0.0 png-nwi numworks/app/icon.png numworks/app/icon.nwi
"""

import pathlib

from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parents[2]
TEX = ROOT / "assets" / "textures" / "block"
OUT = ROOT / "numworks" / "app" / "icon.png"

W, H = 55, 56


def tinted(name, tint):
    img = Image.open(TEX / f"{name}.png").convert("RGBA")
    if tint:
        px = img.load()
        for y in range(16):
            for x in range(16):
                r, g, b, a = px[x, y]
                px[x, y] = (r * tint[0] // 255, g * tint[1] // 255, b * tint[2] // 255, a)
    return img


def main():
    top = tinted("grass_block_top", (0x91, 0xBD, 0x59))
    side = tinted("grass_block_side", None)
    icon = Image.new("RGBA", (W, H), (255, 255, 255, 255))
    out = icon.load()
    # Isometric cube: each face texel is mapped by inverse projection.
    cx, cy, s = 27.5, 8.0, 1.45  # centre top, scale (pixels per texel)
    for py in range(H):
        for px in range(W):
            x, y = px + 0.5 - cx, py + 0.5 - cy
            # Top face: u along (+1, +0.5), v along (-1, +0.5).
            u = (x / s + 2 * y / s) / 2
            v = (2 * y / s - x / s) / 2
            if 0 <= u < 16 and 0 <= v < 16:
                out[px, py] = top.getpixel((int(u), int(v)))
                continue
            # Side faces: `lx` is the texel column across the cube's width
            # (-16..0 left face, 0..16 right face); `t` is the texel row
            # going down from the face's top edge.
            lx = x / s
            if -16 <= lx < 0:
                # Left face, shaded to 80 %.
                t = y / s - (16 + lx * 0.5)
                if 0 <= t < 16:
                    r, g, b, a = side.getpixel((int(lx + 16), int(t)))
                    out[px, py] = (r * 80 // 100, g * 80 // 100, b * 80 // 100, 255)
                    continue
            if 0 <= lx < 16:
                # Right face, shaded to 60 %.
                t = y / s - (16 - lx * 0.5)
                if 0 <= t < 16:
                    r, g, b, a = side.getpixel((int(lx), int(t)))
                    out[px, py] = (r * 60 // 100, g * 60 // 100, b * 60 // 100, 255)
    # Gold crack down the front edge.
    for py in range(int(cy + 16 * s) + 6, int(cy + 32 * s) - 2):
        if py % 5:
            out[int(cx) + (py // 5) % 2, py] = (0xE8, 0xB6, 0x4C, 255)
    icon.convert("RGB").save(OUT)
    print(f"wrote {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
