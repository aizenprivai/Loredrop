# Loredrop — Game Design Document

> **Status:** APPROVED v1.0 (2026-11-14) · v1.1 adds the approved Hollow design
> **Last updated:** 2026-11-14
> **Director:** Project owner · **Technical lead:** Claude

This document records every creative and design decision made during the Phase 0
interview. It is the source of truth for *what* we are building. *How* we build it
lives in `ARCHITECTURE.md` (Phase 1).

Decisions marked **🔓 OPEN** are not yet decided and will be brought to the director
when the relevant phase approaches.

---

## 1. Vision

**Loredrop** is a voxel sandbox game written in Rust. It aims to feel as faithful as
possible to the modern Minecraft experience (26.x era) — the same rules, controls,
pacing and world feel — while having its own name, art, sound, creatures and
dimensions. Its original twist is a world that tells stories: **ruins with lore**,
**seasons**, **more dramatic biomes** and **richer underground layers**, plus
technical strengths such as **very long render distances** and **extreme
configurability**.

### 1.1 Pillars

1. **Faithful feel.** A Minecraft player should feel at home within seconds.
2. **Runs everywhere.** Stable 60 FPS on ~2014 hardware at low settings.
3. **Everything configurable.** Every setting exposed, every setting explained.
4. **Built to be extended.** Data-driven content and a strong modding API.
5. **A world with history.** Ruins, lore and seasons give exploration meaning.

### 1.2 What "faithful" means (agreed boundary)

| We match closely | We make our own |
|---|---|
| Rules, mechanics, physics, controls, pacing | Name, logo, UI art |
| Movement speeds, jump height, break times, day length | All textures, sounds, music |
| Inventory/crafting behaviour, tool tiers | Iconic Minecraft-specific mobs (own equivalents) |
| Terrain-generation *techniques* (multi-noise, density) | Dimensions (own fire realm + final realm) |
| Generic content: stone, dirt, ores, wood, cows, zombies… | The top tool tier, the final boss |

Worlds are **not seed-compatible** with Minecraft. Seeds can be numbers or text; random if omitted.

---

## 2. Game modes

| Mode | Priority | Notes |
|---|---|---|
| **Creative** | Built first (Phases 3–5) | Infinite blocks, flight, no damage. Doubles as our test mode. |
| **Survival** | Main target (Phase 6) | Health, hunger, danger, progression. |
| **Hardcore** | Later | Survival with one life. Rule variant. |
| **Spectator** | Later | Noclip observer. Rule variant. |

---

## 3. World

### 3.1 Dimensions and height
- Overworld height: **Y −64 to 320** (384 blocks), matching modern vanilla.
- Height is a configurable engine constant, not hard-coded, so it can change later.
- Engine supports **multiple dimensions** from day one.

### 3.2 Terrain style
Modern-style: continents and oceans, large mountain ranges, rivers, erosion-shaped
valleys, 3D overhangs, large cave systems (cheese/spaghetti/noodle-style caves),
open caverns and aquifers.

### 3.3 Biomes
Data-driven (one definition file per biome). **Core starter set (Phase 4):**

> Ocean · Deep ocean · Beach · River · Plains · Forest · Birch forest · Taiga ·
> Desert · Snowy plains · Mountains

Expanded over time. **Original addition — dramatic biomes**, e.g. towering
spires, deep canyons, floating-rock highlands. 🔓 OPEN: exact list.

### 3.4 Underground layers (original)
More varied underground than vanilla: distinct depth bands with their own stone
types, cave biomes, ores and hazards. 🔓 OPEN: number of layers and their themes.

### 3.5 Ruins with lore (original — the game's namesake)
**Approved story outline (2026-11-14):**
1. **The Builders** (placeholder name) — a master-builder civilisation covers the world.
2. They open gateways to the **fire realm** and mine a wondrous material (the top tool tier).
3. Reaching further, they enter the **final realm**; something there transforms
   them into the **hollow wanderers** that roam at night, still moving blocks with
   no memory why (our own iconic creature — neutral, see §5.1 "The Hollow").
4. **The awakened one** — what they touched — is the final boss.
5. Some Builders hid their knowledge in ruins as **lore tablets** ("lore drops");
   the player collects them in a journal and follows the trail.
6. **Ruins by era:** surface ruins (golden age) → desert halls & sunken towers
   (expansion) → deep vaults in the deepslate (what they became).
7. **The survivors** — simple villages of those who refused; passive, later traders.

Progression: surface ruins → deep vaults → fire realm → final realm → boss.
Implemented in Phase 4: four ruin types and tablets (`assets/data/lore.toml`,
12 draft fragments). 🔓 OPEN: final names; journal UI (Phase 5).

### 3.6 Seasons (original)
Seasonal cycle affecting foliage colour, snow cover, crop growth, and possibly
weather and creature behaviour.
⚠️ Tension with "faithful": vanilla has no seasons. Seasons will be a **world
option** that can be turned off. 🔓 OPEN: on or off by default; season length;
gameplay impact (cosmetic only vs. affects farming/survival).

