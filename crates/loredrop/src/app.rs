//! Window, event loop and per-frame orchestration.
//!
//! Each frame:
//! 1. measure time since the last frame,
//! 2. run as many fixed 20 Hz ticks as that time covers (at most 10, so a long
//!    stall can't trigger a "spiral of death"),
//! 3. apply mouse look,
//! 4. build the UI and render, interpolating between ticks.

use crate::Options;
use crate::frame_stats::FrameStats;
use crate::game::{Game, GameMode, TICKS_PER_SECOND};
use crate::icons;
use crate::overlay::{self, OverlayData};
use crate::world::World;
use ld_config::Settings;
use ld_input::{Action, Bindings, InputState};
use ld_render::{Camera, FrameParams, Renderer, UiFrame};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{CursorGrabMode, Fullscreen, Window, WindowId};

const TICK: f64 = 1.0 / TICKS_PER_SECOND;
const MAX_TICKS_PER_FRAME: u32 = 10;
/// Classic Minecraft daytime sky.
const SKY: [f32; 3] = [0.47, 0.65, 1.0];
/// Placeholder plains-like tints until biomes exist (Phase 4).
const GRASS_TINT: [f32; 3] = [0.57, 0.74, 0.35];
const FOLIAGE_TINT: [f32; 3] = [0.47, 0.67, 0.18];
/// Murky blue used for the sky/fog when the camera is under water.
const UNDERWATER: [f32; 3] = [0.08, 0.22, 0.45];

/// Loads game data and creates the (empty) world.
/// Everything loaded from the assets folder that the app needs at startup.
struct Loaded {
    world: World,
    textures: ld_assets::TextureSet,
    items: Arc<ld_game::ItemRegistry>,
    recipes: Arc<ld_game::RecipeBook>,
    drops: Arc<ld_game::mining::DropTable>,
    lore: Arc<ld_assets::Lore>,
    mobs: Arc<ld_game::mobs::MobRegistry>,
    icons: Vec<icons::Icon>,
    looks: Vec<crate::entities::ItemLook>,
    crack_layers: [u32; 10],
    /// Texture layer per name, for creature skins and other entity textures.
    layers: std::collections::HashMap<String, u32>,
}

fn load_game(settings: &Settings, seed: Option<u64>) -> Result<Loaded, Box<dyn std::error::Error>> {
    let (world, textures) = load_world(settings, seed)?;
    let assets = ld_assets::find_assets_dir().ok_or("assets folder not found")?;
    let items =
        Arc::new(ld_game::ItemRegistry::new(&world.registry, &ld_assets::read_data(&assets, "items.toml")?)?);
    let icons = icons::render_all(&items, &world.registry, &textures, &assets, [GRASS_TINT, FOLIAGE_TINT]);
    let recipes =
        Arc::new(ld_game::RecipeBook::from_toml(&ld_assets::read_data(&assets, "recipes.toml")?, &items)?);
    let drops = Arc::new(ld_game::mining::DropTable::new(&world.registry, &items)?);
    let lore = Arc::new(ld_assets::load_lore(&assets)?);
    let mobs = Arc::new(ld_game::mobs::MobRegistry::from_toml(
        &ld_assets::read_data(&assets, "mobs.toml")?,
        &items,
        &world.biomes,
    )?);
    let layers = textures.names.iter().enumerate().map(|(i, n)| (n.clone(), i as u32)).collect();
    let looks = item_looks(&items, &world.registry, &textures);
    let crack_layers = std::array::from_fn(|i| textures.layer_of(&format!("destroy_stage_{i}")) as u32);
    log::info!(
        "{} items, {} icons, {} recipes",
        items.len(),
        icons.len(),
        recipes.shaped.len() + recipes.shapeless.len() + recipes.smelting.len()
    );
    Ok(Loaded { world, textures, items, recipes, drops, lore, mobs, icons, looks, crack_layers, layers })
}

