# Benchmarks

## Reference hardware targets
- **Minimum:** ~2014 i5-4200U, Intel HD 4400/520, 4 GB RAM → 60 FPS @ 8 chunks, Low preset
- Mid: GTX 1660 / RX 580 → 60+ FPS @ 16 chunks
- High: RTX 3070+ → 144+ FPS @ 32+ chunks

## Phase 2 — test scene (2026-11-14)

**Environment:** cloud container, 4 CPU cores, **no GPU**. All rendering is done in
software by Mesa *llvmpipe* (on the CPU), so these numbers are a worst-case
sanity check, *not* representative of any real graphics card.

Scene: 41 276 triangles, 1 draw call, 1280×720, VSync off, autopilot movement,
release build, 12 s run (`--autopilot --exit-after 12`).

| Backend | FPS | Avg frame | Worst frame | Tick rate |
|---|---|---|---|---|
| Vulkan (llvmpipe) | 115 | 8.73 ms | 14.52 ms | 20 |
| OpenGL (llvmpipe) | 89 | 11.34 ms | 24.73 ms | 21* |

\* The overlay counts ticks in a sliding 1-second window, so it reads 20–21.

Observations:
- Even with *software* rendering, the frame budget for 60 FPS (16.7 ms) is met.
- Vulkan is ~30 % faster than OpenGL here, consistent with the Vulkan-first decision.
- Simulation holds 20 ticks/s under load.

**Pending:** real-GPU numbers from the director's machine. To measure:
`loredrop --exit-after 30` with `show_debug_overlay = true`; the log prints FPS,
average and worst frame time at exit.

## Phase 3 — chunk system (2026-11-14)

Same cloud container (4 cores, no GPU). Release build.

### Micro-benchmarks (`cargo bench`)
| Benchmark | Time |
|---|---|
| Mesh a typical surface section (hills, 3 block types) | **0.26 ms** |
| Mesh worst-case checkerboard section (12 288 quads) | 0.82 ms |
| Read all 4 096 blocks of a paletted section | 13 µs (3.2 ns/block) |
| Fill 4 096 blocks into a fresh section (palette growth) | 19 µs |

### World loading, headless (`loredrop --bench-world <distance>`, 3 worker threads)
| Render distance | Columns generated | Columns meshed | Sections | Quads | Time | World data | Mesh data |
|---|---|---|---|---|---|---|---|
| 4 | 121 | 81 | 107 | 25 777 | 0.11 s | 0.8 MB | 0.8 MB |
| 8 | 361 | 289 | 372 | 98 711 | **0.37 s** | 2.4 MB | 3.2 MB |
| 12 | 729 | 625 | 812 | 206 522 | 0.81 s | 4.8 MB | 6.6 MB |
| 16 | 1 225 | 1 089 | 1 431 | 347 948 | 1.37 s | 8.2 MB | 11.1 MB |

Throughput ≈ 900–1 100 columns/s. Memory is very low because test terrain is
mostly uniform stone/air sections; Phase 4 terrain (caves, ores) will raise it.

### In-game (software rendering, llvmpipe)
| Backend | Render distance | FPS | Avg / worst frame | Time until world ready |
|---|---|---|---|---|
| Vulkan | 8 | 31–41 | 25–32 ms / 31–42 ms | 9.1–9.5 s |
| OpenGL | 8 | 48 | 21 ms / 26 ms | 6.7 s |

⚠️ These FPS numbers measure a **CPU emulating a GPU**, which also steals CPU
time from world loading (0.37 s headless vs ~9 s in-game). They say nothing
about real graphics cards. ~62–74 k triangles, ~190–230 draw calls per frame.

**Pending:** real-GPU measurements on the director's hardware.

## Phase 4 — world generation (2026-11-14)

Same container (4 cores, no GPU), release build, seed 12345.

| Benchmark | Time |
|---|---|
| Generate one chunk column (all stages) | **1.30 ms** (10.3 ms before tree-candidate filtering) |
| Profile one block column (cross-chunk trees, spawn, maps) | 87 µs |
| Mesh a typical surface section (12-byte vertices, biome colours) | 0.28 ms |

### World loading, headless (`--bench-world`, 3 worker threads)
| Render distance | Columns | Sections meshed | Quads | Time | World data | Mesh data |
|---|---|---|---|---|---|---|
| 8 | 361 | 1 729 | 292 501 | **0.49 s** | 7.0 MB | 14.0 MB |
| 12 | 729 | 3 694 | 598 537 | 1.01 s | 14.1 MB | 28.7 MB |
| 16 | 1 225 | 6 701 | 1 119 676 | 1.78 s | 24.1 MB | 53.7 MB |

Real terrain has ~3× the geometry of Phase 3's test hills. Render distance 8
uses ~21 MB for world + meshes, far within the 4 GB minimum target.
In-game software rendering: ~24 FPS, ~186 k triangles, ~650 draw calls (CPU
emulating a GPU — not meaningful for real hardware; draw calls are a Phase 10 target).

## Phases 5–7

Measured on the **director's laptop** (Linux, 3 worker threads) on
2026-11-14, so not directly comparable with the cloud numbers above.

| Benchmark | Result |
|---|---|
| Creature AI, 67 creatures (≈50 zombies chasing + ≈20 Hollows moving blocks), flat test world, release | **0.086 ms per tick** (budget: 50 ms) |
| `--bench-world 8`, seed 12345 | 361 columns, 1 730 sections, 292 906 quads in 1.19 s (304 columns/s); world 7.0 MB, meshes 14.1 MB |

