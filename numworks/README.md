# Loredrop — NumWorks edition

A 3D version of Loredrop that runs on a NumWorks graphing calculator
(models N0110, N0115, N0120). It isn't the PC game shrunk down: it's a
separate engine written for a machine with no graphics chip and a few
hundred KB of memory. It shares the PC game's blocks, textures, mining
numbers and physics.

> Status: **NW phase 3** (creatures, combat, armour, dropped items, ruins
> with lore). Built and tested on PC through a test harness. It has **not
> run on a real calculator yet**: the director's test on a real calculator
> is the first real check.

## What's in it

- An endless 3D world in first person, drawn by ray casting (one ray per
  pixel; no GPU needed), with the PC game's 16x16 textures.
- World generation: continents and oceans, hills, mountains with snow
  caps, 7 biomes (ocean, beach, plains, forest, desert, snowy plains,
  mountains), worm caves and caverns, lava at the bottom, ore veins
  (coal, iron, copper, gold, lapis, diamond, with deepslate variants),
  granite/diorite/andesite pockets, oak, birch and spruce trees, cacti.
- Minecraft physics at 20 ticks per second (walking 4.3 m/s, sprinting,
  jumping 1.25 blocks, swimming, sneaking, optional auto-jump).
- Survival mode (NW2), with the PC game's (Minecraft's) rules and numbers:
  - 10 hearts and 10 food; hunger from sprinting, jumping, swimming and
    mining; saturation; fast healing when well fed; starving (down to half
    a heart); fall damage; drowning after 15 s; lava and burning; death
    screen and respawn (items are lost).
  - Mining speed depends on the tool: 5 tiers (wooden, stone, iron, golden,
    diamond) of pickaxe, axe, shovel, hoe and sword, with durability; ores
    need the right tier to drop anything; drops include coal, raw iron,
    raw copper, raw gold, diamonds, lapis, emeralds, sticks and apples.
  - 36-slot inventory (9 in the hotbar) with stack counts and tool wear bars.
  - Crafting in a 2x2 grid (inventory) or 3x3 (crafting table), shaped and
    shapeless recipes, plus a recipe book that lists what you can make now
    and crafts it in one key press.
  - Furnaces (up to 8): smelt ores, sand, cobblestone, logs, meat, with
    fuel times from the PC game (coal = 8 items); lit furnaces glow.
  - Eating (hold EXE, 1.6 s).
- Creatures (NW3), with the PC game's box models, textures and numbers:
  cows, pigs, sheep and chickens (wander, flee when hurt, drop meat,
  leather, wool, feathers); zombies and skeletons (burn in daylight;
  skeletons keep their distance and shoot arrows); spiders (leap, calm in
  daylight unless hurt); and **the Hollow** — wanders in the dark moving
  blocks around, turns on you if you hit it or break a block it placed,
  and sometimes drops a lore fragment. Animals live on grass in their
  biomes; monsters appear in the dark (night or caves).
- Combat: hit with OK (reach 3), Minecraft 1.9+ attack cooldown, weapon
  damage, knock-back; creatures hurt you back. Armour (leather, iron,
  golden, diamond) in 4 slots, with Minecraft's damage reduction and wear.
- Dropped items lie on the ground (spinning), are picked up by walking
  over them and vanish after 5 minutes; dying drops your inventory where
  you died; Backspace throws one item.
- Ruins of the Builders with lore tablets (EXE to read): broken
  stone-brick ruins on the surface, half-buried desert halls, sealed deep
  vaults in the deepslate. 12 fragments of the story; a lore journal in
  the pause menu.
- Creative mode: every item from a paged palette, instant breaking, flying,
  no damage.
- Day and night (20-minute day), square sun and moon, stars, sunrise
  glow, dark caves and shadows under overhangs, water you can see into,
  distance fog.
- Title screen, pause menu (with position, biome, seed), settings
  (view distance, pixel size, field of view, turn speed, auto-jump, FPS
  display, brightness, each with a description), controls screen.

## Controls