/// How each item looks when dropped in the world.
fn item_looks(
    items: &ld_game::ItemRegistry,
    blocks: &ld_world::BlockRegistry,
    tex: &ld_assets::TextureSet,
) -> Vec<crate::entities::ItemLook> {
    use crate::entities::ItemLook;
    let pack =
        |c: [f32; 3]| ((c[0] * 255.0) as u32) << 16 | ((c[1] * 255.0) as u32) << 8 | (c[2] * 255.0) as u32;
    let tint = |t: ld_world::Tint| match t {
        ld_world::Tint::Grass => pack(GRASS_TINT),
        ld_world::Tint::Foliage => pack(FOLIAGE_TINT),
        _ => 0xFF_FFFF,
    };
    items
        .iter()
        .map(|(_, d)| match d.kind {
            ld_game::ItemKind::Block(id) => {
                let b = blocks.get(id);
                if b.shape == ld_world::Shape::Cross {
                    ItemLook::Flat { layer: tex.layer_of(&b.textures[0]) as u32, tint: tint(b.tints[0]) }
                } else {
                    ItemLook::Cube {
                        layers: std::array::from_fn(|f| tex.layer_of(&b.textures[f]) as u32),
                        tints: std::array::from_fn(|f| tint(b.tints[f])),
                    }
                }
            }
            _ => {
                ItemLook::Flat { layer: tex.layer_of(&format!("item/{}", d.texture)) as u32, tint: 0xFF_FFFF }
            }
        })
        .collect()
}

/// Texture names for the shared texture array: every block texture, item
/// sprites (for dropped items) and the 10 block-breaking crack stages.
fn all_texture_names(blocks: &ld_world::BlockRegistry, assets: &std::path::Path) -> Vec<String> {
    let mut names = blocks.texture_names();
    names.extend((0..10).map(|i| format!("destroy_stage_{i}")));
    if let Ok(dir) = std::fs::read_dir(assets.join("textures").join("entity")) {
        let mut skins: Vec<String> = dir
            .flatten()
            .filter_map(|e| {
                e.file_name().to_str().and_then(|n| n.strip_suffix(".png")).map(|n| format!("entity/{n}"))
            })
            .collect();
        skins.sort();
        names.extend(skins);
    }
    if let Ok(dir) = std::fs::read_dir(assets.join("textures").join("item")) {
        let mut items: Vec<String> = dir
            .flatten()
            .filter_map(|e| {
                e.file_name().to_str().and_then(|n| n.strip_suffix(".png")).map(|n| format!("item/{n}"))
            })
            .collect();
        items.sort();
        names.extend(items);
    }
    names
}