### 3.7 Other dimensions
- **Fire realm** — dangerous, unique resources, required for late-game progression
  (fills the Nether role).
- **Final realm** — endgame location, home of the **final boss** (fills the End role).
- 🔓 OPEN: names, look, creatures, boss design, how players travel between realms.
- Built after the core overworld game is complete.

---

## 4. Progression

Tool and armor tiers: **wood → stone → iron → gold → diamond → [own top tier]**.
🔓 OPEN: name, material and acquisition of the top tier (likely tied to the fire realm).

---

## 5. Creatures and combat

### 5.1 Creatures
- Three categories: **passive**, **neutral**, **hostile** (hostiles mostly spawn in darkness).
- Genre staples used as-is: zombies, skeletons, spiders, common farm and wild animals.
- Minecraft-iconic mobs replaced by our own designs. 🔓 OPEN: further designs.
- Implemented (Phase 7): cow, pig, sheep, chicken (passive); zombie, skeleton,
  spider (hostile). All defined in `assets/data/mobs.toml`.

#### The Hollow (approved 2026-11-14; placeholder name)
Our first original creature, from the lore's hollow wanderers.
- **Look:** tall (2.5 blocks) weathered-stone figure, gold glow in the cracks, blank face.
- **Behaviour:** neutral. Wanders at night and in caves, picking up and placing
  natural blocks "as if building something only it can see".
- **Provocation:** turns hostile if attacked (only the Hollow that was hit —
  director decision), or if the player breaks a block a Hollow placed (every
  Hollow within 16 blocks joins in). Anger lasts 30 seconds.
- **When angry:** fast and strong — 7 damage, 40 health.
- **Drop:** rarely a `lore_fragment` item; right-click to read an undiscovered
  lore fragment into the journal.

### 5.2 Combat — modern (1.9+ style)
Attack cooldown with timing-based damage, weapon speed differences, shields,
sword sweep attacks, critical hits. Code structured so a "classic combat" toggle
can be added later.

---

## 6. Inventory and crafting — vanilla + quality-of-life

**Vanilla base:** 9-slot hotbar, 27-slot inventory, 4 armor slots, offhand slot,
2×2 inventory crafting, 3×3 crafting table, shaped and shapeless recipes,
furnace smelting, recipe book.

**Built-in quality-of-life:**
- Inventory and chest sorting
- Searchable recipe browser (how to craft X / what X is used for)
- Shift-click craft-max and bulk item moving

All recipes are **data-driven** (one file per recipe).

---

## 7. Graphics

### 7.1 Style
**Classic + modern touches:** blocky pixel-art look with smooth lighting,
ambient occlusion, soft shadows, improved water and sky, fog. Heavier effects are
toggles. Original placeholder textures ship with the game.

### 7.2 Resource packs
Supports **Minecraft Java-format resource packs** (e.g. Faithful) that players
supply themselves — never bundled:
- `pack.mcmeta` + `assets/.../textures/block/*.png` layout, folder or `.zip`
- Name mapping from pack texture names to Loredrop blocks; missing textures fall back
- Higher resolutions (32×, 64×, …) and `.mcmeta` animated textures
- Later: sounds (`sounds.json`). Out of scope initially: block-model JSON,
  fonts, OptiFine extras (connected textures).

### 7.3 Original technical features
- **Very long render distances via LOD** (simplified distant terrain)
- **Built-in performance overlay** (FPS, frame time, chunk stats, memory)
- **Built-in shader-style settings** (shadows, water, bloom, etc. without external mods)

---

## 8. Settings — "everything configurable"

Every option is exposed in the in-game settings menu **and** in a human-readable
config file. **Every option carries a description**: a tooltip in-game and a
comment in the config file explaining what it does and its performance cost.

Settings menu tabs (initial plan):

| Tab | Examples |
|---|---|
| **Graphics** | Preset (Low/Medium/High/Ultra/Custom), render distance, LOD distance, FOV, resolution scale, VSync, FPS cap, smooth lighting, ambient occlusion, shadows (quality/distance), water quality, fog, clouds, bloom, particles, mipmaps, anisotropic filtering, entity distance, graphics backend |
| **Performance** | Worker thread count, chunk-builds-per-frame, memory limits, performance overlay |
| **Audio** | Master, music, ambient, blocks, creatures, players, UI volumes; output device |
| **Controls** | Full key rebinding, mouse sensitivity, invert Y, toggle vs. hold for sprint/sneak |
| **Gameplay** | Difficulty, combat options, HUD options, QoL toggles |
| **Interface** | GUI scale, chat, language, accessibility |
| **Resource packs / Mods** | Pack selection & order; mod list & load order; unsafe-native toggle |

Example of the config file style:

```toml
[graphics]
# How far you can see, in chunks (1 chunk = 16 blocks).
# Higher values look better but cost CPU, GPU and memory.
# Low-end hardware: 6–8. Mid: 12–16. High: 24+.
render_distance = 8
```

