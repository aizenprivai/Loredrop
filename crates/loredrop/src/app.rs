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
use crate::game::{Game, TICKS_PER_SECOND};
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

/// Loads game data and creates the (empty) world.
fn load_world(
    settings: &Settings,
    seed: Option<u64>,
) -> Result<(World, ld_assets::TextureSet), Box<dyn std::error::Error>> {
    let assets = ld_assets::find_assets_dir()
        .ok_or("could not find the 'assets' folder (set LOREDROP_ASSETS to its location)")?;
    log::info!("assets: {}", assets.display());
    let registry = Arc::new(ld_assets::load_blocks(&assets)?);
    let textures = ld_assets::load_block_textures(&assets, &registry.texture_names());
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
    let (world, textures) = load_world(&settings, options.seed)?;

    let event_loop = EventLoop::new()?;
    let mut app = App::new(options, settings, world, textures);
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
    world_ready_logged: bool,
}

impl App {
    fn new(options: Options, settings: Settings, world: World, textures: ld_assets::TextureSet) -> Self {
        Self {
            bindings: settings.keybinds.clone(),
            show_debug: settings.interface.show_debug_overlay,
            game: {
                let mut g = Game::new(world, options.autopilot);
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
        self.frames += 1;
        let auto_shot = self.options.screenshot_after == Some(self.frames);

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
        };
        let show_debug = self.show_debug;
        let out = r.egui_ctx.run_ui(raw, |ui| overlay::draw(ui.ctx(), show_debug, &overlay_data));
        r.egui_state.handle_platform_output(&r.window, out.platform_output);
        let ui = UiFrame {
            primitives: r.egui_ctx.tessellate(out.shapes, out.pixels_per_point),
            textures_delta: out.textures_delta,
            pixels_per_point: out.pixels_per_point,
        };

        let params = FrameParams {
            camera: Camera {
                position: self.game.eye_position(alpha),
                yaw_deg: self.game.yaw,
                pitch_deg: self.game.pitch,
                fov_deg: self.settings.graphics.fov,
            },
            sky_color: SKY,
            fog: self.settings.graphics.fog.then(|| {
                // Fade out over the last stretch before the loaded edge.
                let end = self.settings.graphics.render_distance as f32 * 16.0 - 4.0;
                (end * 0.7, end)
            }),
            grass_tint: GRASS_TINT,
            foliage_tint: FOLIAGE_TINT,
        };
        let want_shot = self.screenshot_requested || auto_shot;
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
        self.running = Some(Running { window, renderer, egui_ctx, egui_state });
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
                    self.set_mouse_captured(false);
                    return;
                }
                self.input.handle_input(&self.bindings, &name, pressed);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                if pressed && !self.mouse_captured && button == MouseButton::Left {
                    self.set_mouse_captured(true);
                    return; // the capturing click is not a game action
                }
                self.input.handle_input(&self.bindings, &mouse_button_name(button), pressed);
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
