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
- [ ] Director approval of Phase 4 report

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

## Infrastructure
- [x] Local git repository with history
- [x] Pushed to GitHub: https://github.com/aizenprivai/Loredrop (2026-11-14)

## Open design items
See GDD.md §16.
