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



# ---------------------------------------------------------------------------
# Phase 4 textures
# ---------------------------------------------------------------------------

def deepslate():
    rng = random.Random(20)
    img = Image.new("RGBA", (N, N))
    for y in range(N):
        for x in range(N):
            v = 72 + rng.uniform(-10, 10) + (-10 if y % 4 == 0 else 0)  # layered look
            img.putpixel((x, y), (clamp(v), clamp(v), clamp(v + 6), 255))
    return img


def ore(base_fn, color, seed, cluster=5):
    """Host rock with a few clusters of mineral pixels."""
    img = base_fn()
    rng = random.Random(seed)
    for _ in range(cluster):
        cx, cy = rng.randrange(1, N - 2), rng.randrange(1, N - 2)
        for dx, dy in [(0, 0), (1, 0), (0, 1), (1, 1), (rng.choice([-1, 2]), rng.choice([0, 1]))]:
            x, y = (cx + dx) % N, (cy + dy) % N
            k = rng.uniform(-20, 20)
            img.putpixel((x, y), tuple(clamp(c + k) for c in color) + (255,))
    return img


ORES = {
    "coal": (40, 40, 40), "iron": (216, 175, 147), "copper": (196, 110, 70),
    "gold": (250, 210, 60), "diamond": (90, 230, 220), "lapis": (40, 70, 190),
    "emerald": (40, 200, 90),
}


def water():
    rng = random.Random(21)
    img = Image.new("RGBA", (N, N))
    for y in range(N):
        for x in range(N):
            wave = 12 if (x + y * 2 + rng.randrange(3)) % 7 == 0 else 0
            v = 175 + rng.uniform(-10, 10) + wave
            img.putpixel((x, y), (clamp(v), clamp(v), clamp(v), 180))  # grey: tinted by biome
    return img


def lava():
    rng = random.Random(22)
    img = Image.new("RGBA", (N, N))
    for y in range(N):
        for x in range(N):
            k = rng.uniform(-30, 30)
            hot = rng.random() < 0.15
            c = (255, 220 + k / 2, 90) if hot else (220 + k, 90 + k, 20)
            img.putpixel((x, y), tuple(clamp(v) for v in c) + (255,))
    return img


def snow():
    return noise_tex(23, (242, 248, 250), 6)


def grass_block_snow():
    img = dirt()
    rng = random.Random(24)
    for x in range(N):
        depth = 3 + rng.choice([0, 1, 1, 2])
        for y in range(depth):
            v = 240 + rng.uniform(-8, 8)
            img.putpixel((x, y), (clamp(v), clamp(v), clamp(v + 4), 255))
    return img


def log_side(seed, base, dark):
    rng = random.Random(seed)
    img = Image.new("RGBA", (N, N))
    for y in range(N):
        for x in range(N):
            k = rng.uniform(-10, 10)
            c = dark if rng.random() < 0.12 else base
            img.putpixel((x, y), tuple(clamp(v + k) for v in c) + (255,))
    return img


def birch_log():
    rng = random.Random(25)
    img = log_side(25, (220, 218, 210), (40, 40, 36))
    for _ in range(6):  # dark horizontal marks
        y, x0 = rng.randrange(N), rng.randrange(N)
        for dx in range(rng.randrange(2, 5)):
            img.putpixel(((x0 + dx) % N, y), (45, 44, 40, 255))
    return img


def log_top(seed, ring_base, bark):
    rng = random.Random(seed)
    img = Image.new("RGBA", (N, N))
    for y in range(N):
        for x in range(N):
            r = ((x - 7.5) ** 2 + (y - 7.5) ** 2) ** 0.5
            if r > 7.0:
                c = tuple(b + rng.uniform(-8, 8) for b in bark)
            else:
                ring = -16 if int(r) % 2 == 0 else 0
                c = tuple(b + ring + rng.uniform(-5, 5) for b in ring_base)
            img.putpixel((x, y), tuple(clamp(t) for t in c) + (255,))
    return img


