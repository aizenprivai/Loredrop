# Changelog

## [Unreleased]
### 2026-11-14 — Phase 0
- Completed design interview (15 questions).
- Created GDD.md (draft v0.1), ROADMAP.md and project tracking documents.
- Chose project name: **Loredrop**.
- GDD approved by director (v1.0). Phase 0 complete.
- Director requirement: Vulkan as the graphics API for maximum performance. Phase 1 started.
- Rendering decision: wgpu with Vulkan as primary backend (DX12/Metal/OpenGL fallbacks).
- Wrote ARCHITECTURE.md draft v0.1.
- Scaffolded Cargo workspace: loredrop, ld_core, ld_config, ld_input, ld_world,
  ld_physics, ld_mesh, ld_render. Coordinate types implemented with 3 unit tests.
  Builds clean (no clippy warnings); wgpu 30 + winit 0.30 verified compatible.
- Architecture approved by director. Phase 1 complete.

### 2026-11-14 — Phase 2: core engine foundation
- **Window & game loop** (`loredrop`): winit window, fixed 20 ticks/s simulation
  with render interpolation, max-10-ticks catch-up guard, FPS limiter, VSync,
  borderless fullscreen (F11), focus-loss key release.
- **Renderer** (`ld_render`): wgpu with Vulkan first, then DX12/Metal, then
  OpenGL; a forced backend that fails falls back to auto. Reverse-Z infinite
  depth, camera-relative rendering (f64 → f32 offsets), back-face culling,
  cylindrical distance fog, egui UI pass. Shader gamma fallback for drivers
  without sRGB view support (found on OpenGL). F2 screenshots, with an
  offscreen re-render fallback where the frame can't be read back (OpenGL).
- **Input** (`ld_input`): action-based input, rebindable keys (defaults match
  Minecraft Java), press queue so short taps between ticks are never lost,
  OS key-repeat filtering.
- **Settings** (`ld_config`): single-definition schema; every setting has a
  label, description, performance note, range and apply-mode; commented
  `settings.toml` generated from it; invalid values clamp/fall back with
  warnings; unreadable file kept as `.broken`; atomic saves.
- **Physics** (`ld_physics`): AABB collision with per-axis sweep (Y, then
  larger horizontal axis), Minecraft-faithful player movement: walk 4.32 b/s,
  sprint 5.61 b/s, jump 1.25 blocks, sprint-jump boost, sneak, creative flight
  (10.9 b/s), landing ends flight, double-tap jump toggles flight.
- **Camera**: Minecraft yaw/pitch convention, 0.15°/count mouse at 100 %,
  invert option, pitch clamp, eased sneak eye height.
- **Debug overlay** (F3): FPS / avg / worst frame time, TPS, position, block &
  chunk coords, facing, speed, movement state, GPU/backend/driver, resolution,
  VSync, draw calls, triangles. Crosshair and mouse-capture hint.
- **Test scene**: 96×96 platform, staircase, 2-high wall, tunnel, pillar, pit,
  pyramid; hidden-face culled mesh (41 k triangles).
- 29 automated tests across 6 crates; clippy clean.
- Phase 2 approved by director (real-hardware test deferred: director away from computer). Phase 3 started.

### 2026-11-14 — Phase 3: voxel engine & chunk system
- **Block registry** (`ld_world::block`): block types are data, loaded from
  `assets/data/blocks.toml` (name, display name, render layer, collision,
  cull-same, per-face textures with all/side/top/bottom/… keys, per-face tint).
  Typos and duplicates are rejected with clear errors. 11 starter blocks.
- **Chunk storage**: 16³ sections with palette compression (uniform sections
  use 0 bytes; ≤16 block types ≈ 2 KB vs 8 KB raw), automatic bit-width growth,
  `compact()` after generation; 24-section columns covering Y −64..320;
  `ChunkMap` with `Arc` copy-on-write columns for lock-free worker access.
- **Streaming pipeline** (`loredrop::world`): render-distance + 1 ring loaded,
  nearest-first generation and meshing on a rayon worker pool, capped jobs in
  flight, per-section versions so stale meshes are discarded, throttled GPU
  uploads, unloading of far columns and their meshes. Block edits mark up to 8
  neighbouring sections for rebuild (tested; player editing comes in Phase 5).