| Key | Action |
|---|---|
| Arrows | Look around |
| 8 / 5 / 4 / 6 | Walk forward / back / left / right |
| 0 | Jump, swim up, fly up |
| 1 | Sneak, fly down |
| 3 | Sprint (until you stop walking) |
| OK | Hit the creature you look at; hold to mine a block |
| EXE | Place the held block; use a crafting table, furnace or lore tablet; read a lore fragment; hold to eat |
| Backspace | Throw one of the held item |
| 7 / 9 | Previous / next hotbar slot |
| Var | Inventory (creative: item palette) |
| Toolbox | Creative: fly on / off |
| Back | Pause menu |
| Home | Quit |

In inventory screens: arrows move the cursor; **OK** picks up / puts down /
swaps a stack; **EXE** takes half or puts down one; on the crafting result,
OK crafts once and EXE crafts as many as possible; **Toolbox** opens the
recipe book (OK = craft one, EXE = craft all); **( )** turns the creative
palette's pages; **Back** or **Var** closes.

## Installing on a calculator

1. Get `loredrop.nwa` (in this folder, or build it, see below).
2. Make sure the calculator runs an up-to-date official NumWorks software
   (version 16 or later supports apps like this).
3. On a computer, in Chrome or Edge, open <https://my.numworks.com/apps>,
   plug in the calculator, and send `loredrop.nwa` with NumWorks' installer.
4. On the calculator, Loredrop appears at the end of the Home screen.

**Warning:** NumWorks' installer replaces the apps already installed this
way (games from other sites included). Exam mode hides and usually removes
these apps. The calculator also gives apps no storage, so **the world is not
saved when you quit**.

## Building

```sh
rustup target add thumbv7em-none-eabihf
./build_nwa.sh            # writes loredrop.nwa (and checks it with nwlink if Node.js is installed)
cd app && cargo run --release   # builds and installs over USB (needs Node.js)
```

## Testing on PC

The game code (`core/`) is plain Rust with no calculator dependency, so it
is tested on PC:

```sh
cargo test                                   # unit tests (world, physics, rendering...)
cargo run --release -p ld_nw_desktop -- shot --seed 1 --time 6000 --look 200 5 --out shot.png
cargo run --release -p ld_nw_desktop -- shot --seed 1 --script "8:40,Ok:30" --out walk.png
cargo run --release -p ld_nw_desktop -- bench --seed 1
cargo run --release -p ld_nw_desktop -- map --seed 1 --out map.png
```

See the header of `desktop/src/main.rs` for every option.

## Layout

```text
numworks/
  core/        the game (no_std, no heap): world, worldgen, render, player,
               items, inventory (+ crafting), furnace, survival, ui, hud, game,
               mobs, entities (creatures/items/arrows), models (drawing them),
               structures (ruins)
  desktop/     PC test harness (fake screen and keyboard, PNG screenshots, benchmark)
  app/         the calculator app: EADK bindings, icon, .cargo config for ARM
  tools/       Python generators: blocks+textures from the PC game's data, icon, PC font
  build_nwa.sh builds app/ and writes loredrop.nwa
```

`core/src/gen_blocks.rs`, `gen_items.rs` and `gen_mobs.rs` are generated
from the PC game's `assets/data/blocks.toml`, `items.toml`, `recipes.toml`,
`mobs.toml`, `lore.toml` and textures by `tools/gen_blocks.py`; re-run it
when the PC game's data changes. (Plants and torches are left out for now.)

## Limits (by design, for the calculator)

- The world is 48 blocks tall (sea at 20); the PC game uses 384.
- Only 32 x 32 columns are in memory around the player (48 KB); the rest
  is regenerated from the seed when you come back. The player's changes are
  remembered (up to 1024 changed blocks; beyond that, changes far away are
  forgotten).
- View distance 6 to 14 blocks.
- No saving (calculator apps have no storage access).
- At most 16 creatures, 24 dropped items and 6 arrows at once, all within
  the loaded area (creatures that leave it disappear). Creatures steer
  straight at their goal (no full path-finding like on PC).
- Hunger can't kill you (Normal difficulty stops at half a heart).
