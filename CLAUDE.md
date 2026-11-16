# CLAUDE.md — instructions for Claude sessions working on Loredrop

Read this first. It replaces the memory of earlier conversations.

## Roles and working agreement (set by the director, the project owner)

- The user is the **game director**; Claude is the **technical lead**.
  The director makes creative decisions; Claude handles architecture,
  implementation, testing and documentation.
- **Ask questions ONE AT A TIME** and wait for the answer.
- **Never start a new phase without the director's approval.** Each phase
  ends with a report: features implemented, tested, missing, known bugs,
  performance observations, technical difficulties, architecture changes,
  next objectives. Then ask for approval.
- Ask before major architecture or game-design changes. Explain technical
  topics in plain language. Be honest about limitations and untested work.
- Never claim a feature works without testing it.
- Keep these documents up to date: `GDD.md`, `ROADMAP.md`, `ARCHITECTURE.md`,
  `CHANGELOG.md`, `TODO.md`, `KNOWN_BUGS.md`, `BENCHMARKS.md`, `README.md`.
- Commit and push regularly (GitHub: `aizenprivai/Loredrop`, branch `main`).
- **When a phase is finished, launch the game for the director to test**
  (release build, normal window, no `--exit-after`), right after the report.
  Run it in the background so the session isn't blocked.

## Creative direction (see GDD.md for details)

- A voxel sandbox **as faithful as possible to modern Minecraft (26.x)** in
  rules, mechanics, numbers and feel — but with **its own identity**: own name
  (Loredrop), art, sounds, music, and own designs for Minecraft-specific
  creations (iconic mobs, dimensions, redstone). Never copy Minecraft's
  textures, names of its unique mobs/places, music or exact content.
- Generic staples are fine (stone, ores, cows, zombies, skeletons, spiders…).
- Code must be **modular** and **commented** (doc comments on all modules,
  public items, and non-obvious logic). Every setting must be configurable
  and carry a description.
- Performance target: **60 FPS on ~2014 hardware** at render distance 8.
- Original features: ruins with lore (the game's namesake), seasons,
  dramatic biomes, underground layers, long render distances via LOD,
  built-in performance overlay and shader-style settings.

## Current status (update this section as work progresses)

- Phases 0–7 complete and approved. **Phase 8 (world simulation &
  atmosphere) in progress** — see the Phase 8 section of TODO.md.
- Done in Phase 7: data-driven creatures (`assets/data/mobs.toml`), box-model
  rendering with animation, pathfinding, passive/hostile AI, ranged
  skeletons, combat against creatures, spawning, despawning, loot, new foods,
  leather armour, `--summon` developer option.
- **The Hollow** (Loredrop original, placeholder name) — design approved by
  the director: tall (2.5 blocks) weathered stone figure, gold glow in the
  cracks, blank face; **neutral**: wanders at night/in caves moving blocks
  "as if building something only it can see"; turns hostile if attacked or if
  the player breaks a block it placed; fast and strong when angry (7 damage,
  40 health); rarely drops a `lore_fragment` item. **Fully implemented**
  (anger, block moving, provocation, carried-block rendering, natural
  spawning, readable lore fragments). Director decision: attacking one
  Hollow angers only that one.
- **NumWorks calculator edition** (side project in `numworks/`, started
  2026-11-16 at the director's request; director chose **3D**). NW phase 1
  done on PC (engine, worldgen, survival mining, creative, menus). The
  director couldn't test on the calculator yet and approved starting NW2.
  **NW2 done and approved** (items/tools/drops, inventory, crafting +
  recipe book, furnace, health/hunger/fall/drowning/lava, death).
  **NW3 done and approved** (8 creatures incl. the Hollow, combat, armour,
  dropped items, ruins + lore tablets + journal). **NW4 done** (auto pixel
  size 2/3/4, *Show timings*, layer-major world, torches + block light,
  plants, wall-sidestepping creatures). **NW5 max-speed pass done**
  (integer compares, incremental index, sky skipping, creature sphere
  culling, M7 tuning, auto view distance) — **NW4 and NW5 approved**
  (2026-11-16). **First real test (director's N0120): works very well,
  `3D63 G0 B0 P2 V12`** (≈ 16 FPS; only the 3D view costs anything).
  Release `numworks-v0.5` on GitHub holds the .nwa.
  Phase 8 of the PC game is paused, not abandoned. See `numworks/README.md`.
  **NumWorks work lives on the GitHub branch `numworks`** (created at the
  director's request, 2026-11-16); `main` still holds the PC game only.
  Cloud sessions can't build/run the official NumWorks simulator (the
  sandbox blocked building downloaded code); test with `ld_nw_desktop`.
- Open design items (ask the director when relevant): final lore names,
  fire realm / final realm / boss, top tool tier, redstone equivalent,
  XP/enchanting, multiplayer.

## Project layout and tools

- Rust workspace: `crates/` (`ld_core`, `ld_config`, `ld_input`, `ld_world`,
  `ld_worldgen`, `ld_mesh`, `ld_render`, `ld_physics`, `ld_assets`, `ld_game`,
  `loredrop` = the executable). Data in `assets/data/*.toml`; placeholder art
  generated by `tools/*.py` (Pillow).
- `cargo test --workspace` (≈138 tests, no GPU needed),
  `cargo clippy --workspace --all-targets` (keep at zero warnings),
  `cargo fmt --all`, `cargo bench -p ld_mesh|ld_world|ld_worldgen`.
- NumWorks edition: `cd numworks && cargo test && cargo clippy --all-targets`;
  `./numworks/build_nwa.sh` builds `loredrop.nwa` (needs
  `rustup target add thumbv7em-none-eabihf`); screenshots with
  `cargo run --release -p ld_nw_desktop -- shot ...`.
- Useful run options: `--seed`, `--pos x y z`, `--look yaw pitch`, `--fly`,
  `--summon a,b`, `--map <blocks> <file.png>`, `--bench-world <rd>`,
  `--screenshot-after <frames> --screenshot-path <file>`, `--exit-after <s>`.
- The director's PC has rustc 1.100 (stricter clippy than the cloud's);
  git has no identity set there — commits use the repo's existing identity
  via `git -c user.name=… -c user.email=…`.
- Cloud sessions have no GPU: they test with Mesa llvmpipe under Xvfb, so FPS
  numbers there are meaningless. Real-hardware results come from the
  director's Linux PC (first test: steady 60 FPS with VSync).
