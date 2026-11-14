# Loredrop — Technical Architecture

> **Status:** APPROVED v1.0 (Phase 1) · updated v1.7 during Phase 8
> **Last updated:** 2026-11-14

This document describes *how* Loredrop is built. *What* we build is in `GDD.md`.
Each section starts with a plain-language summary, followed by technical detail.

---

## 1. Technology selection

### 1.1 Engine: custom engine on wgpu (not Bevy)

**Plain language:** rather than building on top of a general-purpose engine, we
assemble our own small engine from proven Rust libraries. Voxel games spend most
of their effort in systems no general engine provides (chunks, meshing, voxel
lighting), so a general engine would mostly get in the way.

| Option | Verdict | Reasoning |
|---|---|---|
| **Bevy 0.20** | ❌ Rejected | Still pre-1.0 with breaking changes roughly every 3–4 months — a long project would spend real time on upgrades, and our mod API would inherit that churn. Its general-purpose renderer is heavier than we want on 2014 integrated GPUs, and we would replace its rendering for chunks anyway. |
| **Raw Vulkan (ash)** | ❌ Rejected (director decision) | 2–3× more rendering code; no support for Windows GPUs lacking Vulkan drivers; needs MoltenVK on macOS. |
| **wgpu 30 + winit 0.30 + selected crates** | ✅ **Chosen** | Vulkan-first with automatic DX12/Metal/OpenGL fallback; full control over the voxel pipeline; stable, widely used; same code path later works on Android/iOS. |

### 1.2 Library stack

| Area | Crate | Why |
|---|---|---|
| GPU | `wgpu` 30 | Vulkan-first portable graphics; shaders in WGSL, validated by `naga` |
| Window / OS input | `winit` 0.30 | De-facto standard; 0.31 is still beta, we upgrade when stable |
| Math | `glam` 0.34 | SIMD-accelerated vectors/matrices |
| Threading | `rayon`, `crossbeam-channel` | Work-stealing job pool, lock-free message passing |
| Config | `serde`, `toml_edit` | `toml_edit` writes TOML **while preserving comments**, needed for documented settings |
| Debug & settings UI | `egui` 0.37 (+ `egui-wgpu`) | Fast to build menus/overlays; *game* HUD & inventory get a custom textured UI |
| Audio (Phase 8) | `kira` | Game-oriented audio: mixing, spatial sound, streaming music |
| Entities (Phase 7) | `hecs` (proposed) | Small, stable ECS; avoids Bevy churn. Final choice in Phase 7 |
| Compression (Phase 9) | `lz4_flex` / `zstd` | Fast chunk compression for saves |
| Mods (later) | `wasmtime` | Sandboxed, fast WebAssembly runtime |
| Profiling | `tracing` + `tracy-client` | Frame-by-frame CPU profiling with the Tracy GUI |
| Benchmarks | `criterion` | Statistically sound micro-benchmarks for meshing/generation |

---

## 2. Project structure

**Plain language:** the game is split into many small crates (Rust packages).
Each owns one system. Lower crates never depend on higher ones, which keeps
systems independent, lets them be tested alone, and lets Rust compile them in
parallel.

```
loredrop/
├── Cargo.toml            workspace + shared dependency versions
├── crates/
│   ├── loredrop/         executable: wires everything together (kept thin)
│   ├── ld_core/          coordinates, IDs, constants          (no internal deps)
│   ├── ld_config/        settings schema + commented settings.toml
│   ├── ld_input/         device input → game actions, rebinding
│   ├── ld_world/         block registry, chunk storage, loaded world
│   ├── ld_physics/       AABB collision & movement
│   ├── ld_mesh/          meshing (culling, greedy, AO) — CPU only
│   └── ld_render/        wgpu renderer (Vulkan-first)
├── assets/               (Phase 2+) default textures, shaders, data files
└── docs: GDD, ROADMAP, ARCHITECTURE, CHANGELOG, TODO, KNOWN_BUGS, BENCHMARKS
```

Planned crates, added when their phase starts: `ld_worldgen` (4), `ld_game`
(gameplay rules, modes, inventory, crafting — 5), `ld_ui` (5), `ld_assets`
(resource packs — 5), `ld_entity` (7), `ld_audio` (8), `ld_save` (9),
`ld_mod_api` + `ld_mod_host` (later).