- **Mesher** (`ld_mesh`): 18³ neighbourhood copy, hidden-face culling across
  section/column borders, greedy meshing, 4-level ambient occlusion with
  anisotropy-fixing quad flip, separate opaque/cutout/translucent lists,
  8-byte packed vertices, world-bottom culling.
- **Chunk renderer** (`ld_render::chunks`): texture array with CPU-generated
  alpha-aware mipmaps, nearest/mip-linear sampling, repeat-tiling UVs derived
  in the shader, per-face Minecraft-style shading + AO, grass/foliage tints,
  opaque and cutout (alpha-test) passes, shared quad index buffer, per-section
  camera-relative origins, CPU frustum culling, front-to-back sorting.
- **Assets** (`ld_assets`): assets-folder discovery, block-file loading,
  texture loading by Minecraft-compatible names, mixed-size scaling, first-frame
  for animation strips, magenta/black fallback for missing textures.
- **Placeholder textures**: 13 original procedurally generated 16×16 textures
  (`tools/gen_placeholder_textures.py`).
- **Test terrain** (`ld_worldgen::TestTerrain`): deterministic rolling hills,
  bedrock floor, sand lowlands, simple oak trees (temporary, for Phase 3 only).
- **Settings**: `graphics.render_distance`, `graphics.mipmap_levels`,
  `performance.worker_threads`, `performance.chunk_uploads_per_frame`.
- **Debug overlay**: chunks ready/wanted, job queues, sections drawn/culled,
  world & mesh memory, block underfoot.
- **Tools**: `loredrop --bench-world <distance>` headless generation+meshing
  benchmark; criterion benches for sections and meshing.
- Removed the Phase 2 hand-built test scene; game tests now use a test-course
  generator in a real `World`.
- 57 automated tests; clippy and rustdoc clean.
- Phase 3 approved by director. Local git repository created (GitHub repository
  pending: this session cannot create repositories — see TODO).

### 2026-11-14 — Phase 4 (in progress): procedural world generation
- **Overworld generator** (`ld_worldgen::overworld`), Minecraft-1.18-style
  techniques with our own noises and numbers:
  - Five climate noises (temperature, humidity, continentalness, erosion,
    weirdness), auto-calibrated to a fixed spread so biome thresholds stay meaningful.
  - Terrain shape from hand-tuned curves: deep oceans → shelf → beaches →
    inland plains → hills → mountain ranges, with river valleys where
    weirdness crosses zero.
  - 3D density with roughness-scaled detail noise (cliffs, overhangs),
    computed on a 4×8×4 lattice and trilinearly interpolated.
  - Caves: cheese caverns, spaghetti tunnels (with surface entrances) and
    noodle passages; sea/river floors sealed; ≈8 % of underground volume.
  - Sea level 63, water and ice (cold biomes), lava lakes below Y −54,
    deepslate below Y 0 (mixed 0–8), ragged bedrock floor.
  - Surface rules per biome; bare rock and snow caps on mountains.
  - Ores and rock blobs from `assets/data/ores.toml` with uniform/triangle
    depth distributions modelled on modern Minecraft; deepslate variants.
  - Trees (oak, birch, spruce, cactus) that correctly cross chunk borders;
    plants per biome. Spawn search for dry land.
- **Biomes** (`assets/data/biomes.toml`): 11 core biomes, data-driven.
- **Blocks**: 47 new; `shape = "cross"`, `replaceable`, `water` tint; 45 new textures.
- **Meshing/rendering**: blended per-vertex biome colours (12-byte vertices),
  double-sided plant meshes, translucent water pass.
- **Game**: `--seed`, spawn from generator, F3 shows biome and seed.
- **Developer tools**: `--map`, `--pos`, `--look`, `--fly`, worldgen benches.
- **Quality**: CPU shader-validation test (after a shader typo crashed startup);
  NaN-robustness tests (after a NaN created 320-high walls — fixed).
- Generation optimised from 10.3 ms to 1.3 ms per column.
- 79 automated tests; clippy and rustdoc clean.
- First real-hardware test by the director (Linux): runs at a steady 60 FPS
  (VSync), mouse speed confirmed — KB-001 closed. Added a troubleshooting
  section to the README (touchpad "disable while typing", VSync, glibc).

### 2026-11-14 — Phase 4 (part 2): ruins of the Builders
- Lore outline approved by the director (inspired by the *shape* of fan
  theories about lost builder civilisations; all names and details original).
