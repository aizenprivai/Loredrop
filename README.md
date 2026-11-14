# Loredrop

A voxel sandbox game written in Rust: the feel of modern Minecraft, with its own
world, art and lore. See `GDD.md` for the design and `ROADMAP.md` for progress.

**Current state: Phase 4 (world generation, in progress)** — endless seeded
worlds with continents, oceans, rivers, 11 biomes, snow-capped mountains,
caves, ores, deepslate, trees and plants, streamed on background threads and
rendered Vulkan-first. F3 shows performance, chunk, biome and seed info. No
structures, block editing, survival or lighting yet.

## Build & run

Requires Rust 1.90+ (`rustup`).

```bash
cargo run --release
```

On Linux you also need the usual desktop libraries (normally pre-installed):
Vulkan or OpenGL drivers, `libxkbcommon-x11`, `libxcursor`, `libxi`, `libxrandr`
(X11) or Wayland.

The first run creates `settings.toml` in the current folder; every option in it is
explained by a comment. Run `loredrop --help` for command-line options.

## Controls (defaults, rebindable in `settings.toml`)

| Key | Action |
|---|---|
| Click | capture mouse (Esc releases) |
| W A S D | move |
| Space | jump · double-tap: start/stop flying · hold while flying: rise |
| Left Shift | sneak · while flying: descend |
| Left Ctrl | sprint |
| F3 | debug & performance overlay |
| F2 | screenshot (saved to `screenshots/`) |
| F11 | fullscreen |

## Development

```bash
cargo test --workspace      # all unit/integration tests (no GPU needed)
cargo bench -p ld_mesh      # meshing benchmarks (also: -p ld_world)
cargo run --release -- --bench-world 8   # headless world-loading benchmark
cargo run --release -- --map 4096 map.png --seed 42   # top-down world map
cargo run --release -- --seed "my world"  # play a specific seed
cargo clippy --workspace --all-targets
cargo doc --no-deps --open  # browsable docs from code comments
```

Project documents: `GDD.md`, `ROADMAP.md`, `ARCHITECTURE.md`, `CHANGELOG.md`,
`TODO.md`, `KNOWN_BUGS.md`, `BENCHMARKS.md`.

## Troubleshooting

- **Camera freezes while walking/jumping (laptop touchpad):** your desktop's
  "disable touchpad while typing" feature is pausing the touchpad whenever a
  key is held. Turn it off: GNOME → Settings → Mouse & Touchpad →
  "Disable Touchpad While Typing"; KDE → Touchpad → "Disable while typing".
- **FPS stuck at 60 (or your monitor's rate):** that's VSync. Set
  `vsync = false` in `settings.toml` to see uncapped FPS.
- **Error mentioning `GLIBC_2.39`:** the prebuilt Linux binary needs a recent
  distro; build from source with `cargo run --release` instead.