### 2.1 Dependency rules

```
                 loredrop (exe)
   ┌──────┬──────┬───┴────┬────────┬────────┐
ld_render ld_game ld_ui  ld_audio ld_save  ld_mod_host
   │       │ └──────┐                 │
ld_mesh  ld_entity ld_worldgen  ld_physics
   └───────┴───────┬┴─────────────────┘
               ld_world   ld_config   ld_input
                    └────────┬───────────┘
                          ld_core
```

- `ld_world`, `ld_mesh`, `ld_worldgen`, `ld_physics` have **no GPU or OS
  dependency** → testable and benchmarkable headless (important: our dev machine
  has no GPU).
- Only `ld_render` talks to wgpu. Only the executable talks to winit.
- Gameplay crates talk to each other through plain data and events, never
  through the renderer.

---

## 3. Core loop and threading

**Plain language:** the game simulation ticks at a fixed **20 times per second**
exactly like Minecraft, which keeps behaviour faithful (crop growth, mob speed,
redstone-like timing). Rendering runs as fast as the screen allows and smoothly
interpolates between ticks. Heavy jobs run on background threads so the game
never freezes.

| Thread | Work |
|---|---|
| **Main** | OS events, input, fixed 20 TPS simulation, render submission |
| **Worker pool** (rayon, `cores − 1`, configurable) | World generation, lighting, meshing |
| **I/O thread** (Phase 9) | Reading/writing region files |
| **Audio thread** (kira) | Mixing |

Communication rules:
- Workers receive **immutable snapshots** of the data they need (e.g. a section
  plus its 1-block neighbour border) and send results back over channels.
  No locks on hot paths.
- The main thread caps how many finished meshes it uploads to the GPU per frame
  (setting: `performance.chunk_uploads_per_frame`) to avoid frame spikes.
- Jobs carry a **version number**; if a chunk changed while its mesh was being
  built, the stale result is discarded.
- Jobs are prioritised by distance to the player and whether they're in view.

---

## 4. Chunk data structures

**Plain language:** a chunk column (16 × 384 × 16 blocks) is split into 24
stacked 16-block cubes called **sections**. Each section stores a small list of
the block types it contains ("palette") and, for each block, a tiny index into
that list. Empty air sections cost almost nothing. This is how modern Minecraft
saves memory too.

- **Block states:** a global `u16` ID per block state (e.g. "oak log, axis=Y").
  Up to 65 536 states. Properties (solid, transparent, light emission, hardness…)
  come from data files loaded into a registry at startup.
- **Section storage:** palette + bit-packed indices, bit width chosen per section
  (0 bits for a uniform section, 4 bits for ≤16 types, … up to 16 bits).
  Typical memory: 0 bytes (uniform) to ~2 KB per section, vs 8 KB uncompressed.
- **Light:** *(changed in Phase 8 — see "Changes made during Phase 8")* not
  stored; computed when meshes are built.
- **Column:** 24 sections + heightmaps + biome data (biomes stored per 4×4×4 cell
  like modern Minecraft).
- **World map:** `AHashMap<ChunkPos, ChunkColumn>` per dimension; chunks move
  through states `Generating → Lighting → Meshing → Ready`.
- **Coordinates:** blocks use `i32`; entity/camera positions use `f64`, and the
  renderer draws **camera-relative** so there's no jitter far from spawn.

---

## 5. Rendering pipeline

**Plain language:** for each section we build a mesh only of the block faces you
can actually see, merge neighbouring identical faces into big rectangles (greedy
meshing), and pack each vertex into a few bytes. The GPU then only draws
sections inside your field of view.

1. **Meshing (worker threads):** hidden-face culling → greedy meshing → per-vertex
   ambient occlusion & light. Separate opaque, cutout (leaves, grass) and
   translucent (water, glass) meshes.
2. **Compact vertices:** 8 bytes per vertex (position in section, face, texture
   layer, AO, light). ~4–6× smaller than a naive layout → much less memory
   bandwidth, crucial on integrated GPUs.
