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