fn load_world(
    settings: &Settings,
    seed: Option<u64>,
) -> Result<(World, ld_assets::TextureSet), Box<dyn std::error::Error>> {
    let assets = ld_assets::find_assets_dir()
        .ok_or("could not find the 'assets' folder (set LOREDROP_ASSETS to its location)")?;
    log::info!("assets: {}", assets.display());
    let registry = Arc::new(ld_assets::load_blocks(&assets)?);
    let textures = ld_assets::load_block_textures(&assets, &all_texture_names(&registry, &assets));
    log::info!(
        "loaded {} block types, {} textures ({}px)",
        registry.len(),
        textures.layers.len(),
        textures.size
    );
    let biomes = Arc::new(ld_assets::load_biomes(&assets)?);
    let ores = ld_assets::read_data(&assets, "ores.toml")?;
    let seed = seed.unwrap_or_else(|| {
        // No seed given: derive one from the clock, like starting a new world.
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        (t.as_nanos() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
    });
    log::info!("world seed: {}", seed as i64);
    let mesh_table = Arc::new(ld_mesh::MeshTable::new(&registry, Some(&biomes), |n| textures.layer_of(n)));
    let generator = Arc::new(ld_worldgen::Overworld::new(seed, &registry, biomes.clone(), &ores)?);
    let world =
        World::new(registry, biomes, seed, generator, mesh_table, settings.performance.worker_threads);
    Ok((world, textures))
}

/// `--bench-world`: generates and meshes a render distance with no window or
/// GPU, and prints timings. Used for BENCHMARKS.md.
pub fn bench_world(rd: u32, settings: &Settings, seed: Option<u64>) {
    struct Counter {
        sections: usize,
        quads: usize,
        bytes: usize,
    }
    impl crate::world::SectionSink for Counter {
        fn upload(&mut self, _: ld_core::coords::SectionPos, m: &ld_mesh::SectionMesh) {
            self.sections += 1;
            self.quads += m.quad_count();
            self.bytes += m.quad_count() * 4 * std::mem::size_of::<ld_mesh::PackedVertex>();
        }
        fn remove(&mut self, _: ld_core::coords::SectionPos) {}
    }
    let (mut world, _) = match load_world(settings, Some(seed.unwrap_or(12345))) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let (sx, _, sz) = world.generator().find_spawn();
    let center = ld_core::coords::BlockPos::new(sx, 0, sz).chunk();
    let mut sink = Counter { sections: 0, quads: 0, bytes: 0 };
    let start = Instant::now();
    while !world.is_settled(center) {
        world.update(center, rd, u32::MAX, &mut sink);
        std::thread::sleep(Duration::from_micros(100));
    }
    let t = start.elapsed().as_secs_f64();
    let s = world.stats(center);
    println!(
        "render distance {rd}: {} columns generated, {} columns meshed, {} sections ({} quads) in {t:.2} s \
         using {} threads → {:.1} columns/s; world data {:.1} MB, mesh data {:.1} MB",
        s.columns_loaded,
        s.columns_ready,
        sink.sections,
        sink.quads,
        s.threads,
        s.columns_loaded as f64 / t,
        s.world_bytes as f64 / 1e6,
        sink.bytes as f64 / 1e6
    );
}

/// `--map`: renders a top-down map (one pixel per 4 blocks) of the world
/// around spawn — biome colours shaded by height, water by depth. A quick way
/// to judge continents, biomes and rivers without flying around.
pub fn draw_map(blocks: u32, path: &std::path::Path, settings: &Settings, seed: Option<u64>) {
    let seed = seed.unwrap_or(12345);
    let fail = |e: String| -> ! {
        eprintln!("{e}");
        std::process::exit(1);
    };
    let assets = ld_assets::find_assets_dir().unwrap_or_else(|| fail("assets folder not found".into()));
    let registry = ld_assets::load_blocks(&assets).unwrap_or_else(|e| fail(e));
    let biomes = Arc::new(ld_assets::load_biomes(&assets).unwrap_or_else(|e| fail(e)));
    let ores = ld_assets::read_data(&assets, "ores.toml").unwrap_or_else(|e| fail(e));
    let g = ld_worldgen::Overworld::new(seed, &registry, biomes.clone(), &ores).unwrap_or_else(|e| fail(e));
    let _ = settings;
    let step = 4;
    let px = (blocks / step).max(16);
    let (sx, _, sz) = ld_worldgen::TerrainGenerator::find_spawn(&g);
    let start = Instant::now();
    // Rows are independent: generate them on all cores.
    let rows: Vec<Vec<u8>> = {
        use std::sync::Mutex;
        let out = Mutex::new(vec![Vec::new(); px as usize]);
        std::thread::scope(|scope| {
            let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
            for t in 0..threads {
                let (g, out) = (&g, &out);
                scope.spawn(move || {
                    for row in (t..px as usize).step_by(threads) {
                        let mut line = Vec::with_capacity(px as usize * 3);
                        for col in 0..px {
                            let x = sx + (col as i32 - px as i32 / 2) * step as i32;
                            let z = sz + (row as i32 - px as i32 / 2) * step as i32;
                            let p = g.column_profile(x, z);
                            let rgb = if p.water_above {
                                let depth = (ld_worldgen::SEA_LEVEL - p.top) as f32;
                                let k = (1.0 - depth / 50.0).clamp(0.25, 1.0);
                                [0.15 * k, 0.35 * k, 0.85 * k]
                            } else {
                                let b = g.biomes().get(p.biome);
                                let base = match b.name.as_str() {
                                    "desert" | "beach" => [0.86, 0.8, 0.55],
                                    "snowy_plains" => [0.95, 0.97, 1.0],
                                    "mountains" if p.top > 170 => [0.97, 0.97, 0.97],
                                    "mountains" if p.top > 140 => [0.55, 0.55, 0.55],
                                    _ => b.grass_color,
                                };
                                let shade = 0.65 + ((p.top - 63) as f32 / 160.0).clamp(0.0, 0.5);
                                base.map(|c| c * shade)
                            };
                            line.extend(rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8));
                        }
                        out.lock().unwrap()[row] = line;
                    }
                });
            }
        });
        out.into_inner().unwrap()
    };
    let mut img = image::RgbImage::new(px, px);
    for (y, line) in rows.iter().enumerate() {
        for x in 0..px as usize {
            img.put_pixel(x as u32, y as u32, image::Rgb([line[x * 3], line[x * 3 + 1], line[x * 3 + 2]]));
        }
    }
    // Mark ruins: red = surface ruin, orange = desert hall, cyan = sunken tower,
    // purple = deep vault. Also list the nearest of each kind to spawn.
    let half_blocks = (px as i32 / 2) * step as i32;
    let ruins = g.structures_in_area(sx - half_blocks, sz - half_blocks, sx + half_blocks, sz + half_blocks);
    for r in &ruins {
        let color = match r.kind {
            ld_worldgen::StructureKind::SurfaceRuin => [230, 40, 40],
            ld_worldgen::StructureKind::DesertHall => [255, 150, 0],
            ld_worldgen::StructureKind::SunkenTower => [0, 230, 230],
            ld_worldgen::StructureKind::DeepVault => [170, 60, 230],
        };
        let (cx, cy) = ((r.x - sx + half_blocks) / step as i32, (r.z - sz + half_blocks) / step as i32);
        for dy in -2..=2 {
            for dx in -2..=2 {
                let (x, y) = (cx + dx, cy + dy);
                if (0..px as i32).contains(&x) && (0..px as i32).contains(&y) {
                    img.put_pixel(x as u32, y as u32, image::Rgb(color));
                }
            }
        }
    }
    for kind in ld_worldgen::StructureKind::ALL {
        let nearest =
            ruins.iter().filter(|r| r.kind == kind).min_by_key(|r| (r.x - sx).pow(2) + (r.z - sz).pow(2));
        match nearest {
            Some(r) => println!(
                "nearest {:<13} at {} {} {} ({} found on map)",
                kind.name(),
                r.x,
                r.y,
                r.z,
                ruins.iter().filter(|q| q.kind == kind).count()
            ),
            None => println!("no {} on this map", kind.name()),
        }
    }
    // Mark spawn with a small red cross.
    let c = px / 2;
    for d in 0..5u32 {
        for (x, y) in [(c - 2 + d, c), (c, c - 2 + d)] {
            img.put_pixel(x, y, image::Rgb([255, 0, 0]));
        }
    }
    match img.save(path) {
        Ok(()) => println!(
            "map of {blocks}×{blocks} blocks (seed {}) around spawn ({sx}, {sz}) saved to {} in {:.1} s",
            seed as i64,
            path.display(),
            start.elapsed().as_secs_f64()
        ),
        Err(e) => fail(format!("could not save map: {e}")),
    }
}