3. **Textures:** a **2D texture array** (one layer per block texture) instead of an
   atlas. Lets greedy-meshed quads tile textures correctly, and makes resource
   packs (Faithful, any resolution) a matter of filling layers. Mixed-resolution
   packs are scaled to the largest size.
4. **GPU memory:** meshes live in large shared buffers managed by our own
   allocator (fewer, bigger allocations; cheap updates).
5. **Culling:** frustum culling per section on the CPU; cave/occlusion culling
   (skip sections hidden behind solid terrain) in Phase 10.
6. **Pass order:** opaque (front-to-back) → cutout → sky → translucent
   (back-to-front) → particles → UI/overlay.
7. **Quality tiers:** every expensive effect (shadows, water quality, AO, fog,
   clouds, bloom) is behind a setting with a Low preset tuned for 2014 iGPUs.
8. **LOD (Phase 10):** distant chunks rendered as simplified meshes at 2×/4×/8×
   block size, for render distances far beyond what full detail allows.

Vulkan: requested first; if no Vulkan adapter exists, wgpu falls back to
DX12/Metal, then OpenGL. Backend can be forced in settings. Draw-call batching
uses multi-draw-indirect where the backend supports it.

---

## 6. Memory strategy

- Palette-compressed sections; uniform sections store no per-block data.
- Chunks beyond render distance + 2 are unloaded (saved first if modified).
- Reusable scratch buffers per worker thread for meshing (no per-job allocation).
- Memory budget setting with an on-screen warning in the performance overlay.
- Target: render distance 8 must fit comfortably in a 4 GB system.

---

## 7. World storage format (Phase 9, designed now)

- One folder per world: `world.toml` (seed, settings, version), `players/`,
  `<dimension>/regions/`.
- **Region files:** 32 × 32 chunks per file, with a header table of offsets, each
  chunk compressed individually (LZ4 default, zstd optional).
- Every chunk and file carries a **format version** with upgrade code, so old
  worlds keep loading.
- Safe saving: write to a temp file, then rename (never a half-written save);
  automatic rotating backups.

---

## 8. Settings system

**Plain language:** every setting is defined exactly once, with its description.
That one definition produces the menu entry, its tooltip, and a commented line in
`settings.toml`, so the description can never be missing or out of date.

Each setting declares: key, category/tab, type, default, valid range, a
description, a **performance impact note**, and whether it applies live or needs
a restart. Invalid values fall back to the default with a warning — a broken
config file never stops the game from starting. See GDD §8 for the tabs.

---

## 9. Modding architecture (designed now, built later)

- All content (blocks, items, recipes, biomes, loot) is **data-driven** from day
  one; the base game itself is loaded as the first "data pack".
- Engine code exposes behaviour through **registries and events** (e.g.
  `BlockPlaced`, `EntityDamaged`) rather than hard-wired calls — the same hooks
  mods will use later.
- `ld_mod_api`: a versioned API for both WASM mods (wasmtime, sandboxed) and
  opt-in unsafe native mods (pinned to an exact game build).

---

## 10. Development & testing tools

| Tool | Purpose |
|---|---|
| `cargo test` | Unit tests per crate (coordinates, chunk storage, meshing, physics) |
| `criterion` benches | Meshing/generation/lighting speed, tracked in BENCHMARKS.md |
| Headless runs | Generate & mesh worlds without a window — works on GPU-less CI |
| `cargo clippy`, `cargo fmt` | Lint and format on every change |
| `cargo doc` | Browsable docs from code comments |
| Tracy | Frame-level CPU profiling |
| In-game debug overlay | FPS, frame time, chunk counts, memory, position (Phase 2) |
| Benchmark command | `loredrop --benchmark` flythrough producing a report the director runs on real hardware |

**Testing limitation:** the development machine has no GPU, so FPS can only be
measured on the director's hardware. CPU-side systems are measured here.

---

## 11. Coding standards

- Every crate, module, public type and function has doc comments; non-obvious
  logic has inline comments explaining *why*.
- `unsafe` is avoided; any use requires a `// SAFETY:` comment.
- No `unwrap()` on anything that can fail at runtime from user data (configs,
  saves, resource packs) — show an error and recover.
- Rust 2024 edition, minimum Rust 1.90.

---