Creature cost is negligible. Re-run with `cargo test --release -p loredrop
creature_tick_cost -- --ignored --nocapture`. The test world is flat, so
pathfinding around real terrain will cost more, but there's plenty of headroom.

## Phase 8 — lighting (director's laptop, 2026-11-14)

| Benchmark | Before lighting | With lighting |
|---|---|---|
| Light for one real-terrain section (`--example light_bench`) | — | 2.0 ms first version → **0.87 ms** after optimisation |
| `--bench-world 8`, seed 12345 (3 threads) | 1.19 s | **1.87–1.90 s** (one light box per column run) |

Optimisations: decode each section once instead of per-block lookups;
sunlight entry computed per section rather than per block; cheaper seeding
of the flood fill; one light box shared by all dirty sections of a column
(≈2.4× fewer cells). Light changes after a block edit cost about one light
box per affected column (≈1–2 ms each, on worker threads).

## First real-hardware test (director's PC, 2026-11-14)

Linux, prebuilt release binary, default settings (render distance 8, VSync on):
**steady 60 FPS (VSync-capped)**. Mouse sensitivity confirmed correct.
Still to collect: GPU model, uncapped FPS (`vsync = false`), and higher render
distances.

### Phase 4 part 2 — structures
Generation with ruins: **1.49 ms per column** (+0.2 ms for structure lookups).

## NumWorks edition — NW phase 1 (2026-11-16, PC only)

No calculator was available, so these numbers are from the PC harness
(`ld_nw_desktop bench`) and from instruction counts (callgrind). They are
**estimates** until the director measures FPS on a real calculator (FPS is
shown in the status line).

| Measure | Value |
|---|---|
| Render, default settings (pixel size 2 = 16 000 rays, view 12) | ~11.7 million x86 instructions per frame (callgrind) |
| Host time per frame, default settings | 2.8 ms (looking up: 3.3 ms, looking down: 1.8 ms) |
| Host time per frame, pixel size 4 | 0.8 ms |
| Host time per frame, view 6 / 14 | 2.0 / 3.2 ms |
| App size | 242 KB file; 96 KB code + 68 KB initialised data (game state) |

After NW2 (survival, inventory, crafting): `loredrop.nwa` is 333 KB (155 KB
code + textures, 69 KB game state). The 3D view's cost is unchanged; the
hearts/food panel is only redrawn when what it shows changes.

After NW3 (creatures): `loredrop.nwa` is 449 KB (234 KB code + textures,
80 KB game state). Callgrind, 30 frames turning with five creatures 4.5
blocks away (worst case): terrain 14.3 M + creatures 8.6 M instructions per
frame (creatures were 23 M before per-box screen rectangles). With no
creature in view the cost is the same as before.

NW4: layer-major world storage — host time 2.95 → 2.49 ms/frame (seed 1,
same scene), ~3 % fewer instructions (the multiplications and bounds
checks are what went). Pixel size 3: 1.44 ms vs 2.85 (size 2) and 0.82
(size 4) on the host. Plants and torch light add a few percent where
present (seed 3 plains: 3.17 ms). *Auto* pixel size now picks 2–4 at run
time, and *Show timings* displays the real split on the calculator.

NW5 (max-speed pass), host ms/frame, pixel size 2, view 12, turning:

| Scene | NW4 | NW5 |
|---|---|---|
| Seed 1 | 2.86 | 2.19 |
| Seed 3 (plains with plants) | 3.17 | 2.31 |
| Five creatures 4.5 blocks away | 4.73 | 3.28 |

Callgrind (seed 1, 30 frames): render 388 M → 381 M instructions (the
ray-loop savings of −7 % are spent again by sky skipping, which trades
memory reads for arithmetic). Which matters more on the calculator (cache
misses on the 48 KB world vs raw instruction count) can only be told by the
*Show timings* figures from a real calculator.

### First real-calculator result (director, N0120, 2026-11-16)

Installs and runs well. *Show timings*: `3D63 G0 B0 P2 V12` — 63 ms per
frame drawing the 3D view (≈ 16 FPS), game logic and bottom bar under 1 ms,
automatic quality kept pixel size 2 and view 12. The 3D view is the whole
cost. 63 ms ≈ 35 M cycles at 550 MHz for ≈ 13 M (x86) instructions: about
2.7 cycles per instruction, so memory waits (code, textures and tables in
the external QSPI flash; the 48 KB world vs the CPU's data cache) probably
matter as much as the arithmetic.

NW6 (host, pixel 2, view 12): seed 1 2.19 → 1.87 ms, seed 3 2.31 → 1.77.
Five creatures close by: 3.3–3.6 ms (unchanged within run-to-run noise;
creatures aren't helped by the fill map). Callgrind: render 381 M → 399 M
instructions (the fill-map test adds instructions but removes most
block-array reads). Real-calculator split: Settings → *Run speed test*.

Rough guess made before the test for the N0120 (Cortex-M7, 550 MHz): 15–30 FPS at default
settings; older N0110 (216 MHz): roughly 2.5 times slower. Pixel size 4 is
about 3–4 times faster.
Seed 12345 within 4 km of spawn: 70 surface ruins, 8 desert halls, 38 sunken
towers, 141 deep vaults (underground).
