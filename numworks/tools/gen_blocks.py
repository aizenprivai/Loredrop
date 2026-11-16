#!/usr/bin/env python3
"""Generate the NumWorks edition's game data from the PC game's data files.

Reads the PC game's `assets/data/blocks.toml`, `items.toml`, `recipes.toml`
and the block and item textures, and writes two plain Rust files:

* `numworks/core/src/gen_blocks.rs`: one constant per block, the block
  property table (`BLOCKS`), every texture as 16x16 RGB565 (`TEXTURES`, biome
  tint baked in, see-through texels set to the `TRANSPARENT` key) and the
  mining crack overlays (`CRACKS`).
* `numworks/core/src/gen_items.rs`: tool tiers, the item table (`ITEMS`,
  blocks first so a block's item id equals its block id), one constant per
  non-block item, crafting recipes, smelting recipes, fuels and what each
  block drops.

The generated files are committed, so building the calculator app needs no
Python. Re-run this script whenever the PC game's data changes:

    python3 numworks/tools/gen_blocks.py

Left out on purpose (not in the calculator edition yet): cross-shaped
blocks (plants, torches), armour, and recipes that use anything left out.
"""

import pathlib
import tomllib

from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parents[2]
DATA = ROOT / "assets" / "data"
BLOCK_TEX = ROOT / "assets" / "textures" / "block"
ITEM_TEX = ROOT / "assets" / "textures" / "item"
ENTITY_TEX = ROOT / "assets" / "textures" / "entity"
OUT_MOBS = ROOT / "numworks" / "core" / "src" / "gen_mobs.rs"

# PC biome names → calculator biomes (the calculator has fewer biomes).
BIOME_MAP = {
    "plains": "Plains",
    "forest": "Forest",
    "birch_forest": "Forest",
    "taiga": "Snowy",
    "snowy_plains": "Snowy",
    "mountains": "Mountains",
    "desert": "Desert",
    "beach": "Beach",
}
OUT_BLOCKS = ROOT / "numworks" / "core" / "src" / "gen_blocks.rs"
OUT_ITEMS = ROOT / "numworks" / "core" / "src" / "gen_items.rs"

# Biome tints baked into the textures (the calculator has a single tint per
# block). Values are the PC game's plains colours from biomes.toml.
TINTS = {"grass": (0x91, 0xBD, 0x59), "foliage": (0x77, 0xAB, 0x2F), "water": (0x3F, 0x76, 0xE4)}

# Colour used to mark see-through texels. Must match `TRANSPARENT` in blocks.rs.
TRANSPARENT = 0xF81F

TIER_ORDER = ["wooden", "stone", "iron", "golden", "diamond"]


def rgb565(r, g, b):
    """Pack an 8-bit RGB colour into 16-bit RGB565, the calculator's screen format."""
    return ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)


def face_value(table, face):
    """Resolve a per-face table such as `textures` or `tint` for one face.

    `face` is "top", "bottom" or "side"; more specific keys win, like on PC.
    For sides, the PC game's `north` face (the "front" of crafting tables and
    furnaces) is preferred so the calculator shows the recognisable front.
    """
    if table is None:
        return None
    if isinstance(table, str):
        return table
    if face == "side":
        for key in ("north", "side", "all"):
            if key in table:
                return table[key]
        return None
    return table.get(face, table.get("all"))


def load_texture(path, tint):
    """Load one 16x16 texture as a list of 256 RGB565 values."""
    img = Image.open(path).convert("RGBA")
    if img.size != (16, 16):
        img = img.resize((16, 16), Image.NEAREST)
    out = []
    for y in range(16):
        for x in range(16):
            r, g, b, a = img.getpixel((x, y))
            if a < 64:
                out.append(TRANSPARENT)
                continue
            if tint:
                tr, tg, tb = TINTS[tint]
                r, g, b = r * tr // 255, g * tg // 255, b * tb // 255
            c = rgb565(r, g, b)
            # Never let a real pixel collide with the transparency key.
            out.append(c ^ 0x0020 if c == TRANSPARENT else c)
    return out


def parse_drops(drops, name):
    """Parse a PC drop string into (item name, min, max, chance) tuples.

    Formats: missing = the block itself; "none"; "item"; "item:2-5" (random
    count); "item@0.02" (2 % chance); several separated by commas.
    """
    if drops is None:
        return [(name, 1, 1, 1.0)]
    if drops == "none":
        return []
    out = []
    for part in drops.split(","):
        part = part.strip()
        chance = 1.0
        if "@" in part:
            part, c = part.split("@")
            chance = float(c)
        lo = hi = 1
        if ":" in part:
            part, rng = part.split(":")
            if "-" in rng:
                lo, hi = (int(v) for v in rng.split("-"))
            else:
                lo = hi = int(rng)
        out.append((part, lo, hi, chance))
    return out