---

## 9. Audio and atmosphere
- **Music:** vanilla-style sparse ambient tracks with long silences; context sets
  (day, night, caves, fire realm…).
- **Sound effects:** CC0 placeholders, sources logged in `CREDITS.md`.
- **3D positional audio** with distance falloff; cave reverb later.
- **Resource-pack sounds** supported after texture support.

---

## 10. Modding

- **Data packs** (from early on): blocks, items, recipes, biomes, loot, structures.
- **WASM mods (default):** sandboxed, stable across game updates, written in Rust
  via an official `mod-sdk` crate, deep access through the mod API (blocks, items,
  entities, AI, worldgen, events, UI, commands).
- **Unsafe native mods (opt-in):** disabled by default, explicit warning, pinned
  to an exact game build (mismatch refused, not crashed), flagged in crash reports.
- **Ease of use:** project template, hot reload during development, in-game mod
  list with load order and clear error messages.
- Designed in Phase 1; implemented after the core game is stable.

---

## 11. Platforms
- **Desktop first:** Windows, Linux, macOS.
- **Mobile later** (Android, then iOS). Prepared for via abstract input, scalable UI,
  rendering quality tiers, no desktop-only dependencies in the core.
- **NumWorks calculator edition** (side project, started 2026-11-16, director's
  request; director chose **3D first person**). A separate engine in
  `numworks/` sharing the PC game's blocks, textures and numbers. Differences
  forced by the hardware: 48-block-tall world, 6–14 block view distance, no
  saving, simpler lighting. See `numworks/README.md`.

## 12. Performance targets

| Tier | Hardware | Target |
|---|---|---|
| **Minimum** | ~2014 dual/quad-core (i5-4200U), Intel HD 4400/520, 4 GB RAM, Vulkan/DX12/Metal or OpenGL 3.3 fallback | **Stable 60 FPS @ 8 chunks, Low preset** |
| Mid | GTX 1660 / RX 580, 4–6 cores | 60+ FPS @ 16 chunks |
| High | RTX 3070+, 8 cores | 144+ FPS @ 32+ chunks (+ LOD beyond) |

Optimisation is a first-class goal: greedy meshing, face/frustum/cave culling,
palette-compressed chunks, multithreaded generation/meshing/lighting/saving, LOD.

---

## 13. Code standards (director requirements)
- **Rust** as the primary language.
- **Modular:** each system is its own crate/module with clear boundaries.
- **Commented:** doc comments on every module, public type and function;
  inline comments on non-obvious logic; `cargo doc` produces browsable docs.

---

## 14. Key technical challenges

| Challenge | Why it's hard | Mitigation |
|---|---|---|
| 60 FPS on 2014 integrated GPUs | Low fill-rate, low memory bandwidth, old drivers | Greedy meshing, aggressive culling, OpenGL fallback, Low preset tuned first |
| Modern-scale terrain (384-high, big caves) | 3D noise is expensive; 24× more voxels per column than old Minecraft | Multithreaded generation, sub-chunk sections, caching, SIMD-friendly noise |
| Long render distance via LOD | Seams between LOD levels, memory | Planned in architecture; implemented in Phase 10 |
| Smooth lighting across chunk borders | Light propagates between chunks | Dedicated lighting pass with neighbour awareness |
| Java resource-pack compatibility | Name mapping, animations, varied resolutions | Mapping table, texture-array atlas, fallbacks |
| Stable WASM mod API | Must not break with every game update | Versioned API, designed early, implemented late |
| Native mods | No stable Rust ABI, no sandbox | Opt-in, exact-build pinning, clear warnings |
| GPU testing | Dev environment has no GPU | CPU-side benchmarks here; director runs FPS benchmarks |
| Mobile later | Touch, thermals, UI | Abstractions from day one |

## 15. Initial feature priorities

1. **Must have (core):** voxel engine, chunk streaming, worldgen with core biomes,
   Creative mode, inventory/crafting, settings system with described options,
   performance overlay, saving/loading.
2. **Should have:** Survival mode, creatures + AI, day/night, lighting, weather,
   audio, resource packs (textures), modern combat.
3. **Original features:** ruins with lore, seasons, dramatic biomes, underground
   layers, LOD long-distance rendering, shader-style settings.
4. **Later:** fire realm, final realm + boss, WASM mods, native mods, resource-pack
   sounds, Hardcore/Spectator, multiplayer, mobile.

## 16. Open design items (to decide later)
- Own equivalent (if any) of Minecraft's redstone — deliberately not generated (Phase 4)
- Seasons: default on/off, length, gameplay impact
- World lore / backstory and ruin types
- Dramatic biome list; underground layer themes
- Top tool tier name/material
- Fire realm and final realm: names, look, travel method, boss
- Own designs for further iconic mobs (the Hollow is done)
- Experience/XP and enchanting
- Multiplayer: scope and timing
- Logo and visual identity
