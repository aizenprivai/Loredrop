# Loredrop

A voxel sandbox game written in Rust: the feel of modern Minecraft, with its own
world, art and lore. See `GDD.md` for the design and `ROADMAP.md` for progress.

**Current state: Phase 3 (voxel engine & chunk system)** — an endless,
streamed voxel world (temporary hills-and-trees terrain) built from compressed
chunks, meshed on background threads with greedy meshing and ambient
occlusion, rendered with Vulkan-first wgpu. Walk, sprint and fly with
Minecraft-faithful physics; F3 shows performance and chunk statistics. No real
world generation (biomes, caves, water) or block editing yet.

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
cargo clippy --workspace --all-targets
cargo doc --no-deps --open  # browsable docs from code comments
```

Project documents: `GDD.md`, `ROADMAP.md`, `ARCHITECTURE.md`, `CHANGELOG.md`,
`TODO.md`, `KNOWN_BUGS.md`, `BENCHMARKS.md`.