def planks(seed, base):
    rng = random.Random(seed)
    img = Image.new("RGBA", (N, N))
    for y in range(N):
        offset = (y // 4) * 5
        for x in range(N):
            v = rng.uniform(-8, 8)
            if y % 4 == 3 or (x + offset) % 16 == 0:
                v -= 30
            img.putpixel((x, y), tuple(clamp(b + v) for b in base) + (255,))
    return img


def leaves(seed, holes=0.18, coloured=None):
    rng = random.Random(seed)
    img = noise_tex(seed, (150, 150, 150), 40, gray=True)
    for y in range(N):
        for x in range(N):
            if rng.random() < holes:
                img.putpixel((x, y), (0, 0, 0, 0))
            elif coloured:
                g = img.getpixel((x, y))[0] / 150
                img.putpixel((x, y), tuple(clamp(c * g) for c in coloured) + (255,))
    return img


def cactus_side():
    rng = random.Random(26)
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    for y in range(N):
        for x in range(1, N - 1):
            k = rng.uniform(-10, 10) + (-25 if x in (4, 11) else 0)
            img.putpixel((x, y), (clamp(60 + k), clamp(130 + k), clamp(50 + k), 255))
        for x in (2, 8, 13):
            if y % 4 == 1:
                img.putpixel((x, y), (230, 230, 200, 255))  # spines
    return img


def cactus_top():
    img = noise_tex(27, (80, 150, 60), 12)
    for i in range(N):
        for p in [(i, 0), (i, N - 1), (0, i), (N - 1, i)]:
            img.putpixel(p, (0, 0, 0, 0))
    return img


def plant(seed, stems=7, tall=12, colour=None):
    """Cross-shaped plant (greyscale unless `colour`, so grass/ferns get tinted)."""
    rng = random.Random(seed)
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    for _ in range(stems):
        x = rng.randrange(1, N - 1)
        h = rng.randrange(tall // 2, tall)
        lean = rng.choice([-1, 0, 1])
        for i in range(h):
            px = x + (lean if i > h // 2 else 0)
            v = 120 + rng.uniform(-30, 40)
            c = (v, v, v) if colour is None else tuple(cc + rng.uniform(-15, 15) for cc in colour)
            img.putpixel((px % N, N - 1 - i), tuple(clamp(t) for t in c) + (255,))
    return img


def flower(seed, petal):
    rng = random.Random(seed)
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    for i in range(8):  # stem
        img.putpixel((7, N - 1 - i), (60, 120, 40, 255))
    img.putpixel((6, N - 4), (70, 140, 50, 255))
    for dx, dy in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)]:
        k = rng.uniform(-15, 15)
        c = (250, 230, 120) if (dx, dy) == (0, 0) else tuple(cc + k for cc in petal)
        img.putpixel((7 + dx, 5 + dy), tuple(clamp(t) for t in c) + (255,))
    return img


def sandstone(seed, bands=True):
    rng = random.Random(seed)
    img = Image.new("RGBA", (N, N))
    for y in range(N):
        for x in range(N):
            k = rng.uniform(-7, 7) + (-14 if bands and y in (3, 11) else 0)
            img.putpixel((x, y), (clamp(214 + k), clamp(200 + k), clamp(152 + k), 255))
    return img


TEXTURES.update({
    "deepslate": deepslate, "deepslate_top": deepslate,
    "water_still": water, "lava_still": lava, "snow": snow, "grass_block_snow": grass_block_snow,
    "birch_log": birch_log, "birch_log_top": lambda: log_top(28, (205, 186, 130), (220, 218, 210)),
    "birch_planks": lambda: planks(29, (196, 178, 122)), "birch_leaves": lambda: leaves(30, 0.15, coloured=(128, 167, 85)),
    "spruce_log": lambda: log_side(31, (70, 48, 28), (45, 30, 18)),
    "spruce_log_top": lambda: log_top(32, (120, 86, 52), (70, 48, 28)),
    "spruce_planks": lambda: planks(33, (114, 84, 50)), "spruce_leaves": lambda: leaves(34, 0.12, coloured=(90, 140, 92)),
    "cactus_side": cactus_side, "cactus_top": cactus_top, "cactus_bottom": cactus_top,
    "short_grass": lambda: plant(35), "fern": lambda: plant(36, stems=5, tall=14),
    "dead_bush": lambda: plant(37, stems=6, tall=10, colour=(120, 86, 50)),
    "dandelion": lambda: flower(38, (240, 210, 40)), "poppy": lambda: flower(39, (210, 30, 30)),
    "cornflower": lambda: flower(40, (80, 110, 230)),
    "sandstone": lambda: sandstone(41), "sandstone_top": lambda: sandstone(42, bands=False),
    "sandstone_bottom": lambda: sandstone(43, bands=False),
    "granite": lambda: noise_tex(44, (150, 104, 86), 16, specks=[(180, 130, 110), (110, 80, 70)], speck_chance=0.2),
    "diorite": lambda: noise_tex(45, (190, 190, 192), 18, specks=[(230, 230, 230), (120, 120, 120)], speck_chance=0.25),
    "andesite": lambda: noise_tex(46, (136, 136, 138), 10, specks=[(110, 110, 112), (160, 160, 160)], speck_chance=0.2),
    "clay": lambda: noise_tex(47, (160, 166, 178), 6),
    "ice": lambda: noise_tex(48, (150, 190, 250), 10),
})
for name, colour in ORES.items():
    TEXTURES[f"{name}_ore"] = (lambda c=colour, s=len(name): lambda: ore(stone, c, 100 + s))()
    TEXTURES[f"deepslate_{name}_ore"] = (lambda c=colour, s=len(name): lambda: ore(deepslate, c, 200 + s))()



# ---------------------------------------------------------------------------
# Phase 4b: ruins
# ---------------------------------------------------------------------------

def bricks(seed, base, mortar, rows=4, crack=0.0, moss=0.0):
    rng = random.Random(seed)
    img = Image.new("RGBA", (N, N))
    h = N // rows
    for y in range(N):
        row = y // h
        offset = (row % 2) * (N // 4)
        for x in range(N):
            edge = (y % h == h - 1) or ((x + offset) % (N // 2) == 0)
            k = rng.uniform(-9, 9)
            c = mortar if edge else tuple(b + k for b in base)
            if not edge and moss and rng.random() < moss:
                c = (70 + k, 110 + k, 50 + k)
            img.putpixel((x, y), tuple(clamp(v) for v in c) + (255,))
    if crack:
        x, y = rng.randrange(3, 12), 0
        while y < N:
            img.putpixel((x % N, y), tuple(clamp(m - 15) for m in mortar) + (255,))
            x += rng.choice([-1, 0, 1])
            y += 1
    return img


def chiseled(seed, base, dark):
    img = bricks(seed, base, dark, rows=2)
    for i in range(4, 12):  # carved square ring
        for p in [(i, 4), (i, 11), (4, i), (11, i)]:
            img.putpixel(p, tuple(clamp(d) for d in dark) + (255,))
    return img


def lore_tablet_side():
    """Dark carved slab with rows of glyphs that glint gold — original design."""
    rng = random.Random(60)
    img = Image.new("RGBA", (N, N))
    for y in range(N):
        for x in range(N):
            k = rng.uniform(-6, 6)
            border = x in (0, N - 1) or y in (0, N - 1)
            c = (40 + k, 38 + k, 46 + k) if border else (62 + k, 60 + k, 70 + k)
            img.putpixel((x, y), tuple(clamp(v) for v in c) + (255,))
    for row in (3, 7, 11):  # three lines of glyphs
        x = 2
        while x < N - 3:
            w = rng.choice([1, 2])
            for dx in range(w):
                for dy in range(rng.choice([1, 2])):
                    img.putpixel((x + dx, row + dy), (214, 178, 84, 255))
            x += w + 1
    return img


def lore_tablet_top():
    img = noise_tex(61, (58, 56, 66), 6)
    for i in range(4, 12):
        img.putpixel((i, 7), (214, 178, 84, 255))
        img.putpixel((7, i), (214, 178, 84, 255))
    return img


TEXTURES.update({
    "stone_bricks": lambda: bricks(50, (126, 126, 126), (88, 88, 88)),
    "cracked_stone_bricks": lambda: bricks(51, (120, 120, 120), (84, 84, 84), crack=1),
    "mossy_stone_bricks": lambda: bricks(52, (122, 124, 120), (86, 88, 84), moss=0.3),
    "chiseled_stone_bricks": lambda: chiseled(53, (128, 128, 128), (80, 80, 80)),
    "deepslate_bricks": lambda: bricks(54, (70, 70, 76), (44, 44, 50)),
    "cracked_deepslate_bricks": lambda: bricks(55, (66, 66, 72), (40, 40, 46), crack=1),
    "cut_sandstone": lambda: bricks(56, (216, 202, 154), (190, 174, 124), rows=2),
    "chiseled_sandstone": lambda: chiseled(57, (214, 200, 150), (176, 158, 110)),
    "lore_tablet_side": lore_tablet_side,
    "lore_tablet_top": lore_tablet_top,
})



# ---------------------------------------------------------------------------
# Phase 5: crafting table, furnace
# ---------------------------------------------------------------------------

def table_top():
    img = planks(70, (176, 132, 80))
    for i in range(N):  # dark frame with a crafting grid
        for p in [(i, 0), (i, N - 1), (0, i), (N - 1, i)]:
            img.putpixel(p, (84, 60, 34, 255))
    for i in range(2, N - 2):
        for p in [(i, 5), (i, 10), (5, i), (10, i)]:
            img.putpixel(p, (110, 80, 46, 255))
    return img


def table_side(front):
    img = planks(71 + front, (150, 112, 66))
    rng = random.Random(72 + front)
    for y in range(0, 4):  # top band
        for x in range(N):
            img.putpixel((x, y), (104, 74, 42, 255))
    # Tools hanging on the side (saw / hammer shapes, original pixels)
    for x in range(3, 7):
        img.putpixel((x, 6), (170, 170, 176, 255))
    for y in range(6, 11):
        img.putpixel((5, y), (96, 70, 40, 255))
    if front:
        for y in range(6, 12):
            img.putpixel((11, y), (96, 70, 40, 255))
        for x in range(9, 14):
            img.putpixel((x, 6), (150, 150, 156, 255))
    return img


def furnace_side():
    return noise_tex(73, (112, 112, 112), 10, specks=[(90, 90, 90), (130, 130, 130)], speck_chance=0.15)


def furnace_front(lit):
    img = furnace_side()
    for y in range(8, 14):  # fire opening
        for x in range(4, 12):
            c = (240, 150, 40) if lit and (x + y) % 3 else ((60, 50, 40) if lit else (30, 30, 30))
            img.putpixel((x, y), c + (255,))
    for x in range(3, 13):
        img.putpixel((x, 7), (70, 70, 70, 255))
        img.putpixel((x, 14), (70, 70, 70, 255))
    for x in range(5, 11):  # vent slits at top
        img.putpixel((x, 3), (40, 40, 40, 255))
    return img


TEXTURES.update({
    "crafting_table_top": table_top,
    "crafting_table_side": lambda: table_side(0),
    "crafting_table_front": lambda: table_side(1),
    "furnace_side": furnace_side,
    "furnace_top": lambda: noise_tex(74, (120, 120, 120), 8),
    "furnace_front": lambda: furnace_front(False),
    "furnace_front_on": lambda: furnace_front(True),
})



# ---------------------------------------------------------------------------
# Phase 5: block-breaking cracks (10 stages, transparent background)
# ---------------------------------------------------------------------------

def destroy_stage(stage):
    """Cracks that spread from the centre as `stage` goes 0 → 9."""
    rng = random.Random(90)
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    # Pre-generate crack paths; draw more of them (and longer) at higher stages.
    paths = []
    for k in range(10):
        x, y = 7.5 + rng.uniform(-1, 1), 7.5 + rng.uniform(-1, 1)
        ang = rng.uniform(0, 6.283)
        pts = []
        for _ in range(12):
            import math
            ang += rng.uniform(-0.7, 0.7)
            x += math.cos(ang)
            y += math.sin(ang)
            pts.append((int(x) % N, int(y) % N))
        paths.append(pts)
    for k in range(min(10, 2 + stage)):
        for (px, py) in paths[k][: 3 + stage]:
            img.putpixel((px, py), (20, 20, 20, 230))
    return img


TEXTURES["white_wool"] = lambda: noise_tex(95, (232, 232, 228), 12)


def torch():
    """A thin wooden stick with a glowing amber head, centred in the tile
    (drawn as crossed panes, like plants)."""
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    for y in range(6, N):  # stick, 2 px wide
        shade = 0 if y % 3 else -12
        img.putpixel((7, y), (clamp(122 + shade), clamp(86 + shade), clamp(48 + shade), 255))
        img.putpixel((8, y), (clamp(98 + shade), clamp(68 + shade), clamp(38 + shade), 255))
    # Head: white-hot core with amber and orange around it.
    for (x, y, c) in [
        (7, 5, (255, 250, 220)), (8, 5, (255, 236, 170)),
        (7, 4, (255, 214, 110)), (8, 4, (255, 190, 80)),
        (7, 3, (250, 160, 50)), (8, 3, (255, 200, 90)),
        (6, 5, (240, 150, 40)), (9, 5, (240, 150, 40)),
        (7, 2, (230, 120, 30)),
    ]:
        img.putpixel((x, y), c + (255,))
    return img


TEXTURES["torch"] = torch

for _s in range(10):
    TEXTURES[f"destroy_stage_{_s}"] = (lambda st=_s: lambda: destroy_stage(st))()

if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for name, fn in TEXTURES.items():
        fn().save(os.path.join(OUT, name + ".png"))
    print(f"wrote {len(TEXTURES)} textures (incl. cracks) to {os.path.normpath(OUT)}")