def rust_name(display):
    return display.replace('"', "'")


def main():
    blocks_toml = tomllib.loads((DATA / "blocks.toml").read_text())
    items_toml = tomllib.loads((DATA / "items.toml").read_text())
    recipes_toml = tomllib.loads((DATA / "recipes.toml").read_text())

    blocks = [b for b in blocks_toml["block"] if b.get("shape", "cube") == "cube" and b.get("layer") != "invisible"]
    items = list(items_toml["item"])
    mobs_toml = tomllib.loads((DATA / "mobs.toml").read_text())
    lore_toml = tomllib.loads((DATA / "lore.toml").read_text())

    textures = []  # list of (label, pixels)
    tex_index = {}

    def tex(directory, name, tint=None):
        key = (str(directory), name, tint)
        if key not in tex_index:
            tex_index[key] = len(textures)
            textures.append((f"{name}{' (' + tint + ' tint)' if tint else ''}", load_texture(directory / f"{name}.png", tint)))
        return tex_index[key]

    # ---------------------------------------------------------------- ids
    ids = {}
    for i, b in enumerate(blocks):
        ids[b["name"]] = i + 1
    block_count = len(blocks) + 1
    for i, it in enumerate(items):
        ids[it["name"]] = block_count + i
    item_count = block_count + len(items)

    # ------------------------------------------------------------- blocks
    block_rows = []
    for b in blocks:
        faces = [tex(BLOCK_TEX, face_value(b.get("textures"), f), face_value(b.get("tint"), f)) for f in ("top", "side", "bottom")]
        block_rows.append((b, faces))

    lines = [
        "//! GENERATED by `numworks/tools/gen_blocks.py` from the PC game's",
        "//! `assets/data/blocks.toml` and textures. Do not edit by hand.",
        "",
        "use crate::blocks::{Block, BlockDef, Layer, Tool};",
        "",
        "/// Empty space.",
        "pub const AIR: Block = 0;",
    ]
    for b, _ in block_rows:
        lines.append(f"/// {b.get('display', b['name'])}.")
        lines.append(f"pub const {b['name'].upper()}: Block = {ids[b['name']]};")
    lines += ["", "/// Number of block types, air included.", f"pub const BLOCK_COUNT: usize = {block_count};", ""]
    lines.append("/// Properties of every block, indexed by block id.")
    lines.append("pub static BLOCKS: [BlockDef; BLOCK_COUNT] = [")
    lines.append(
        '    BlockDef { name: "Air", layer: Layer::Air, solid: false, tex: [0, 0, 0], hardness: 0.0, '
        "tool: Tool::None, harvest_level: -1, light: 0 },"
    )
    for b, faces in block_rows:
        layer = {"opaque": "Opaque", "cutout": "Cutout", "translucent": "Water"}[b.get("layer", "opaque")]
        if b["name"] == "lava":
            layer = "Lava"
        tool = (b.get("tool") or "none").capitalize()
        lines.append(
            f'    BlockDef {{ name: "{rust_name(b.get("display", b["name"]))}", layer: Layer::{layer}, '
            f"solid: {str(b.get('solid', True)).lower()}, tex: [{faces[0]}, {faces[1]}, {faces[2]}], "
            f"hardness: {float(b.get('hardness', 0.0))!r}, tool: Tool::{tool}, "
            f"harvest_level: {b.get('harvest_level', -1)}, light: {int(b.get('light', 0))} }},"
        )
    lines.append("];")

    # Item textures (added to the same texture table).
    item_icons = {}
    for it in items:
        item_icons[it["name"]] = tex(ITEM_TEX, it.get("texture", it["name"]))

    # Creature part textures (also in the same table).
    for m in mobs_toml["mob"]:
        for part in m["parts"]:
            tex(ENTITY_TEX, part["tex"])
            if "front" in part:
                tex(ENTITY_TEX, part["front"])

    lines += ["", "/// Number of textures.", f"pub const TEXTURE_COUNT: usize = {len(textures)};", ""]
    lines.append("/// 16x16 RGB565 textures (blocks, then items), row by row. See `blocks::TRANSPARENT`.")
    lines.append("pub static TEXTURES: [[u16; 256]; TEXTURE_COUNT] = [")
    for label, px in textures:
        lines.append(f"    // {label}")
        lines.append("    [")
        for row in range(16):
            lines.append("        " + ", ".join(f"0x{v:04X}" for v in px[row * 16:(row + 1) * 16]) + ",")
        lines.append("    ],")
    lines.append("];")
    lines.append("")
    # Mining cracks: the PC game's destroy_stage_0..9 overlays as bit masks
    # (bit x of row y set = darken that texel).
    lines.append("/// Mining crack overlays (10 stages), one 16-bit mask per texture row.")
    lines.append("pub static CRACKS: [[u16; 16]; 10] = [")
    for stage in range(10):
        img = Image.open(BLOCK_TEX / f"destroy_stage_{stage}.png").convert("RGBA").resize((16, 16), Image.NEAREST)
        masks = []
        for y in range(16):
            mask = 0
            for x in range(16):
                if img.getpixel((x, y))[3] >= 64:
                    mask |= 1 << x
            masks.append(f"0x{mask:04X}")
        lines.append(f"    [{', '.join(masks)}],")
    lines.append("];")
    OUT_BLOCKS.write_text("\n".join(lines) + "\n")

    # -------------------------------------------------------------- items
    tiers = items_toml["tiers"]
    L = [
        "//! GENERATED by `numworks/tools/gen_blocks.py` from the PC game's",
        "//! `assets/data/items.toml`, `recipes.toml` and `blocks.toml`. Do not edit by hand.",
        "",
        "use crate::blocks::{BLOCK_COUNT, Tool};",
        "use crate::items::{Drop, Item, ItemDef, ItemKind, Shaped, Shapeless, TierDef};",
        "",
        "/// Tool tiers (wooden, stone, iron, golden, diamond), Minecraft numbers.",
        "pub static TIERS: [TierDef; 5] = [",
    ]
    for t in TIER_ORDER:
        d = tiers[t]
        L.append(f'    TierDef {{ name: "{t}", level: {d["level"]}, speed: {float(d["speed"])!r}, durability: {d["durability"]} }},')
    L.append("];")
    L.append("")
    for it in items:
        L.append(f"/// {it.get('display', it['name'])}.")
        L.append(f"pub const {it['name'].upper()}: Item = {ids[it['name']]};")
    L += ["", "/// Number of item types (blocks included; 0 = nothing).", f"pub const ITEM_COUNT: usize = {item_count};", ""]
    L.append("/// Every item, indexed by item id. Ids below `BLOCK_COUNT` are the blocks.")
    L.append("pub static ITEMS: [ItemDef; ITEM_COUNT] = [")
    L.append('    ItemDef { name: "Nothing", max_stack: 0, icon: 0, kind: ItemKind::Block, creative: false },')
    for b, faces in block_rows:
        creative = b.get("item", True) and b["name"] not in ("water", "lava")
        L.append(
            f'    ItemDef {{ name: "{rust_name(b.get("display", b["name"]))}", max_stack: 64, icon: {faces[1]}, '
            f"kind: ItemKind::Block, creative: {str(creative).lower()} }},"
        )
    for it in items:
        if "tool" in it:
            kind = f"ItemKind::Tool {{ tool: Tool::{it['tool'].capitalize()}, tier: {TIER_ORDER.index(it['tier'])}, damage: {float(it.get('attack_damage', 1.0))!r}, speed: {float(it.get('attack_speed', 4.0))!r} }}"
            stack = 1
        elif "armor" in it:
            slot = ["helmet", "chestplate", "leggings", "boots"].index(it["armor"])
            kind = (
                f"ItemKind::Armor {{ slot: {slot}, defense: {it['defense']}, "
                f"toughness: {float(it.get('toughness', 0.0))!r}, durability: {it['durability']} }}"
            )
            stack = 1
        elif "food" in it:
            kind = f"ItemKind::Food {{ food: {it['food']}, saturation: {float(it['saturation'])!r} }}"
            stack = it.get("max_stack", 64)
        else:
            kind = "ItemKind::Material"
            stack = it.get("max_stack", 64)
        L.append(
            f'    ItemDef {{ name: "{rust_name(it.get("display", it["name"]))}", max_stack: {stack}, '
            f"icon: {item_icons[it['name']]}, kind: {kind}, creative: true }},"
        )
    L.append("];")
    L.append("")
    L.append("const _: () = assert!(ITEM_COUNT > BLOCK_COUNT);")
    L.append("")

    # Tags and recipes. An ingredient is 0 (empty), an item id, or
    # TAG_FLAG | tag index.
    tag_names = list(recipes_toml.get("tags", {}).keys())
    tag_members = {}
    for t in tag_names:
        members = [ids[m] for m in recipes_toml["tags"][t] if m in ids]
        tag_members[t] = members

    def ingredient(s):
        if s.startswith("#"):
            t = s[1:]
            return None if not tag_members.get(t) else 0x8000 | tag_names.index(t)
        return ids.get(s)

    L.append("/// Item groups used by recipes (`#planks`...).")
    L.append(f"pub static TAGS: [&[Item]; {len(tag_names)}] = [")
    for t in tag_names:
        L.append(f"    &[{', '.join(str(m) for m in tag_members[t])}], // {t}")
    L.append("];")
    L.append("")

    shaped = []
    for r in recipes_toml.get("shaped", []):
        if r["result"] not in ids:
            continue
        pattern = r["pattern"]
        h = len(pattern)
        w = max(len(row) for row in pattern)
        cells = []
        ok = True
        for row in pattern:
            for x in range(w):
                ch = row[x] if x < len(row) else " "
                if ch == " ":
                    cells.append(0)
                else:
                    code = ingredient(r["key"][ch])
                    if code is None:
                        ok = False
                    cells.append(code or 0)
        if ok:
            cells += [0] * (9 - len(cells))
            shaped.append((w, h, cells, ids[r["result"]], r.get("count", 1), r["result"]))
    L.append("/// Shaped crafting recipes (patterns also match mirrored, as on PC).")
    L.append(f"pub static SHAPED: [Shaped; {len(shaped)}] = [")
    for w, h, cells, res, count, rname in shaped:
        L.append(f"    Shaped {{ w: {w}, h: {h}, cells: [{', '.join(str(c) for c in cells)}], result: {res}, count: {count} }}, // {rname}")
    L.append("];")
    L.append("")

    shapeless = []
    for r in recipes_toml.get("shapeless", []):
        codes = [ingredient(i) for i in r["ingredients"]]
        if r["result"] in ids and all(c is not None for c in codes):
            shapeless.append((codes, ids[r["result"]], r.get("count", 1), r["result"]))
    L.append("/// Shapeless crafting recipes (ingredients in any place).")
    L.append(f"pub static SHAPELESS: [Shapeless; {len(shapeless)}] = [")
    for codes, res, count, rname in shapeless:
        L.append(f"    Shapeless {{ ingredients: &[{', '.join(str(c) for c in codes)}], result: {res}, count: {count} }}, // {rname}")
    L.append("];")
    L.append("")

    smelting = []
    for r in recipes_toml.get("smelting", []):
        code = ingredient(r["input"])
        if code is not None and r["result"] in ids:
            smelting.append((code, ids[r["result"]], r.get("time", 200), r["input"], r["result"]))
    L.append("/// Furnace recipes: (ingredient, result, ticks).")
    L.append(f"pub static SMELTING: [(u16, Item, u16); {len(smelting)}] = [")
    for code, res, t, a, b in smelting:
        L.append(f"    ({code}, {res}, {t}), // {a} -> {b}")
    L.append("];")
    L.append("")

    fuels = [(ids[n], t, n) for n, t in recipes_toml.get("fuels", {}).items() if n in ids]
    L.append("/// Furnace fuels: (item, burn ticks).")
    L.append(f"pub static FUELS: [(Item, u16); {len(fuels)}] = [")
    for i, t, n in fuels:
        L.append(f"    ({i}, {t}), // {n}")
    L.append("];")
    L.append("")

    L.append("/// What each block drops when harvested, indexed by block id.")
    L.append("pub static DROPS: [&[Drop]; BLOCK_COUNT] = [")
    L.append("    &[], // air")
    for b, _ in block_rows:
        parts = []
        for name, lo, hi, chance in parse_drops(b.get("drops"), b["name"]):
            if name in ids:
                parts.append(f"Drop {{ item: {ids[name]}, min: {lo}, max: {hi}, chance: {float(chance)!r} }}")
        L.append(f"    &[{', '.join(parts)}], // {b['name']}")
    L.append("];")
    OUT_ITEMS.write_text("\n".join(L) + "\n")

    # --------------------------------------------------------------- mobs
    import math

    M = [
        "//! GENERATED by `numworks/tools/gen_blocks.py` from the PC game's",
        "//! `assets/data/mobs.toml` and `lore.toml`. Do not edit by hand.",
        "",
        "use crate::items::Drop;",
        "use crate::mobs::{Category, Era, MobDef, Part, Role};",
        "use crate::worldgen::Biome;",
        "",
    ]
    mobs = mobs_toml["mob"]
    for i, m in enumerate(mobs):
        M.append(f"/// {m.get('display', m['name'])}.")
        M.append(f"pub const {m['name'].upper()}: u8 = {i};")
    M += ["", "/// Number of creature kinds.", f"pub const MOB_COUNT: usize = {len(mobs)};", ""]
    M.append("/// Every creature kind (Minecraft numbers from the PC game).")
    M.append("pub static MOBS: [MobDef; MOB_COUNT] = [")
    for m in mobs:
        drops = []
        for d in m.get("drops", []):
            for name, lo, hi, chance in parse_drops(d, m["name"]):
                if name in ids:
                    drops.append(f"Drop {{ item: {ids[name]}, min: {lo}, max: {hi}, chance: {float(chance)!r} }}")
        carries = [str(ids[c]) for c in m.get("carries", []) if c in ids and ids[c] < block_count]
        spawn = m.get("spawn", {})
        biomes = []
        for b in spawn.get("surface", []):
            rb = BIOME_MAP.get(b)
            if rb and f"Biome::{rb}" not in biomes:
                biomes.append(f"Biome::{rb}")
        group = spawn.get("group", [1, 1])
        parts = []
        for p in m["parts"]:
            x, y, z, w, h, d = (float(v) / 16.0 for v in p["box"])
            pivot = [float(v) / 16.0 for v in p.get("pivot", [0, 0, 0])]
            rot = [math.radians(float(v)) for v in p.get("rot", [0, 0, 0])]
            tex_i = tex(ENTITY_TEX, p["tex"])
            front = tex(ENTITY_TEX, p["front"]) if "front" in p else tex_i
            role = p.get("role", "none").capitalize()
            parts.append(
                f"Part {{ min: [{x!r}, {y!r}, {z!r}], size: [{w!r}, {h!r}, {d!r}], "
                f"pivot: [{pivot[0]!r}, {pivot[1]!r}, {pivot[2]!r}], rot: [{rot[0]!r}, {rot[1]!r}, {rot[2]!r}], "
                f"role: Role::{role}, phase: {p.get('phase', 0)}, tex: {tex_i}, front: {front} }}"
            )
        M.append(
            f'    MobDef {{ name: "{rust_name(m.get("display", m["name"]))}", '
            f"category: Category::{m['category'].capitalize()}, health: {float(m['health'])!r}, "
            f"speed: {float(m['speed'])!r}, width: {float(m['width'])!r}, height: {float(m['height'])!r}, "
            f"attack: {float(m.get('attack_damage', 0))!r}, ranged: {str(m.get('ranged', False)).lower()}, "
            f"burns_in_daylight: {str(m.get('burns_in_daylight', False)).lower()}, "
            f"calm_in_light: {str(m.get('calm_in_light', False)).lower()}, "
            f"angry_speed: {float(m.get('angry_speed', 1.0))!r}, "
            f"drops: &[{', '.join(drops)}], carries: &[{', '.join(carries)}], "
            f"surface: &[{', '.join(biomes)}], dark: {str(spawn.get('dark', False)).lower()}, "
            f"group: ({group[0]}, {group[1]}), weight: {spawn.get('weight', 1)}, "
            f"parts: &[\n        {(',' + chr(10) + '        ').join(parts)},\n    ] }},"
        )
    M.append("];")
    M.append("")
    names = lore_toml.get("names", {})
    frags = lore_toml["fragment"]
    M.append("/// Lore fragments (the Builders' story), with their era.")
    M.append(f"pub static LORE: [(Era, &str); {len(frags)}] = [")
    for f in frags:
        text = f["text"]
        for k, v in names.items():
            text = text.replace("{" + k + "}", v)
        # The calculator fonts are safest with plain ASCII punctuation.
        text = text.replace("—", "-").replace("’", "'").replace("“", '"').replace("”", '"')
        text = text.replace("\\", "\\\\").replace('"', '\\"')
        M.append(f'    (Era::{f["era"].capitalize()}, "{text}"),')
    M.append("];")
    OUT_MOBS.write_text("\n".join(M) + "\n")

    print(
        f"wrote {OUT_BLOCKS.relative_to(ROOT)} and {OUT_ITEMS.relative_to(ROOT)}: "
        f"{len(blocks)} blocks, {len(items)} items, {len(textures)} textures, "
        f"{len(shaped)} shaped + {len(shapeless)} shapeless recipes, {len(smelting)} smelting, {len(fuels)} fuels"
    )


if __name__ == "__main__":
    main()