- **Structure system** (`ld_worldgen::structures`): Minecraft-style random-spread
  placement per region with separation, biome and terrain checks, structures
  spanning chunk borders (each chunk rebuilds overlapping layouts from their
  seed), deterministic per-block decay, foundations down to the ground.
- **Four ruin types**, one per story era: surface ruins (golden age), desert
  halls and sunken towers (expansion), deep vaults in the deepslate (the dark
  chapter). Each holds one lore tablet.
- **Lore**: `assets/data/lore.toml` with 12 draft fragments (3 eras) using
  placeholder names substituted from a names table; parsed and validated by
  `ld_assets::load_lore`. Tablets pick a fragment deterministically (reading
  them comes with interaction in Phase 5).
- **Blocks**: stone/deepslate bricks (+ cracked, mossy, chiseled), cut and
  chiseled sandstone, and the original **lore tablet**; 10 new textures.
- **Underwater view**: blue fog and tint when the camera is in water.
- `--map` marks ruins (red/orange/cyan/purple) and prints the nearest of each kind.
- 83 automated tests; clippy and rustdoc clean.
- Project pushed to GitHub (private repository aizenprivai/Loredrop) with full history.
- Phase 4 approved by director.

### 2026-11-14 — Phase 5: core gameplay
- **Interaction**: block raycasting (grid walk), selection outline, Creative
  break/place/pick-block with Minecraft timings (break every 5 ticks, place
  every 4) and reach (5 Creative / 4.5 Survival); placing respects the
  player's hitbox, replaceable blocks and plant support; plants pop off when
  their ground is removed.
- **Items** (`ld_game::items`, `assets/data/items.toml`): every block is an
  item; 13 materials and 25 tools (5 tiers × 5 kinds) with Minecraft's tier
  stats; 38 original item sprites; isometric 3D block icons.
- **Inventory** (`ld_game::inventory`): 36 slots + armour + off-hand,
  Minecraft click rules (pick up, split, place one, merge, swap), shift-click,
  storage sorting (QoL), hotbar with 1–9 and mouse wheel.
- **UI**: hotbar HUD with counts and durability bars; inventory screen (E)
  with searchable Creative item list; menus release the mouse and block game input.
- **Crafting** (`ld_game::crafting`, `assets/data/recipes.toml`): shaped
  (mirrorable) and shapeless recipes with item tags, 2×2 inventory grid,
  crafting table 3×3, shift-click craft-max (QoL), searchable recipe browser (QoL).
- **Survival mining** (`ld_game::mining`): hardness, effective tools, harvest
  levels and drop tables in blocks.toml; Minecraft's break formula (verified
  against known break times); crack overlay; tool durability and breaking;
  F4 switches Creative/Survival.
- **Dropped items**: item entities with physics, pickup, merging, despawn;
  Q / Ctrl+Q to throw; new entity render pipeline (basis for creatures).
- **Furnace** (`ld_game::furnace`): fuel, 200-tick smelting, cool-down,
  lit/unlit block swap, furnace screen, contents drop when broken.
- **Lore**: right-click tablets to read their fragment (era from the ruin
  type); journal (J) collects discovered fragments.
- New blocks: crafting table, furnace, lit furnace; 10 crack textures.
- 106 automated tests; clippy and rustdoc clean.
- Phase 5 approved by director.

### 2026-11-14 — Phase 6: survival systems
- **Player stats** (`ld_game::stats`): 20 health with Minecraft's
  invulnerability frames; food, saturation and exhaustion (sprint, jump,
  sprint-jump, mining, swimming, damage); fast and slow regeneration;
  starvation limited by difficulty; 15 s of breath then drowning; burning.
- **Damage sources**: falling (1 per block beyond 3), lava (+fire), fire,
  drowning, suffocation, cactus, void. Creative is invulnerable.
- **Swimming**: water and lava movement (slow acceleration, drag, weak
  gravity, swim up with jump, climb out at banks); water cancels fall damage;
  mining under water is 5× slower; can't sprint at 3 shanks or less.
- **HUD**: hearts, hunger, armour and air bars (original 9×9 icons), blinking
  hearts and red flash when hurt, eating progress, attack-cooldown indicator;
  survival stats line in F3.
- **Food**: apples (4 food, 0.3 saturation) drop from oak leaves (0.5 %);
  eating takes 32 ticks; block drops can now list several items.