## 12. Changes made during Phase 2 (v1.1)

- **Crate dependencies adjusted:** `ld_config` depends on `ld_input` (key
  bindings are settings); `ld_render` depends on `ld_config` (backend choice);
  `ld_physics` no longer depends on `ld_world` — terrain is accessed through the
  `VoxelCollider` trait, so physics works with any block source.
- **egui colour handling:** the UI draws through a non-sRGB *view* of the frame.
  Drivers that can't create such views (seen on OpenGL) get a non-sRGB frame and
  the world shader gamma-encodes itself. Both paths verified to give matching
  images.
- **Screenshots:** read back directly when the surface allows it; otherwise the
  frame is re-rendered into an offscreen texture (OpenGL path).
- **Fallback rule:** a forced backend that fails to start falls back to `auto`
  rather than refusing to launch.
- Phase 2 uses a temporary 16-byte coloured vertex and one mesh per test scene;
  Phase 3 replaces this with the packed chunk format described in §5.

### Changes made during Phase 3 (v1.2)

- **New crates created earlier than planned:** `ld_worldgen` (Phase 4) now
  holds the `TerrainGenerator` trait and a temporary test generator;
  `ld_assets` (Phase 5) handles asset discovery, block files and textures.
- **Streaming pipeline lives in the executable** (`loredrop::world`) for now.
  It talks to the GPU through a small `SectionSink` trait, so it is tested
  headlessly. It may move into its own crate when gameplay systems need it.
- **GPU buffers:** each section currently gets its own vertex buffer, with a
  shared quad index buffer and per-draw instance slices for origins. The large
  shared-buffer allocator in §5.4 is **deferred to Phase 10**, together with
  multi-draw-indirect, once profiling on real GPUs shows where the cost is.
- **Texture coordinates** are computed in the shader from vertex positions
  rather than stored, keeping vertices at 8 bytes and making greedy quads tile.
- **Meshing condition:** a column is meshed only once all 8 neighbours are
  generated; unloaded neighbours act as solid walls for physics.

### Changes made during Phase 4 (v1.3)

- **World generation** pipeline: climate → terrain shape → lattice density
  (4×8×4, interpolated) → fill → biomes → surface → ores → vegetation, on a
  flat per-column block array packed into palette sections at the end.
- **Cross-chunk features without neighbour data:** `column_profile(x, z)`
  reproduces chunk maths exactly, so chunks place trees owned by neighbours.
  Rule: feature decisions may only use what `column_profile` can compute.
- **Data files:** `biomes.toml` and `ores.toml` join `blocks.toml`.
- **Vertex format is 12 bytes** (+ per-vertex biome RGB). Biomes stored per column.
- **Third render pass:** translucent (blended, no depth write, back-to-front).
- **Shaders validated on the CPU in tests** (naga).

### Changes made during Phase 5 (v1.4)

- **New crate `ld_game`** (planned in §2): items, inventory and click rules,
  crafting/smelting recipes, mining rules (break formula, harvest levels,
  drop tables), furnace logic. Pure logic, fully unit-tested.
- **New data files:** `items.toml`, `recipes.toml`; blocks gained
  `hardness`, `tool`, `harvest_level`, `drops`, `item`.
- **Entity rendering:** a free-form textured-triangle pipeline (camera-relative
  f64 → f32 on the CPU) shares the block texture array, which now also holds
  item sprites and crack stages. Used for dropped items and mining cracks;
  creatures will reuse it.
- **Menus** (`loredrop::screens`) are egui screens drawn with our own slot
  painter; while open they release the mouse and set `Game::input_blocked`.
  Mutable game state (inventory, the open furnace) is lent to the menu for the
  frame and returned afterwards, so the overlay can read the game meanwhile.
- **Block entities:** furnaces live in `Game::furnaces` keyed by position;
  a general block-entity store (chests etc.) will follow the same pattern and
  must be saved in Phase 9.

### Changes made during Phase 6 (v1.5)

- `ld_game` gained `stats` (health/hunger/air/fire state machine, armour
  formula, fall damage) and `combat` (cooldown and hit rules). Both are pure
  and unit-tested against Minecraft's published numbers.
