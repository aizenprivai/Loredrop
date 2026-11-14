#!/usr/bin/env python3
"""Generates Loredrop's original HUD icons (9x9 PNG, pixel art): hearts,
hunger, armour and air bubbles in full / half / empty variants.

Usage: python3 tools/gen_gui_textures.py   (needs Pillow)
Output: assets/textures/gui/*.png
"""
import os
from PIL import Image

OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "textures", "gui")
DARK = (24, 16, 16, 255)

HEART = [
    ".XX.XX...",
    "XXXXXXX..",
    "XXXXXXX..",
    "XXXXXXX..",
    ".XXXXX...",
    "..XXX....",
    "...X.....",
]
SHANK = [
    "....XX...",
    "...XXXX..",
    "..XXXXX..",
    ".XXXXXX..",
    "XXXXXX...",
    "XX.XX....",
    "X........",
]
SHIELD = [
    "XXXXXXX..",
    "XXXXXXX..",
    "XXXXXXX..",
    "XXXXXXX..",
    ".XXXXX...",
    "..XXX....",
    "...X.....",
]
BUBBLE = [
    "..XXX....",
    ".X...X...",
    "X.....X..",
    "X.....X..",
    "X.....X..",
    ".X...X...",
    "..XXX....",
]


def icon(shape, fill, shine, fraction):
    """fraction: 1 = full, 0.5 = half (left half filled), 0 = empty outline."""
    img = Image.new("RGBA", (9, 9), (0, 0, 0, 0))
    w = len(shape[0])
    cols = [x for row in shape for x, c in enumerate(row) if c == "X"]
    left, right = min(cols), max(cols)
    split = left + (right - left + 1) * fraction
    for y, row in enumerate(shape):
        for x, c in enumerate(row):
            if c != "X":
                continue
            # Outline pixels: neighbours outside the shape.
            edge = any(
                not (0 <= y + dy < len(shape) and 0 <= x + dx < w and shape[y + dy][x + dx] == "X")
                for dx, dy in [(1, 0), (-1, 0), (0, 1), (0, -1)]
            )
            if edge:
                img.putpixel((x, y + 1), DARK)
            elif x < split:
                img.putpixel((x, y + 1), shine if (y < 2 and x < left + 3) else fill)
            else:
                img.putpixel((x, y + 1), (60, 50, 50, 255))
    return img


SETS = {
    "heart": (HEART, (220, 30, 40, 255), (255, 150, 150, 255)),
    "food": (SHANK, (190, 120, 60, 255), (240, 190, 120, 255)),
    "armor": (SHIELD, (190, 196, 206, 255), (240, 244, 250, 255)),
}

if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for name, (shape, fill, shine) in SETS.items():
        for suffix, frac in [("full", 1.0), ("half", 0.5), ("empty", 0.0)]:
            icon(shape, fill, shine, frac).save(os.path.join(OUT, f"{name}_{suffix}.png"))
    b = Image.new("RGBA", (9, 9), (0, 0, 0, 0))
    for y, row in enumerate(BUBBLE):
        for x, c in enumerate(row):
            if c == "X":
                b.putpixel((x, y + 1), (90, 170, 255, 255))
    b.putpixel((2, 3), (230, 245, 255, 255))
    b.save(os.path.join(OUT, "air_bubble.png"))
    print(f"wrote HUD icons to {os.path.normpath(OUT)}")