pub fn run(options: Options, settings: Settings) -> Result<(), Box<dyn std::error::Error>> {
    // Load game data before opening a window, so data errors are reported clearly.
    let loaded = load_game(&settings, options.seed)?;

    let event_loop = EventLoop::new()?;
    let mut app = App::new(options, settings, loaded);
    event_loop.run_app(&mut app)?;
    if let Some(e) = app.fatal.take() {
        return Err(e.into());
    }
    Ok(())
}

/// Things that only exist once the window is open.
struct Running {
    window: Arc<Window>,
    renderer: Renderer,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    /// Item icons as egui textures, indexed by item ID.
    icon_textures: Vec<egui::TextureHandle>,
    stat_icons: crate::hud::StatIcons,
}

struct App {
    options: Options,
    settings: Settings,
    bindings: Bindings,
    input: InputState,
    game: Game,
    stats: FrameStats,
    running: Option<Running>,
    last_frame: Instant,
    started: Instant,
    accumulator: f64,
    frames: u64,
    show_debug: bool,
    mouse_captured: bool,
    screenshot_requested: bool,
    fatal: Option<String>,
    textures: Option<ld_assets::TextureSet>,
    egui_has_keyboard: bool,
    /// A menu asked to close during drawing; handled at the start of the next frame.
    pending_close: bool,
    icons: Vec<icons::Icon>,
    looks: Vec<crate::entities::ItemLook>,
    crack_layers: [u32; 10],
    layers: std::collections::HashMap<String, u32>,
    /// Reused every frame for entity triangles.
    entity_vertices: Vec<ld_render::EntityVertex>,
    world_ready_logged: bool,
    screen: crate::screens::Screen,
    menu: crate::screens::MenuState,
}

impl App {
    fn new(options: Options, settings: Settings, loaded: Loaded) -> Self {
        let Loaded { world, textures, items, recipes, drops, lore, mobs, icons, looks, crack_layers, layers } =
            loaded;
        Self {
            bindings: settings.keybinds.clone(),
            show_debug: settings.interface.show_debug_overlay,
            icons,
            looks,
            crack_layers,
            layers,
            entity_vertices: Vec::new(),
            screen: crate::screens::Screen::None,
            egui_has_keyboard: false,
            pending_close: false,
            menu: crate::screens::MenuState::default(),
            game: {
                let mut g = Game::new(world, items, recipes, drops, options.autopilot);
                g.lore = lore;
                g.mobs = crate::mobs::Mobs::new(mobs);
                g.pending_summons = options.summon.clone();
                g.set_start_mode(match settings.gameplay.game_mode {
                    ld_config::GameModeSetting::Survival => GameMode::Survival,
                    ld_config::GameModeSetting::Creative => GameMode::Creative,
                });
                g.apply_dev_start(options.start_pos, options.start_look, options.fly);
                g
            },
            textures: Some(textures),
            world_ready_logged: false,
            options,
            settings,
            input: InputState::new(),
            stats: FrameStats::new(),
            running: None,
            last_frame: Instant::now(),
            started: Instant::now(),
            accumulator: 0.0,
            frames: 0,
            mouse_captured: false,
            screenshot_requested: false,
            fatal: None,
        }
    }

