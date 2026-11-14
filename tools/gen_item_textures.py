#!/usr/bin/env python3
"""Generates Loredrop's original placeholder ITEM sprites (16x16 PNG).

Tools are drawn as a diagonal wooden handle (bottom-left → top-right) plus a
head whose shape depends on the tool type and whose colour depends on the
tier. Names follow Minecraft Java resource-pack names so packs can override.

Usage: python3 tools/gen_item_textures.py   (needs Pillow)
Output: assets/textures/item/*.png
"""
import os
import random
from PIL import Image

OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "textures", "item")
N = 16
HANDLE = (110, 78, 40)
HANDLE_DARK = (74, 52, 26)
OUTLINE = (30, 24, 18)


def clamp(v):
    return max(0, min(255, int(v)))


def blank():
    return Image.new("RGBA", (N, N), (0, 0, 0, 0))


def put(img, x, y, c):
    if 0 <= x < N and 0 <= y < N:
        img.putpixel((x, y), tuple(clamp(v) for v in c) + (255,))


def handle(img, start=1, end=10):
    for i in range(start, end):
        x, y = i, N - 1 - i
        put(img, x, y, HANDLE if i % 3 else HANDLE_DARK)
        put(img, x + 1, y, OUTLINE)


def shade(c, k):
    return tuple(v * k for v in c)


def pickaxe(color):
    img = blank()
    handle(img, 1, 11)
    # Curved head along the top-right
    pts = [(5, 2), (6, 1), (7, 1), (8, 1), (9, 1), (10, 2), (11, 2), (12, 3), (13, 4), (13, 5), (14, 6), (14, 7), (14, 8), (13, 9)]
    for i, (x, y) in enumerate(pts):
        put(img, x, y, shade(color, 1.0 if i % 3 else 0.8))
        put(img, x, y + 1, shade(color, 0.6))
    return img


def axe(color):
    img = blank()
    handle(img, 1, 11)
    for y in range(1, 7):
        for x in range(8, 14):
            if (x - 8) + (6 - y) < 9 and not (x == 13 and y in (1, 6)):
                put(img, x, y, shade(color, 1.0 if (x + y) % 4 else 0.75))
    for y in range(1, 7):
        put(img, 7, y + 1, shade(color, 0.55))
    return img


def shovel(color):
    img = blank()
    handle(img, 1, 10)
    for y in range(1, 6):
        for x in range(10, 15):
            if abs((x - 12) + (y - 3)) <= 3 and abs((x - 12) - (y - 3)) <= 2:
                put(img, x, y, shade(color, 1.0 if (x * y) % 5 else 0.75))
    return img


def hoe(color):
    img = blank()
    handle(img, 1, 11)
    for x in range(7, 13):
        put(img, x, 2, shade(color, 1.0))
        put(img, x, 3, shade(color, 0.7))
    put(img, 6, 3, shade(color, 0.6))
    return img


def sword(color):
    img = blank()
    for i in range(1, 4):  # grip
        put(img, i, N - 1 - i, HANDLE)
    for d in (-1, 0, 1, 2):  # cross-guard
        put(img, 4 + d, N - 5 + d, shade(color, 0.6) if color != HANDLE else HANDLE_DARK)
    for i in range(5, 15):  # blade
        x, y = i, N - 1 - i
        put(img, x, y, shade(color, 1.0))
        put(img, x - 1, y, shade(color, 0.75))
        put(img, x, y + 1, shade(color, 0.55))
    return img


def gem(color, seed):
    rng = random.Random(seed)
    img = blank()
    for y in range(3, 13):
        for x in range(3, 13):
            if abs(x - 7.5) + abs(y - 7.5) <= 5:
                k = 1.25 if x + y < 14 else 0.85
                put(img, x, y, shade(color, k * rng.uniform(0.9, 1.05)))
    put(img, 6, 5, (255, 255, 255))
    return img


def lump(color, seed):
    rng = random.Random(seed)
    img = blank()
    for y in range(4, 13):
        for x in range(3, 13):
            if (x - 7.5) ** 2 / 25 + (y - 8) ** 2 / 16 <= 1 and rng.random() > 0.06:
                put(img, x, y, shade(color, rng.uniform(0.7, 1.1)))
    return img


