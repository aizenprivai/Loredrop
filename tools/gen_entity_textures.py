#!/usr/bin/env python3
"""Generates Loredrop's original creature skins: 16x16 PNG tiles used on the
faces of each creature's box model (face, head side, body, legs...).

Usage: python3 tools/gen_entity_textures.py   (needs Pillow)
Output: assets/textures/entity/*.png
"""
import os
import random
from PIL import Image

OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "textures", "entity")
N = 16


def clamp(v):
    return max(0, min(255, int(v)))


def fill(seed, base, spread=10, patches=None, patch_chance=0.0):
    rng = random.Random(seed)
    img = Image.new("RGBA", (N, N))
    # Patches: blobs of a second colour (cow spots, dirt on pigs...).
    centers = [(rng.uniform(0, N), rng.uniform(0, N), rng.uniform(2, 5)) for _ in range(4)] if patches else []
    for y in range(N):
        for x in range(N):
            c = base
            for cx, cy, r in centers:
                if (x - cx) ** 2 + (y - cy) ** 2 < r * r:
                    c = patches
            k = rng.uniform(-spread, spread)
            img.putpixel((x, y), tuple(clamp(v + k) for v in c) + (255,))
    return img


def face(base, eye, extra=None, eye_y=6, spread=8, seed=1, eye_white=True):
    img = fill(seed, base, spread)
    for ex in (4, 10):
        if eye_white:
            img.putpixel((ex - 1, eye_y), (240, 240, 240, 255))
        img.putpixel((ex, eye_y), eye + (255,))
        img.putpixel((ex, eye_y + 1), eye + (255,))
    if extra:
        extra(img)
    return img


def snout(color, dark):
    def f(img):
        for x in range(5, 11):
            for y in range(9, 13):
                img.putpixel((x, y), color + (255,))
        img.putpixel((6, 10), dark + (255,))
        img.putpixel((9, 10), dark + (255,))
    return f


def mouth(color):
    def f(img):
        for x in range(5, 11):
            img.putpixel((x, 11), color + (255,))
    return f


def beak(img):
    for x in range(6, 10):
        img.putpixel((x, 9), (240, 180, 40, 255))
        img.putpixel((x, 10), (220, 150, 30, 255))
    for x in range(7, 9):
        img.putpixel((x, 11), (200, 30, 30, 255))
        img.putpixel((x, 12), (200, 30, 30, 255))


def ribs(seed):
    img = fill(seed, (40, 40, 44), 4)
    for y in range(1, 15, 3):
        for x in range(2, 14):
            img.putpixel((x, y), (200, 200, 196, 255))
    for y in range(N):
        img.putpixel((7, y), (210, 210, 206, 255))
        img.putpixel((8, y), (190, 190, 186, 255))
    return img


def hollow(seed, cracks=3, blank_face=False):
    """Weathered stone-grey skin with cracks glowing faint gold (Loredrop original)."""
    rng = random.Random(seed)
    img = fill(seed, (122, 120, 116), 10)
    for _ in range(cracks):
        x, y = rng.randrange(N), rng.randrange(N)
        for _ in range(rng.randrange(5, 10)):
            img.putpixel((x % N, y % N), (232, 196, 96, 255))  # the gold of the lore tablets
            if rng.random() < 0.5:
                img.putpixel(((x + 1) % N, y % N), (150, 120, 60, 255))
            x += rng.choice([-1, 0, 1])
            y += 1
    if blank_face:
        # A smooth, featureless face: slightly lighter, no eyes or mouth.
        for y in range(3, 13):
            for x in range(3, 13):
                r, g, b, a = img.getpixel((x, y))
                img.putpixel((x, y), (min(255, r + 14), min(255, g + 14), min(255, b + 14), a))
    return img


TEXTURES = {
    # The Hollow (Loredrop original creature)
    "hollow_face": lambda: hollow(80, cracks=1, blank_face=True),
    "hollow_head": lambda: hollow(81, cracks=2),
    "hollow_body": lambda: hollow(82, cracks=4),
    "hollow_limb": lambda: hollow(83, cracks=2),
    # Cow: brown with cream patches
    "cow_face": lambda: face((96, 64, 40), (20, 20, 20), snout((200, 170, 150), (60, 40, 30)), seed=10),
    "cow_head": lambda: fill(11, (96, 64, 40), 8),
    "cow_body": lambda: fill(12, (96, 64, 40), 8, patches=(226, 214, 196)),
    "cow_leg": lambda: fill(13, (86, 58, 36), 8),
    # Pig: pink
    "pig_face": lambda: face((236, 162, 162), (30, 20, 20), snout((250, 190, 190), (150, 80, 80)), seed=20),
    "pig_body": lambda: fill(21, (236, 162, 162), 6),
    "pig_leg": lambda: fill(22, (220, 146, 146), 6),
    # Sheep: wool and a pale face
    "sheep_face": lambda: face((214, 190, 166), (20, 20, 20), mouth((150, 120, 100)), seed=30),
    "sheep_wool": lambda: fill(31, (234, 234, 230), 12),
    "sheep_leg": lambda: fill(32, (214, 190, 166), 6),
    # Chicken: white feathers, yellow legs, beak and wattle
    "chicken_face": lambda: face((240, 240, 236), (20, 20, 20), beak, seed=40, eye_white=False),
    "chicken_body": lambda: fill(41, (240, 240, 236), 8),
    "chicken_leg": lambda: fill(42, (230, 170, 40), 6),
    # Zombie: grey-green skin, torn blue-green shirt, dark trousers
    "zombie_face": lambda: face((96, 140, 96), (20, 20, 20), mouth((50, 80, 50)), seed=50, eye_white=False),
    "zombie_head": lambda: fill(51, (90, 132, 90), 8),
    "zombie_body": lambda: fill(52, (52, 120, 128), 10, patches=(96, 140, 96)),
    "zombie_arm": lambda: fill(53, (96, 140, 96), 8),
    "zombie_leg": lambda: fill(54, (58, 52, 110), 8),
    # Skeleton: bone white with dark gaps
    "skeleton_face": lambda: face((200, 200, 196), (10, 10, 10), mouth((60, 60, 60)), seed=60, eye_white=False),
    "skeleton_head": lambda: fill(61, (200, 200, 196), 6),
    "skeleton_body": lambda: ribs(62),
    "skeleton_limb": lambda: fill(63, (200, 200, 196), 6),
    # Spider: dark hairy body, red eyes
    "spider_face": lambda: face((44, 38, 36), (220, 30, 30), None, eye_y=5, seed=70, eye_white=False),
    "spider_body": lambda: fill(71, (44, 38, 36), 10),
    "spider_leg": lambda: fill(72, (34, 30, 28), 6),
}

if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for name, fn in TEXTURES.items():
        fn().save(os.path.join(OUT, name + ".png"))
    print(f"wrote {len(TEXTURES)} entity textures to {os.path.normpath(OUT)}")