    fn set_mouse_captured(&mut self, captured: bool) {
        let Some(r) = &self.running else { return };
        if captured {
            // Locked is ideal (cursor stays put); some platforms only support Confined.
            let ok = r
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| r.window.set_cursor_grab(CursorGrabMode::Confined))
                .is_ok();
            if !ok {
                log::warn!("could not capture the mouse on this platform");
            }
        } else {
            let _ = r.window.set_cursor_grab(CursorGrabMode::None);
        }
        r.window.set_cursor_visible(!captured);
        self.mouse_captured = captured;
        self.input.take_mouse_delta(); // discard movement from before the change
    }

    fn open_screen(&mut self, screen: crate::screens::Screen) {
        self.screen = screen;
        self.game.input_blocked = true;
        self.set_mouse_captured(false);
        self.input.release_all();
    }

    fn close_screen(&mut self) {
        let creative = self.game.mode == GameMode::Creative;
        crate::screens::return_cursor(&mut self.menu, &mut self.game.inventory, &self.game.items, creative);
        crate::screens::return_grid(&mut self.menu, &mut self.game.inventory, &self.game.items);
        self.screen = crate::screens::Screen::None;
        self.game.input_blocked = false;
        self.input.release_all(); // don't let menu clicks leak into the world
        self.set_mouse_captured(true);
    }

    fn toggle_fullscreen(&mut self) {
        if let Some(r) = &self.running {
            let fs = r.window.fullscreen().is_none();
            r.window.set_fullscreen(fs.then_some(Fullscreen::Borderless(None)));
        }
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = now - self.last_frame;
        self.last_frame = now;
        self.stats.record_frame(now, dt);

        // Settings that apply live.
        self.game.difficulty = match self.settings.gameplay.difficulty {
            ld_config::Difficulty::Peaceful => ld_game::stats::Difficulty::Peaceful,
            ld_config::Difficulty::Easy => ld_game::stats::Difficulty::Easy,
            ld_config::Difficulty::Normal => ld_game::stats::Difficulty::Normal,
            ld_config::Difficulty::Hard => ld_game::stats::Difficulty::Hard,
        };

        // --- fixed-rate simulation ---
        self.accumulator += dt.as_secs_f64().min(0.5);
        let mut ticks = 0;
        while self.accumulator >= TICK && ticks < MAX_TICKS_PER_FRAME {
            self.game.tick(&mut self.input, &self.settings);
            self.stats.record_tick(now);
            self.accumulator -= TICK;
            ticks += 1;
        }
        if ticks == MAX_TICKS_PER_FRAME {
            self.accumulator = 0.0; // we're too far behind: drop the backlog
        }
        let alpha = self.accumulator / TICK;

        // --- per-frame input ---
        let (dx, dy) = self.input.take_mouse_delta();
        if self.mouse_captured {
            self.game.look(dx, dy, &self.settings);
        }
        if self.input.take_press(Action::ToggleDebugOverlay) {
            self.show_debug = !self.show_debug;
        }
        if self.input.take_press(Action::ToggleFullscreen) {
            self.toggle_fullscreen();
        }
        if self.input.take_press(Action::Screenshot) {
            self.screenshot_requested = true;
        }
        let dead_screen = matches!(self.screen, crate::screens::Screen::Death { .. });
        if self.input.take_press(Action::Inventory) && !dead_screen {
            if self.screen == crate::screens::Screen::None {
                self.open_screen(crate::screens::Screen::Inventory);
            } else {
                self.close_screen();
            }
        }
        if self.input.take_press(Action::Journal) && !dead_screen {
            if self.screen == crate::screens::Screen::Journal {
                self.close_screen();
            } else if self.screen == crate::screens::Screen::None {
                self.open_screen(crate::screens::Screen::Journal);
            }
        }
        if self.input.take_press(Action::SwitchGameMode) {
            self.game.mode = match self.game.mode {
                GameMode::Creative => GameMode::Survival,
                GameMode::Survival => GameMode::Creative,
            };
            if self.game.mode == GameMode::Survival {
                self.game.player.flying = false;
            }
            log::info!("game mode: {:?}", self.game.mode);
        }
        self.frames += 1;
        let auto_shot = self.options.screenshot_after == Some(self.frames);

        if std::mem::take(&mut self.pending_close) {
            if matches!(self.screen, crate::screens::Screen::Death { .. }) {
                self.game.respawn(); // the death screen's only button is Respawn
            }
            self.close_screen();
        }
        // Death: spill the inventory once and show the death screen.
        if self.game.stats.is_dead() && !matches!(self.screen, crate::screens::Screen::Death { .. }) {
            let message = self.game.stats.death.map(|d| d.death_message()).unwrap_or("You died").to_string();
            self.menu = crate::screens::MenuState::default(); // anything on the cursor is lost with the rest
            self.screen = crate::screens::Screen::None;
            self.game.on_death();
            self.open_screen(crate::screens::Screen::Death { message });
        }
        // The game asked for a menu (e.g. right-clicked a crafting table).
        if let Some(screen) = self.game.open_request.take()
            && self.screen == crate::screens::Screen::None
        {
            self.open_screen(screen);
        }

        // --- world streaming ---
        let Some(r) = &mut self.running else { return };
        let center = self.game.player_chunk();
        self.game.world.update(
            center,
            self.settings.graphics.render_distance,
            self.settings.performance.chunk_uploads_per_frame,
            &mut r.renderer,
        );
        if !self.world_ready_logged && self.game.world.is_settled(center) {
            self.world_ready_logged = true;
            log::info!(
                "world ready: {} columns meshed in {:.2} s (render distance {})",
                self.game.world.stats(center).columns_ready,
                self.started.elapsed().as_secs_f64(),
                self.settings.graphics.render_distance
            );
        }

        // --- UI + render ---
        // The inventory is lent to the menus for this frame (and put back
        // below), so the overlay can read the rest of the game at the same time.
        let mut inv = std::mem::take(&mut self.game.inventory);
        // Likewise the open furnace (if any).
        let furnace_pos = match &self.screen {
            crate::screens::Screen::Furnace(p) => Some(*p),
            _ => None,
        };
        let mut furnace = furnace_pos.and_then(|p| self.game.furnaces.remove(&p));
        let size = r.window.inner_size();
        r.egui_ctx.set_zoom_factor(self.settings.interface.gui_scale);
        let raw = r.egui_state.take_egui_input(&r.window);
        let overlay_data = OverlayData {
            stats: self.stats.summary(),
            game: &self.game,
            gpu: r.renderer.info(),
            render: r.renderer.stats(),
            resolution: (size.width, size.height),
            vsync: self.settings.graphics.vsync,
            fps_limit: self.settings.graphics.fps_limit,
            mouse_captured: self.mouse_captured,
            world: self.game.world.stats(center),
            render_distance: self.settings.graphics.render_distance,
            menu_open: self.screen != crate::screens::Screen::None,
        };
        let show_debug = self.show_debug;
        let creative = self.game.mode == GameMode::Creative;
        let menu_open = self.screen != crate::screens::Screen::None;
        let strength = self.game.attack_strength();
        let stat_icons = &r.stat_icons;
        let bars = (!creative).then(|| crate::hud::Bars {
            health: self.game.stats.health,
            food: self.game.stats.food,
            armor: self.game.armor_points(),
            air: (self.game.stats.air < ld_game::stats::MAX_AIR).then_some(self.game.stats.air),
            hurt_flash: self.game.stats.hurt_flash,
            eating: self.game.eating as f32 / 32.0,
        });
        let (inv, items, icons, recipes) = (&mut inv, &self.game.items, &r.icon_textures, &self.game.recipes);
        let (lore, journal) = (&self.game.lore, &self.game.journal);
        let (screen, menu) = (&self.screen, &mut self.menu);

        let mut close_requested = false;
        let out = r.egui_ctx.run_ui(raw, |ui| {
            overlay::draw(ui.ctx(), show_debug, &overlay_data);
            crate::hud::draw_hotbar(ui.ctx(), inv, icons, items);
            if let Some(b) = &bars {
                crate::hud::draw_bars(ui.ctx(), stat_icons, b);
            }
            if !menu_open {
                crate::hud::draw_attack_indicator(ui.ctx(), strength);
            }
            let mut m = crate::screens::MenuCtx {
                inventory: inv,
                items,
                icons,
                creative,
                state: menu,
                recipes,
                furnace: furnace.as_mut(),
                lore,
                journal,
            };
            close_requested = crate::screens::draw(ui.ctx(), screen, &mut m);
        });
        let _ = overlay_data; // end of the overlay's borrow of the game
        if let (Some(p), Some(f)) = (furnace_pos, furnace) {
            self.game.furnaces.insert(p, f);
        }
        self.game.inventory = std::mem::take(inv);
        r.egui_state.handle_platform_output(&r.window, out.platform_output);
        let wants_keyboard = r.egui_ctx.egui_wants_keyboard_input();
        self.egui_has_keyboard = wants_keyboard;
        let ui = UiFrame {
            primitives: r.egui_ctx.tessellate(out.shapes, out.pixels_per_point),
            textures_delta: out.textures_delta,
            pixels_per_point: out.pixels_per_point,
        };

        let eye = self.game.eye_position(alpha);
        // Under water: short blue fog instead of the sky (like Minecraft).
        let underwater = self.game.world.map.block(ld_core::coords::BlockPos::new(
            eye.x.floor() as i32,
            eye.y.floor() as i32,
            eye.z.floor() as i32,
        )) == self.game.world.registry.id("water");
        let params = FrameParams {
            camera: Camera {
                position: eye,
                yaw_deg: self.game.yaw,
                pitch_deg: self.game.pitch,
                fov_deg: self.settings.graphics.fov,
            },
            sky_color: if underwater { UNDERWATER } else { SKY },
            fog: if underwater { Some((2.0, 28.0)) } else { None }.or(self.settings.graphics.fog.then(
                || {
                    // Fade out over the last stretch before the loaded edge.
                    let end = self.settings.graphics.render_distance as f32 * 16.0 - 4.0;
                    (end * 0.7, end)
                },
            )),
            grass_tint: GRASS_TINT,
            foliage_tint: FOLIAGE_TINT,
        };
        // Dropped items and the cracks on the block being mined.
        self.entity_vertices.clear();
        let time = self.started.elapsed().as_secs_f32();
        self.game.entities.build_vertices(eye, time, &self.looks, &mut self.entity_vertices);
        let layers = &self.layers;
        let layer_of = |name: &str| layers.get(&format!("entity/{name}")).copied().unwrap_or(0);
        let arrow_layer = layers.get("item/arrow").copied().unwrap_or(0);
        self.game.mobs.build_vertices(eye, alpha, &layer_of, arrow_layer, &mut self.entity_vertices);
        if let Some((pos, progress)) = self.game.breaking {
            let stage = ((progress * 10.0) as usize).min(9);
            let min = glam::DVec3::new(pos.x as f64, pos.y as f64, pos.z as f64);
            crate::entities::crack_vertices(&mut self.entity_vertices, min, eye, self.crack_layers[stage]);
        }
        r.renderer.set_entities(&self.entity_vertices);
        r.renderer.set_outline(self.game.target.map(|h| {
            let min = glam::DVec3::new(h.pos.x as f64, h.pos.y as f64, h.pos.z as f64);
            (min, min + glam::DVec3::ONE)
        }));
        let want_shot = self.screenshot_requested || auto_shot;
        self.pending_close |= close_requested;
        self.screenshot_requested = false;
        match r.renderer.render(&params, Some(ui), want_shot) {
            Ok(Some(shot)) => {
                let path = if auto_shot { self.options.screenshot_path.clone() } else { screenshot_path() };
                save_png(&path, &shot);
            }
            Ok(None) => {}
            Err(e) => {
                self.fatal = Some(e.to_string());
                el.exit();
            }
        }

        if auto_shot {
            el.exit();
        }
        if let Some(limit) = self.options.exit_after
            && self.started.elapsed().as_secs_f64() >= limit
        {
            let s = self.stats.summary();
            log::info!(
                "exit-after reached: {} fps, avg {:.2} ms, worst {:.2} ms, {} tps, player at {:.2?}, yaw {:.2} pitch {:.2}",
                s.fps,
                s.avg_ms,
                s.worst_ms,
                s.tps,
                self.game.player.pos,
                self.game.yaw,
                self.game.pitch
            );
            el.exit();
        }
    }
}