- **Armour**: iron, gold, diamond sets (12 items, original sprites, Minecraft
  points/toughness/durability, recipes), right-click to wear, slot rules,
  Minecraft's damage-reduction formula, durability loss.
- **Combat rules** (`ld_game::combat`): 1.9+ attack cooldown by weapon speed,
  20 %–100 % damage scaling, critical hits, sword sweep, weapon wear. (Nothing
  to hit until creatures arrive in Phase 7.)
- **Death & respawn**: death screen with cause, inventory spilled as items,
  respawn at world spawn with full stats.
- **Settings**: `gameplay.difficulty` (peaceful/easy/normal/hard),
  `gameplay.game_mode` (new games start in Survival with an empty inventory).
- Fixed: `--pos` no longer moves the world spawn.
- 123 automated tests; clippy and rustdoc clean.
- Phase 6 approved by director.

### 2026-11-14 — Phase 7 (part 1): creatures, AI, pathfinding, combat and loot
- **Creature data** (`ld_game::mobs`, `assets/data/mobs.toml`): stats, drops,
  spawn rules and box models per creature. Cow, pig, sheep, chicken, zombie,
  skeleton and spider, each with original skins (`tools/gen_entity_textures.py`).
- **Rendering** (`loredrop::mobs::render`): box models on the entity pipeline
  with walk, head-turn, wing-flap, hurt-flash and death (fall-over) animation.
- **Movement**: Minecraft-style creature physics — swimming, jumping up
  one-block steps, spiders climbing walls.
- **Pathfinding** (`loredrop::mobs::path`): A* over the block grid; walk, step
  up 1, drop down at most 3; capped search falls back to the closest reachable spot.
- **AI**: passive creatures idle, wander and panic when hurt; hostiles notice
  a visible player within 16 blocks and chase; melee attacks once per second;
  skeletons keep their distance and shoot arrows.
- **Combat**: the Phase 6 rules now hit creatures — cooldown scaling, crits,
  sweep, knockback, 10-tick invulnerability, weapon wear; creatures damage the
  player through armour, scaled by difficulty.
- **Spawning**: animal herds when a column is first loaded; hostiles near the
  player in dark, roofed spots; none on Peaceful. Hostiles despawn far from
  the player (always beyond 128 blocks, occasionally beyond 32); creatures in
  unloaded areas are removed.
- **Items**: raw and cooked beef, porkchop, mutton and chicken (furnace
  recipes), rotten flesh, bone, arrow, feather, string, leather, white wool,
  leather armour set.
- `--summon a,b` developer option; F3 counts creatures.

### 2026-11-14 — Phase 7 (part 2, in progress): the Hollow
- Director-approved design for **the Hollow** (Loredrop original, placeholder
  name): 2.5-block weathered stone figure, gold glow in its cracks, blank face;
  neutral; 40 health, 7 damage; rare (8 %) `lore_fragment` drop.
- Data entry, skins and the `lore_fragment` item added.
- Added `CLAUDE.md` (working agreement and status for new sessions).
- Brought all project documents up to date with Phase 7.

### 2026-11-14 — Phase 7 (part 3): the Hollow's behaviour
- **Neutral anger** (`loredrop::mobs`): Hollows ignore the player until hit;
  then they chase for 30 s (`ANGER_TICKS`) at 1.4× speed (`angry_speed` in
  mobs.toml) and hit for 7. Anger clears in Creative, on death or on Peaceful.
- **Block moving**: while calm, a Hollow sometimes picks up a nearby natural
  block (`carries` list in mobs.toml) with open air above, and later places
  it on solid ground, preferring spots touching earlier Hollow-placed blocks
  so its "builds" grow. Never takes blocks under the player or a creature, or
  blocks a Hollow placed. Placed positions are kept in `Mobs::placed`.
- **Provocation**: breaking a Hollow-placed block angers every Hollow within
  16 blocks. A dying Hollow drops the block it carries.
- **Rendering**: carried block drawn between the hands; arms held forward.
- **Spawning**: Hollows now spawn naturally in dark, roofed places (sharing
  the hostile cap; none on Peaceful) and despawn like hostiles.
- **Lore fragments**: right-click to read a fragment not yet in your journal
  (consumed in Survival); if all are known it shows one again and is kept.
- 9 new tests (138 total) plus an ignored timing test (`creature_tick_cost`).
- Toolchain: fixed warnings raised by Rust 1.100 (clippy `as_chunks`, three
  unused dependencies); clippy clean again.
