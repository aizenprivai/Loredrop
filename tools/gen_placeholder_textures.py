#!/usr/bin/env python3
"""Generates Loredrop's original placeholder block textures (16x16 PNG).

These are simple procedural pixel-art stand-ins, created for this project and
free to replace. File names follow Minecraft Java resource-pack names so that
player-supplied packs can override them later (GDD 7.2).

Usage:  python3 tools/gen_placeholder_textures.py   (needs Pillow)
Output: assets/textures/block/*.png
"""
import os
import random
from PIL import Image

OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "textures", "block")
N = 16


def clamp(v):
    return max(0, min(255, int(v)))


def noise_tex(seed, base, spread, specks=(), speck_chance=0.0, gray=False):
    """Random-noise texture around `base` colour, with optional coloured specks."""
    rng = random.Random(seed)
    img = Image.new("RGBA", (N, N))
    for y in range(N):
        for x in range(N):
            k = rng.uniform(-spread, spread)
            c = [b + k for b in base]
            if specks and rng.random() < speck_chance:
                c = list(rng.choice(specks))
            if gray:
                g = sum(c) / 3
                c = [g, g, g]
            img.putpixel((x, y), tuple(clamp(v) for v in c) + (255,))
    return img


def stone():
    return noise_tex(1, (124, 124, 124), 12, specks=[(100, 100, 100), (146, 146, 146)], speck_chance=0.12)


def cobblestone():
    rng = random.Random(2)
    img = Image.new("RGBA", (N, N))
    # Irregular stones: assign each pixel to the nearest of a few seed points.
    seeds = [(rng.uniform(0, N), rng.uniform(0, N), rng.uniform(95, 150)) for _ in range(9)]
    for y in range(N):
        for x in range(N):
            d = sorted(((min(abs(x - sx), N - abs(x - sx)) ** 2 + min(abs(y - sy), N - abs(y - sy)) ** 2, sh)
                        for sx, sy, sh in seeds))
            edge = (d[1][0] ** 0.5 - d[0][0] ** 0.5) < 1.1
            v = 62 if edge else d[0][1] + rng.uniform(-8, 8)
            img.putpixel((x, y), (clamp(v), clamp(v), clamp(v), 255))
    return img


def dirt():
    return noise_tex(3, (122, 86, 58), 14, specks=[(94, 64, 42), (150, 112, 80)], speck_chance=0.15)


def grass_top():
    # Greyscale: the shader tints it green (biome colour later).
    return noise_tex(4, (170, 170, 170), 26, gray=True)


def grass_side():
    img = dirt()
    rng = random.Random(5)
    for x in range(N):
        depth = 3 + rng.choice([0, 0, 1, 1, 2])
        for y in range(depth):
            g = rng.uniform(-18, 18)
            img.putpixel((x, y), (clamp(98 + g), clamp(150 + g), clamp(66 + g), 255))
    return img


def sand():
    return noise_tex(6, (219, 207, 163), 9, specks=[(200, 186, 140)], speck_chance=0.1)


def gravel():
    return noise_tex(7, (130, 124, 120), 22, specks=[(96, 90, 88), (160, 150, 140), (120, 100, 90)], speck_chance=0.3)


def bedrock():
    return noise_tex(8, (80, 80, 80), 40, specks=[(30, 30, 30), (130, 130, 130)], speck_chance=0.25)


def oak_log():
    rng = random.Random(9)
    img = Image.new("RGBA", (N, N))
    stripe = [rng.uniform(-14, 14) for _ in range(N)]
    for y in range(N):
        for x in range(N):
            v = stripe[x] + rng.uniform(-6, 6) + (-18 if x % 4 == 0 else 0)
            img.putpixel((x, y), (clamp(104 + v), clamp(82 + v), clamp(50 + v), 255))
    return img


def oak_log_top():
    rng = random.Random(10)
    img = Image.new("RGBA", (N, N))
    for y in range(N):
        for x in range(N):
            r = ((x - 7.5) ** 2 + (y - 7.5) ** 2) ** 0.5
            if r > 7.0:  # bark rim
                v = rng.uniform(-8, 8)
                c = (104 + v, 82 + v, 50 + v)
            else:
                ring = -16 if int(r) % 2 == 0 else 0
                v = ring + rng.uniform(-5, 5)
                c = (178 + v, 146 + v, 92 + v)
            img.putpixel((x, y), tuple(clamp(t) for t in c) + (255,))
    return img


def oak_planks():
    rng = random.Random(11)
    img = Image.new("RGBA", (N, N))
    for y in range(N):
        offset = (y // 4) * 5
        for x in range(N):
            v = rng.uniform(-8, 8)
            if y % 4 == 3 or (x + offset) % 16 == 0:
                v -= 34  # gaps between boards
            img.putpixel((x, y), (clamp(166 + v), clamp(134 + v), clamp(82 + v), 255))
    return img


def oak_leaves():
    rng = random.Random(12)
    img = noise_tex(12, (150, 150, 150), 40, gray=True)  # tinted by foliage colour
    for y in range(N):
        for x in range(N):
            if rng.random() < 0.18:
                img.putpixel((x, y), (0, 0, 0, 0))  # holes you can see through
    return img


def glass():
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    edge = (220, 235, 240, 255)
    for i in range(N):
        for p in [(i, 0), (i, N - 1), (0, i), (N - 1, i)]:
            img.putpixel(p, edge)
    for i in range(3, 7):  # a small highlight streak
        img.putpixel((i, 9 - i), (240, 250, 255, 255))
    return img


TEXTURES = {
    "stone": stone, "cobblestone": cobblestone, "dirt": dirt,
    "grass_block_top": grass_top, "grass_block_side": grass_side,
    "sand": sand, "gravel": gravel, "bedrock": bedrock,
    "oak_log": oak_log, "oak_log_top": oak_log_top, "oak_planks": oak_planks,
    "oak_leaves": oak_leaves, "glass": glass,
}

if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for name, fn in TEXTURES.items():
        fn().save(os.path.join(OUT, name + ".png"))
    print(f"wrote {len(TEXTURES)} textures to {os.path.normpath(OUT)}")