def ingot(color):
    img = blank()
    for y in range(6, 11):
        for x in range(2 + (10 - y), 14 - (y - 6) // 2):
            put(img, x, y, shade(color, 1.15 if y == 6 else (0.7 if y == 10 else 1.0)))
    return img


def stick():
    img = blank()
    handle(img, 2, 14)
    return img


def armor_piece(shape, color):
    img = blank()
    rows = {
        "helmet": ["..XXXXXXXXXX..", ".XXXXXXXXXXXX.", ".XX........XX.", ".XX........XX."],
        "chestplate": ["XXX..XXX..XXX.", "XXXXXXXXXXXXX.", ".XXXXXXXXXXX..", "..XXXXXXXXX...", "..XXXXXXXXX...", "..XXXXXXXXX...", "..XXXXXXXXX...", "..XXXXXXXXX..."],
        "leggings": ["XXXXXXXXXXXX..", "XXXXXXXXXXXX..", "XXXX....XXXX..", "XXXX....XXXX..", "XXXX....XXXX..", "XXXX....XXXX..", "XXX......XXX.."],
        "boots": ["XXX......XXX..", "XXX......XXX..", "XXXXX....XXXXX", "XXXXX....XXXXX"],
    }[shape]
    top = (16 - len(rows)) // 2
    for y, row in enumerate(rows):
        for x, c in enumerate(row):
            if c == "X":
                k = 1.15 if y == 0 else (0.75 if y == len(rows) - 1 else 1.0)
                put(img, x + 1, y + top, shade(color, k))
    return img


TIERS = {"wooden": (150, 112, 62), "stone": (128, 128, 128), "iron": (222, 222, 222),
         "golden": (250, 214, 64), "diamond": (90, 232, 222)}
SHAPES = {"pickaxe": pickaxe, "axe": axe, "shovel": shovel, "hoe": hoe, "sword": sword}

def apple():
    img = blank()
    for y in range(3, 15):
        for x in range(2, 14):
            if (x - 7.5) ** 2 / 30 + (y - 9) ** 2 / 30 <= 1:
                k = 1.25 if (x < 7 and y < 9) else 1.0
                put(img, x, y, (200 * k, 30 * k, 36 * k))
    for y in range(1, 4):
        put(img, 8, y, HANDLE)  # stem
    put(img, 9, 2, (60, 150, 40)); put(img, 10, 2, (60, 150, 40)); put(img, 10, 1, (60, 150, 40))
    put(img, 5, 6, (255, 200, 200))
    return img


ITEMS = {
    "stick": stick,
    "apple": apple,
    "coal": lambda: lump((44, 44, 48), 1), "charcoal": lambda: lump((60, 48, 36), 2),
    "flint": lambda: lump((70, 70, 76), 3),
    "raw_iron": lambda: lump((200, 160, 130), 4), "raw_copper": lambda: lump((200, 112, 70), 5),
    "raw_gold": lambda: lump((240, 200, 60), 6),
    "iron_ingot": lambda: ingot((216, 216, 216)), "copper_ingot": lambda: ingot((214, 120, 80)),
    "gold_ingot": lambda: ingot((250, 214, 64)),
    "diamond": lambda: gem((90, 232, 222), 7), "emerald": lambda: gem((40, 210, 100), 8),
    "lapis_lazuli": lambda: gem((40, 70, 200), 9),
}
def meat(raw, seed):
    img = lump(raw, seed)
    for y in range(6, 11):
        put(img, 5, y, (240, 230, 220))  # bone / fat streak
    return img


def feather():
    img = blank()
    for i in range(2, 14):
        put(img, i, 15 - i, (150, 150, 150))
        for d in (1, 2):
            if i > 4:
                put(img, i - d, 15 - i - d, (240, 240, 240))
                put(img, i + d, 15 - i + d, (225, 225, 225))
    return img


def bone():
    img = blank()
    for i in range(4, 12):
        put(img, i, 15 - i, (230, 228, 214))
    for (x, y) in [(3, 12), (2, 12), (3, 13), (12, 3), (13, 3), (12, 2)]:
        put(img, x, y, (240, 238, 226))
    return img


def string_item():
    img = blank()
    import math
    for t in range(60):
        x = 3 + t * 10 / 60
        y = 8 + math.sin(t / 5) * 3
        put(img, int(x), int(y), (240, 240, 240))
    return img


def arrow():
    img = blank()
    for i in range(3, 13):
        put(img, i, 15 - i, HANDLE)
    for (x, y) in [(13, 2), (12, 2), (13, 3), (11, 3), (12, 4)]:
        put(img, x, y, (180, 180, 186))
    for (x, y) in [(2, 12), (3, 13), (2, 13), (1, 14), (3, 14)]:
        put(img, x, y, (240, 240, 240))
    return img


ITEMS.update({
    "beef": lambda: meat((190, 40, 40), 20), "cooked_beef": lambda: meat((120, 66, 40), 21),
    "porkchop": lambda: meat((236, 140, 140), 22), "cooked_porkchop": lambda: meat((200, 140, 90), 23),
    "chicken": lambda: meat((240, 200, 190), 24), "cooked_chicken": lambda: meat((210, 150, 80), 25),
    "mutton": lambda: meat((200, 60, 60), 26), "cooked_mutton": lambda: meat((150, 80, 50), 27),
    "rotten_flesh": lambda: lump((120, 100, 60), 28), "leather": lambda: lump((150, 90, 50), 29),
    "feather": feather, "bone": bone, "string": string_item, "arrow": arrow,
})
for metal, color in {"leather": (150, 90, 50), "iron": (222, 222, 222), "golden": (250, 214, 64), "diamond": (90, 232, 222)}.items():
    for piece in ("helmet", "chestplate", "leggings", "boots"):
        ITEMS[f"{metal}_{piece}"] = (lambda p=piece, c=color: lambda: armor_piece(p, c))()
for tier, color in TIERS.items():
    for shape, fn in SHAPES.items():
        ITEMS[f"{tier}_{shape}"] = (lambda f=fn, c=color: lambda: f(c))()

if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for name, fn in ITEMS.items():
        fn().save(os.path.join(OUT, name + ".png"))
    print(f"wrote {len(ITEMS)} item textures to {os.path.normpath(OUT)}")
