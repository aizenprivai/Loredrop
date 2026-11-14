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