/// `screenshots/screenshot-<unix seconds>-<ms>.png`
fn screenshot_path() -> std::path::PathBuf {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    std::path::PathBuf::from(format!("screenshots/screenshot-{}-{:03}.png", t.as_secs(), t.subsec_millis()))
}

fn save_png(path: &std::path::Path, shot: &ld_render::Screenshot) {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        let _ = std::fs::create_dir_all(dir);
    }
    match image::save_buffer(path, &shot.rgba, shot.width, shot.height, image::ColorType::Rgba8) {
        Ok(()) => log::info!("screenshot saved to {}", path.display()),
        Err(e) => log::warn!("could not save screenshot {}: {e}", path.display()),
    }
}

/// Name for a mouse button in key-binding strings.
fn mouse_button_name(b: MouseButton) -> String {
    match b {
        MouseButton::Left => "MouseLeft".into(),
        MouseButton::Right => "MouseRight".into(),
        MouseButton::Middle => "MouseMiddle".into(),
        MouseButton::Back => "MouseBack".into(),
        MouseButton::Forward => "MouseForward".into(),
        MouseButton::Other(n) => format!("Mouse{n}"),
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Loredrop")
            .with_inner_size(LogicalSize::new(1280.0, 720.0))
            .with_fullscreen(self.settings.graphics.fullscreen.then_some(Fullscreen::Borderless(None)));
        let window = match el.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.fatal = Some(format!("could not create window: {e}"));
                el.exit();
                return;
            }
        };
        let size = window.inner_size();
        let mut renderer = match Renderer::new(
            window.clone(),
            (size.width, size.height),
            self.settings.graphics.backend,
            self.settings.graphics.vsync,
        ) {
            Ok(r) => r,
            Err(e) => {
                self.fatal = Some(e.to_string());
                el.exit();
                return;
            }
        };
        if let Some(t) = self.textures.take() {
            renderer.set_block_textures(t.size, &t.layers, self.settings.graphics.mipmap_levels);
        }

        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(renderer.max_texture_side()),
        );
        let icon_textures = self
            .icons
            .iter()
            .enumerate()
            .map(|(i, px)| {
                let img = egui::ColorImage::from_rgba_unmultiplied([icons::SIZE, icons::SIZE], px);
                // Nearest filtering keeps pixel-art icons crisp.
                egui_ctx.load_texture(format!("icon{i}"), img, egui::TextureOptions::NEAREST)
            })
            .collect();
        let assets = ld_assets::find_assets_dir().unwrap_or_else(|| std::path::PathBuf::from("assets"));
        let stat_icons = crate::hud::StatIcons::load(&egui_ctx, &assets);
        self.running = Some(Running { window, renderer, egui_ctx, egui_state, icon_textures, stat_icons });
        self.last_frame = Instant::now();
        self.started = Instant::now();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(r) = &mut self.running {
            let _ = r.egui_state.on_window_event(&r.window, &event);
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.running {
                    r.renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::Focused(false) => {
                // Prevent keys getting "stuck" when alt-tabbing away.
                self.input.release_all();
                self.set_mouse_captured(false);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else { return };
                let name = format!("{code:?}");
                let pressed = event.state == ElementState::Pressed;
                if name == "Escape" && pressed {
                    if matches!(self.screen, crate::screens::Screen::Death { .. }) {
                        return; // only Respawn leaves the death screen
                    }
                    if self.screen != crate::screens::Screen::None {
                        self.close_screen();
                    } else {
                        self.set_mouse_captured(false);
                    }
                    return;
                }
                if self.egui_has_keyboard && pressed {
                    return; // typing in a search box: not a game key
                }
                self.input.handle_input(&self.bindings, &name, pressed);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                if self.screen != crate::screens::Screen::None {
                    return; // clicks belong to the menu (handled by egui)
                }
                if pressed && !self.mouse_captured && button == MouseButton::Left {
                    self.set_mouse_captured(true);
                    return; // the capturing click is not a game action
                }
                self.input.handle_input(&self.bindings, &mouse_button_name(button), pressed);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.mouse_captured {
                    let notches = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => y as f64,
                        winit::event::MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
                    };
                    log::trace!("mouse wheel {delta:?} -> {notches} notches");
                    self.input.add_scroll(notches);
                }
            }
            WindowEvent::RedrawRequested => self.frame(el),
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        // Raw mouse motion: unaffected by OS pointer acceleration or screen edges.
        if let DeviceEvent::MouseMotion { delta } = event
            && self.mouse_captured
        {
            log::trace!("raw mouse motion {delta:?} from {_id:?}");
            self.input.add_mouse_delta(delta.0, delta.1);
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(r) = &self.running else { return };
        let limit = self.settings.graphics.fps_limit;
        if limit == 0 {
            r.window.request_redraw();
            el.set_control_flow(ControlFlow::Poll);
        } else {
            let next = self.last_frame + Duration::from_secs_f64(1.0 / limit as f64);
            if Instant::now() >= next {
                r.window.request_redraw();
            }
            el.set_control_flow(ControlFlow::WaitUntil(next));
        }
    }
}