- `ld_physics::player` handles liquids (`Fluid`) and tracks fall distance,
  reporting landings to the game rather than applying damage itself — physics
  stays free of gameplay rules.
- Environmental damage is checked in `Game::survival_tick` from world queries
  (body/eye blocks, touched blocks); `Game::hurt` routes all damage through
  armour, so creatures in Phase 7 use the same path.
- New `Screen::Death`; app-level flow: death detected after the tick →
  inventory spilled once → death screen → Respawn resets the game state.

### Changes made during Phase 7 (v1.6, in progress)

- **No ECS yet:** `hecs` (proposed in §1.2) was not adopted. Creatures are a
  plain `Vec<Mob>` inside `loredrop::mobs::Mobs`, owned by `Game`. With a few
  dozen creatures this is simpler and fast enough; an ECS will be reconsidered
  if entity counts or kinds grow (e.g. for mods). The planned `ld_entity`
  crate is therefore not created.
- **Data vs behaviour split:** `ld_game::mobs` holds pure data (stats, drops,
  spawn rules, box models, `Category` = passive/neutral/hostile) loaded from
  `mobs.toml`; AI, spawning and physics live in `loredrop::mobs`, rendering
  in `loredrop::mobs::render`.
- **AI** is a small state machine per creature (`Brain`: Idle, Wander, Panic,
  Chase). Neutral creatures add an `anger` countdown: while it runs they use
  the hostile Chase logic.
- **Creatures never edit the world directly.** `Mobs::tick` only reads the
  world; block changes come back as `MobEvent::SetBlock` and `Game` applies
  them with `World::set_block` (so remeshing works as for the player). A
  per-tick `claimed` set stops two creatures using the same spot in one tick.
- **Hollow-placed blocks** are tracked in `Mobs::placed`; the player's block
  removal path (`Game::remove_block`) calls `Mobs::block_broken`, which
  provokes nearby Hollows. Must be saved with the world in Phase 9.
- Deaths are reported as `Death { pos, kind, carrying }` so carried blocks drop.
- **Pathfinding** (`loredrop::mobs::path`) is A* behind a `PathWorld` trait,
  so it is unit-tested without a real world; searches are node-capped.
- **Events:** creature attacks reach the player as `MobEvent::HurtPlayer`,
  applied through `Game::hurt` (armour applies, like all other damage).
- Box models render through the Phase 5 entity pipeline, with creature skins
  added to the shared texture array. Carried blocks reuse the dropped-item
  cube look (`ItemLook::Cube`), passed to the renderer as a lookup closure.

### Changes made during Phase 8 (v1.7, in progress)

- **Time & sky:** `ld_game::time` (pure) produces a `SkyState`; the renderer
  draws a full-screen sky pass first (`sky.wgsl`) and receives `daylight`.
  The camera uniform grew (inverse view-projection, sky colours, sun, misc);
  shaders that don't need the new fields declare only a prefix.
- **Lighting replaces §4's stored light (director decision, 2026-11-14).**
  Light is never stored or saved. Each mesh job covers all dirty sections of
  one column, computes one `LightRegion` (flood fill) reaching 15 blocks
  around them, and meshes each section with a `LightView` of it. Vertices
  carry 4-bit sky and block light; the shader applies the light curve and
  daylight. Rationale: no incremental light updates to get wrong, nothing to
  save, results identical except for light paths leaving the 15-block box.
- **Edits that change light** (filter or emission differ) mark the 3×3×3
  sections around, plus — if sky light can change — the sections below down
  to 15 blocks under the next opaque block. One job per column keeps it cheap.
- **Gameplay light queries:** the light of each meshed, non-empty section is
  cached in `World` (`SectionLight`, uniform sections cost nothing) and read
  by `World::light_at`; without data it falls back to a sky-visibility scan.
- **Entities** get light through the top byte of `EntityVertex::color`.

## 13. Decisions approved in Phase 1

1. ✅ Custom engine on wgpu instead of Bevy (§1.1).
2. ✅ Fixed 20 TPS simulation, faithful to Minecraft (§3).
3. ✅ Texture arrays instead of a texture atlas (§5).
4. ✅ egui for debug/settings menus; custom UI for in-game HUD/inventory (§1.2).
