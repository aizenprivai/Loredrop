# TODO

## Phase 2 — Core engine foundation
- [x] Window + wgpu init with Vulkan-first backend selection and fallbacks
- [x] Fixed 20 TPS game loop with render interpolation
- [x] Input actions + key bindings
- [x] Settings system (schema, descriptions, commented settings.toml)
- [x] Camera + player controller (walk, sprint, sneak, jump, creative flight)
- [x] AABB physics & collision
- [x] Debug/performance overlay (F3), crosshair, screenshots (F2), fullscreen (F11)
- [x] Simple 3D test environment
- [x] Director approval of Phase 2 report (2026-11-14)
- [x] Director tested on real hardware (Linux): 60 FPS VSync-capped, mouse OK
- [ ] Uncapped FPS + GPU model; Windows/macOS startup (later)

## Deferred from Phase 2 (planned later)
- [ ] In-game settings menu (Phase 11; file editing works now)
- [ ] Hot-reload of settings.toml while the game runs
- [ ] Sneak edge-protection (don't fall off ledges while sneaking) — Phase 5/6
- [ ] Step-up for slabs/partial blocks (0.6 blocks) — when partial blocks exist (Phase 5)
- [ ] Sprint/fly FOV change ("FOV effects" setting) — Phase 11
- [ ] View bobbing — Phase 11
- [ ] Pause menu on Esc (currently Esc only releases the mouse) — Phase 11
- [ ] Platform config folder for settings (currently current directory) — Phase 9
- [ ] CI pipeline (needs a git repository)

## Phase 3 — Voxel engine & chunk system
- [x] Block registry (data-driven block types & properties)
- [x] Palette-compressed 16³ sections, 24-section columns
- [x] Chunk map, load/unload by render distance, worker-thread jobs
- [x] Meshing: face culling across chunk borders, greedy meshing, AO
- [x] Packed 8-byte vertices, texture array + mipmaps, placeholder textures
- [x] Section-level frustum culling, render-distance setting
- [x] Meshing/storage benchmarks (criterion) + headless world benchmark
- [x] Replace the test scene with real chunks
- [x] Director approval of Phase 3 report

## Deferred from Phase 3
- [ ] Shared GPU buffer arena for section meshes (ARCHITECTURE §5.4) — currently one buffer per section; revisit in Phase 10 with multi-draw-indirect
- [ ] Translucent render pass (water, stained glass) — Phase 4 (oceans) / Phase 8
- [ ] Cave/occlusion culling — Phase 10
- [ ] Mesher speed-up (0.26 ms/section now; target < 0.1 ms) — Phase 10
- [ ] Texture animation (`.mcmeta`) — with resource packs (Phase 5)
- [ ] Grass-side overlay tinting for Minecraft resource packs (`grass_block_side_overlay`) — Phase 5
- [ ] Block light / sky light values in vertices (bits reserved) — Phase 8
- [ ] Save block *names* in world files and remap IDs on load — Phase 9

## Phase 4 — Procedural world generation
- [x] Seeded world creation; own Perlin/octave/double noise (benchmarked)
- [x] Continents/erosion/peaks height shaping; 3D density for overhangs
- [x] Core biome set (GDD §3.3) with data-driven biome files and biome tints
- [x] Caves (cheese/spaghetti/noodle-style); sealed sea floors; lava lakes
- [x] Ore distribution; trees/vegetation per biome (cross-chunk trees)
- [x] Oceans and rivers (water block + translucent pass)
- [x] Lore outline approved; structure system + 4 ruin types with lore tablets
- [ ] Final names for the Builders, realms, wanderers, boss, material (placeholders in lore.toml)
- [x] Director approval of Phase 4 report

## Deferred from Phase 4
- [ ] Reading lore tablets + lore journal (needs interaction) — Phase 5
- [ ] Data-driven structure templates (ruins are procedural code for now)
- [ ] More ruin types (roads, gateways to the fire realm, villages of the survivors) — later
- [ ] Aquifers (local underground water levels) — caves are always dry above Y −54
- [ ] Non-cube blocks (slabs, stairs, snow layer, lower water surface)
- [ ] Water/lava flow — Phase 8
- [ ] More jagged mountain peaks
- [ ] Fewer river loops / tiny inland ponds (tuning)
- [ ] Biome-colour blend radius setting (fixed 3×3 now)
- [ ] Underground biomes; "dramatic biomes"; underground layers (GDD originals)
- [ ] Own equivalent of Minecraft's redstone (deliberately omitted; GDD §16)

## Phase 5 — Core gameplay
- [x] Block destruction and placement (Creative + Survival), pick block
- [x] Interaction system (use blocks: crafting table, furnace, lore tablet)
- [x] Inventory, hotbar, item management (click rules, shift-click, sorting)
- [x] Crafting (2×2, 3×3, recipes data file, craft-max, recipe browser)
- [x] Tools, mining speeds, harvest tiers, durability
- [x] Basic progression: wood → stone → iron (furnace) → diamond
- [x] Dropped items; lore tablets + journal
- [x] Director approval of Phase 5 report

## Deferred from Phase 5
- [ ] Falling blocks (sand, gravel) — Minecraft core behaviour, not yet implemented
- [ ] Leaf decay, saplings, tree growth; seeds and farming (hoes till soil)
- [ ] Chests (item storage block), torches (needs lighting, Phase 8), doors, ladders, beds, signs
- [x] Armour equipping and protection (Phase 6)
- [ ] Off-hand use
- [ ] Recipe-book "auto-fill" (place ingredients for a recipe automatically)
- [ ] Items left on the cursor when a menu closes with a full inventory are lost (should drop)
- [ ] Journal, inventory and furnaces are not saved yet (Phase 9)
- [x] Survival starts with an empty inventory (Phase 6)
- [ ] Final lore names (placeholders in lore.toml)

## Phase 6 — Survival systems
- [x] Health, hunger, saturation, exhaustion, regeneration, starvation
- [x] Damage: fall, lava/fire, drowning, suffocation, cactus, void
- [x] Swimming in water/lava
- [x] Food (apples) and eating
- [x] Armour (iron/gold/diamond) with Minecraft's formula and durability
- [x] Combat rules (cooldown, crits, sweep) + indicator
- [x] Death screen, item drops on death, respawn
- [x] Difficulty and starting game-mode settings
- [x] Director approval of Phase 6 report

## Deferred from Phase 6
- [x] Hitting creatures, knockback, mob damage to the player (Phase 7)
- [x] Meat from animals and cooked food in the furnace (Phase 7)
- [ ] Bread / farming
- [x] Leather armour (Phase 7)
- [ ] Chainmail armour (Minecraft has no recipe for it; source to decide)
- [ ] Experience/XP and enchanting — not planned yet (design decision needed)
- [ ] Bed / custom spawn point
- [ ] Hunger effects on movement while eating (Minecraft slows you)
- [ ] Health/hunger saved between sessions — Phase 9

## Phase 7 — Entities & AI
- [x] Data-driven creatures (`assets/data/mobs.toml`): stats, drops, spawn rules, box models
- [x] Original skins: cow, pig, sheep, chicken, zombie, skeleton, spider
- [x] Box-model rendering with walk/head/wing/hurt/death animation
- [x] Creature physics (swimming, step jumping, spider wall climbing)
- [x] A* pathfinding (capped search, avoids drops over 3 blocks)
- [x] Passive AI (idle, wander, panic) and hostile AI (chase, melee, ranged with arrows)
- [x] Player attacks on creatures (1.9+ rules, knockback, sweep, weapon wear); creature attacks by difficulty
- [x] Spawning (herds on chunk load; hostiles in dark roofed spots), despawning, loot
- [x] New foods + cooking, leather armour, wool; `--summon` developer option
- [x] The Hollow: design approved, data, skins, `lore_fragment` item
- [x] Hollow: neutral anger state (30 s, faster when angry; chase only when provoked)
- [x] Hollow: pick up / carry / place natural blocks; remember placed positions
- [x] Hollow: breaking a Hollow-placed block angers Hollows within 16 blocks
- [x] Hollow: render the carried block in its arms (arms forward)
- [x] Hollow: natural spawning in dark roofed places (with the hostiles)
- [x] Right-click `lore_fragment` → read an undiscovered fragment into the journal
- [x] Tests, screenshots, docs, Phase 7 report
- [x] Director approval of Phase 7 report

## Deferred from Phase 7
- [ ] Creatures are not saved (Phase 9)
- [ ] Creature sounds (Phase 8 audio)
- [ ] Hollow-placed block list is not saved (Phase 9)
- [ ] Long-legged walk animation looks exaggerated on the Hollow (tune swing by leg length)

## Phase 8 — World simulation & atmosphere
- [x] Part 1: day/night cycle (Minecraft timing), sky gradient, sunrise/sunset glow, sun, moon phases, stars; daylight dims the world; settings + `--time`; F3 clock
- [x] Part 2: lighting — sky + block light (flood fill per mesh job), smooth lighting, dark caves, cave fog, lava/lit furnace/torch light, torch block + recipe, brightness setting
- [x] Part 3: spawning by light level (surface at night, caves), zombies/skeletons burn in daylight, spiders calm in bright light
- [ ] Part 4: weather (rain, snow, thunderstorms)
- [ ] Part 5: water/lava flow; falling sand and gravel
- [ ] Part 6: seasons (design questions for the director first)
- [ ] Part 7: audio (kira): ambient, block, creature sounds; music system
- [ ] Phase 8 report, launch the game for the director
- [ ] Director approval of Phase 8 report

## Deferred from Phase 8
- [ ] Fog toward the sunset should take the glow colour (now always the horizon colour)
- [ ] Wall torches (torches only stand on the ground for now)
- [ ] Sub-block geometry in the chunk vertex format (needed for real torch models, slabs, stairs)

## NumWorks edition — NW phase 1 (`numworks/`)
- [x] Separate workspace: `core` (no_std game), `desktop` (PC harness), `app` (calculator)
- [x] Blocks + textures generated from the PC game's data
- [x] World generation (biomes, caves, ores, trees), 32x32 ring window + edit log
- [x] Ray-casting renderer, sky, day/night, dark caves, water, fog, outline, cracks
- [x] Physics (Minecraft numbers), survival mining, creative mode, hotbar
- [x] Title / pause / settings / controls screens
- [x] `.nwa` build + check with NumWorks' `nwlink`
- [ ] **Director's test on a real calculator** (does it install? FPS? controls?)
- [ ] Test on the official NumWorks simulator (building it was blocked in the cloud session)
- [ ] NW phase 1 approval

## NumWorks edition — NW phase 2 (survival)
- [x] Items, tool tiers, durability, drops (generated from the PC data)
- [x] 36-slot inventory, 2x2 / 3x3 crafting, recipe book, creative palette
- [x] Furnaces (smelting, fuels, lit block)
- [x] Health, hunger, saturation, regeneration, starvation, fall damage, drowning, lava/fire
- [x] Eating, death screen, respawn
- [ ] NW2 report approval
- [ ] Real-calculator test (still pending since NW1)

## NumWorks edition — NW phase 3 (creatures, combat, lore)
- [x] Creatures from the PC data (models, textures, numbers), AI, spawning
- [x] The Hollow: block moving, provocation rules
- [x] Combat (attack cooldown, knock-back), armour slots and formula
- [x] Dropped items (pickup, merge, lifetime, death drops, throwing)
- [x] Ruins (surface, desert halls, deep vaults) with lore tablets; journal
- [ ] NW3 report approval
- [ ] Real-calculator test (still pending since NW1)

## NumWorks edition — NW phase 4 (speed & polish)
- [x] Pixel size 3 and automatic pixel size; timing display
- [x] Layer-major world storage (faster ray block reads)
- [x] Torches with block light; no monster spawns in torch light
- [x] Plants (cross-shaped blocks) in world generation
- [x] Creatures walk around walls
- [ ] NW4 report approval
- [ ] Real-calculator test (still pending since NW1) → tune defaults from real timings

## Deferred / next for the NumWorks edition
- [ ] Full path-finding for creatures (only wall-sidestepping for now)
- [ ] Wall torches; light blocked by walls; lava / lit furnace light
- [ ] Sheep shearing, breeding, baby animals
- [ ] Plants (grass, flowers) — need cross-shaped sprites in the ray caster
- [ ] Falling sand/gravel, flowing water
- [ ] Torches and block light
- [ ] Creatures, the Hollow, ruins with lore tablets (NW3)
- [ ] Speed: tune after the first real FPS numbers (NW4)

## Infrastructure
- [x] Local git repository with history
- [x] Pushed to GitHub: https://github.com/aizenprivai/Loredrop (2026-11-14)

## Open design items
See GDD.md §16.
