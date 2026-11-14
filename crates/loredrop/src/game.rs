//! The game simulation, advanced at a fixed 20 ticks per second.
//!
//! Rendering may run at any framerate; it draws the player *between* the last
//! two tick positions (interpolation) so motion stays smooth at 144 FPS even
//! though physics only updates 20 times a second — exactly like Minecraft.

use crate::world::World;
use glam::DVec3;
use ld_config::Settings;
use ld_input::{Action, InputState};
use ld_physics::player::{MoveInput, PlayerBody};

/// Simulation rate. Do not change: game balance (speeds, timings) assumes 20.
pub const TICKS_PER_SECOND: f64 = 20.0;
/// Two jump presses within this many ticks count as a double-tap (fly toggle).
const DOUBLE_TAP_TICKS: u64 = 7;

/// Game modes (GDD §2). Survival rules (mining time, drops) arrive later in
/// Phase 5; health and hunger in Phase 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMode {
    Creative,
    Survival,
}

/// Block IDs the game logic needs often.
pub struct GameIds {
    pub water: Option<ld_world::BlockId>,
    pub lava: Option<ld_world::BlockId>,
}

pub struct Game {
    pub world: World,
    spawn: DVec3,
    /// Set once the spawn column has loaded and the player was placed safely.
    placed: bool,
    pub player: PlayerBody,
    pub mode: GameMode,
    /// Look direction in degrees (Minecraft convention, see ld_render::Camera).
    pub yaw: f32,
    pub pitch: f32,
    /// Ticks simulated since start.
    pub tick: u64,
    /// Player position and eye height at the previous tick (for interpolation).
    prev_pos: DVec3,
    prev_eye: f64,
    pub(crate) eye: f64,
    last_jump_press: Option<u64>,
    sprint_toggled: bool,
    sneak_toggled: bool,
    /// Test mode: walk forward and jump on its own.
    pub autopilot: bool,
    pub items: std::sync::Arc<ld_game::ItemRegistry>,
    pub inventory: ld_game::Inventory,
    /// The block under the crosshair, if within reach.
    pub target: Option<ld_physics::RayHit>,
    pub(crate) attack_cooldown: u32,
    pub(crate) use_cooldown: u32,
    pub(crate) ids: GameIds,
    /// Set by the app while a menu is open: the world ignores game input.
    pub input_blocked: bool,
}

impl Game {
    pub fn new(world: World, items: std::sync::Arc<ld_game::ItemRegistry>, autopilot: bool) -> Self {
        let ids = GameIds { water: world.registry.id("water"), lava: world.registry.id("lava") };
        // A starter hotbar of building blocks for Creative mode.
        let mut inventory = ld_game::Inventory::default();
        for (i, name) in [
            "grass_block",
            "dirt",
            "stone",
            "cobblestone",
            "oak_planks",
            "oak_log",
            "glass",
            "stone_bricks",
            "sand",
        ]
        .iter()
        .enumerate()
        {
            if let Some(item) = items.id(name) {
                inventory.slots[i] = Some(ld_game::ItemStack::new(item, 64));
            }
        }
        // Spawn on dry land near the world origin (the generator decides).
        let (sx, sy, sz) = world.generator().find_spawn();
        let spawn = DVec3::new(sx as f64 + 0.5, sy as f64, sz as f64 + 0.5);
        let player = PlayerBody::new(spawn);
        let eye = player.eye_height();
        Self {
            prev_pos: player.pos,
            prev_eye: eye,
            eye,
            world,
            spawn,
            placed: false,
            player,
            mode: GameMode::Creative,
            yaw: 180.0, // face north (−Z)
            pitch: 10.0,
            tick: 0,
            last_jump_press: None,
            sprint_toggled: false,
            sneak_toggled: false,
            autopilot,
            items,
            inventory,
            target: None,
            attack_cooldown: 0,
            use_cooldown: 0,
            ids,
            input_blocked: false,
        }
    }

    /// Advances the world by one tick.
    pub fn tick(&mut self, input: &mut InputState, settings: &Settings) {
        self.prev_pos = self.player.pos;
        self.prev_eye = self.eye;
        self.tick += 1;

        // Don't simulate the player until the ground under them exists.
        if !self.world.is_loaded(self.player_chunk()) {
            return;
        }
        if !self.placed {
            self.place_safely();
        }

        // Double-tap jump toggles flight in Creative (Minecraft behaviour).
        while input.take_press(Action::Jump) {
            if self.mode == GameMode::Creative {
                match self.last_jump_press {
                    Some(t) if self.tick - t <= DOUBLE_TAP_TICKS => {
                        self.player.flying = !self.player.flying;
                        log::debug!("flying toggled: {}", self.player.flying);
                        self.last_jump_press = None;
                    }
                    _ => self.last_jump_press = Some(self.tick),
                }
            }
        }

        let axis = |pos: Action, neg: Action| (input.is_held(pos) as i32 - input.is_held(neg) as i32) as f64;
        let mut mv = MoveInput {
            forward: axis(Action::MoveForward, Action::MoveBackward),
            strafe: axis(Action::StrafeLeft, Action::StrafeRight),
            jump: input.is_held(Action::Jump),
            sneak: input.is_held(Action::Sneak),
            sprint: input.is_held(Action::Sprint),
        };

        // Toggle modes: a press flips the state instead of holding.
        if settings.controls.toggle_sprint {
            if input.take_press(Action::Sprint) {
                self.sprint_toggled = !self.sprint_toggled;
            }
            if mv.forward <= 0.0 {
                self.sprint_toggled = false;
            }
            mv.sprint = self.sprint_toggled;
        } else {
            while input.take_press(Action::Sprint) {}
        }
        if settings.controls.toggle_sneak {
            if input.take_press(Action::Sneak) {
                self.sneak_toggled = !self.sneak_toggled;
            }
            mv.sneak = self.sneak_toggled;
        } else {
            while input.take_press(Action::Sneak) {}
        }

        if self.autopilot {
            // Walk a slow circle, sprinting, jumping every 2 seconds.
            mv = MoveInput { forward: 1.0, sprint: true, jump: self.tick.is_multiple_of(40), ..mv };
            self.yaw += 1.5;
        }

        if self.input_blocked {
            mv = MoveInput::default(); // a menu is open: stand still
        }
        self.player.tick(mv, self.yaw as f64, &self.world);
        if !self.input_blocked {
            self.interact(input);
        }

        // Smoothly lower/raise the camera when sneaking (Minecraft eases this too).
        self.eye += (self.player.eye_height() - self.eye) * 0.5;

        // Safety net: falling out of the world respawns you.
        if self.player.pos.y < -128.0 {
            self.player = PlayerBody::new(self.spawn);
            self.prev_pos = self.player.pos;
            self.placed = false;
        }
    }

    /// Moves the player up until they aren't inside a block (e.g. spawning in a tree).
    fn place_safely(&mut self) {
        let mut boxes = Vec::new();
        for _ in 0..400 {
            boxes.clear();
            let a = self.player.aabb();
            let (min, max) =
                (a.min.floor().as_ivec3(), (a.max - glam::DVec3::splat(1e-7)).floor().as_ivec3());
            for x in min.x..=max.x {
                for y in min.y..=max.y {
                    for z in min.z..=max.z {
                        ld_physics::VoxelCollider::collision_boxes(
                            &self.world,
                            ld_core::coords::BlockPos::new(x, y, z),
                            &mut boxes,
                        );
                    }
                }
            }
            if !boxes.iter().any(|b| b.intersects(&a)) {
                break;
            }
            self.player.pos.y += 1.0;
        }
        self.prev_pos = self.player.pos;
        self.placed = true;
    }

    /// Applies mouse movement to the view. Called every *frame* (not tick) so
    /// looking around is as smooth as the framerate allows.
    pub fn look(&mut self, dx: f64, dy: f64, settings: &Settings) {
        // 0.15° per mouse count at 100 % sensitivity, as in Minecraft's default.
        let k = 0.15 * settings.controls.mouse_sensitivity as f64 / 100.0;
        let dy = if settings.controls.invert_mouse { -dy } else { dy };
        self.yaw = (self.yaw + (dx * k) as f32).rem_euclid(360.0);
        self.pitch = (self.pitch + (dy * k) as f32).clamp(-90.0, 90.0);
    }

    /// Developer start options (`--pos`, `--look`, `--fly`).
    pub fn apply_dev_start(&mut self, pos: Option<[f64; 3]>, look: Option<[f32; 2]>, fly: bool) {
        if let Some([x, y, z]) = pos {
            self.spawn = DVec3::new(x, y, z);
            self.player.pos = self.spawn;
            self.prev_pos = self.spawn;
            self.placed = true; // trust the given position
        }
        if let Some([yaw, pitch]) = look {
            self.yaw = yaw;
            self.pitch = pitch.clamp(-90.0, 90.0);
        }
        self.player.flying = fly;
    }

    /// The chunk column the player is in.
    pub fn player_chunk(&self) -> ld_core::coords::ChunkPos {
        ld_core::coords::BlockPos::new(self.player.pos.x.floor() as i32, 0, self.player.pos.z.floor() as i32)
            .chunk()
    }

    /// Eye position interpolated between the last two ticks.
    /// `alpha` is how far (0..1) we are into the current tick.
    pub fn eye_position(&self, alpha: f64) -> DVec3 {
        let feet = self.prev_pos.lerp(self.player.pos, alpha);
        feet + DVec3::new(0.0, self.prev_eye + (self.eye - self.prev_eye) * alpha, 0.0)
    }
}

#[cfg(test)]
mod tests {
    //! Headless tests of the real game-loop logic against a real `World`,
    //! using a hand-built test generator (floor, wall, staircase, tunnel).
    use super::*;
    use ld_core::coords::ChunkPos;
    use ld_input::Bindings;
    use ld_world::{BlockId, BlockRegistry, ChunkColumn};
    use ld_worldgen::TerrainGenerator;
    use std::sync::Arc;

    /// Floor top at y=0 (blocks at y<0), plus test structures near the origin.
    struct Course {
        stone: BlockId,
    }

    impl Course {
        fn solid(x: i32, y: i32, z: i32) -> bool {
            y < 0
                || ((-6..6).contains(&x) && z == 12 && (0..2).contains(&y)) // 2-high wall
                || ((4..10).contains(&x) && (6..9).contains(&z) && y <= x - 4 && y >= 0) // stairs
                || ((-15..-5).contains(&z) && ((x == 10 || x == 13) && (0..2).contains(&y) || (10..14).contains(&x) && y == 2)) // tunnel
        }
    }

    impl TerrainGenerator for Course {
        fn generate(&self, pos: ChunkPos) -> ChunkColumn {
            let mut c = ChunkColumn::new(pos);
            for lz in 0..16 {
                for lx in 0..16 {
                    for y in -8..8 {
                        if Self::solid(pos.x * 16 + lx as i32, y, pos.z * 16 + lz as i32) {
                            c.set(lx, y, lz, self.stone);
                        }
                    }
                }
            }
            c
        }
        fn surface_height(&self, _: i32, _: i32) -> i32 {
            -1
        }
        fn find_spawn(&self) -> (i32, i32, i32) {
            (0, 0, 0)
        }
    }

    fn game() -> Game {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/blocks.toml"))
                .unwrap();
        let reg = Arc::new(BlockRegistry::from_toml(&text).unwrap());
        let course = Arc::new(Course { stone: reg.id("stone").unwrap() });
        let table = Arc::new(ld_mesh::MeshTable::new(&reg, None, |_| 0));
        let mut world = World::new(reg, crate::world::tests::test_biomes(), 1, course, table, 1);
        world.load_now(ChunkPos::new(0, 0), 2);
        let items_toml =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/items.toml"))
                .unwrap();
        let blocks = BlockRegistry::from_toml(&text).unwrap();
        let items = Arc::new(ld_game::ItemRegistry::new(&blocks, &items_toml).unwrap());
        Game::new(world, items, false)
    }

    fn run(game: &mut Game, input: &mut InputState, ticks: u32) {
        let settings = Settings::default();
        for _ in 0..ticks {
            game.tick(input, &settings);
        }
    }

    fn press(input: &mut InputState, key: &str, down: bool) {
        input.handle_input(&Bindings::defaults(), key, down);
    }

    #[test]
    fn spawns_on_the_ground() {
        let mut g = game();
        let mut input = InputState::new();
        run(&mut g, &mut input, 20);
        assert!(g.player.on_ground);
        assert_eq!(g.player.pos.y, 0.0);
    }

    #[test]
    fn walking_into_the_wall_stops_at_its_surface() {
        let mut g = game();
        g.yaw = 0.0; // face south (+Z), toward the wall at z = 12
        let mut input = InputState::new();
        press(&mut input, "KeyW", true);
        run(&mut g, &mut input, 100);
        let expected = 12.0 - ld_physics::player::WIDTH / 2.0;
        assert!((g.player.pos.z - expected).abs() < 1e-6, "z = {}", g.player.pos.z);
        assert_eq!(g.player.pos.y, 0.0, "a 2-high wall can't be climbed");
    }

    #[test]
    fn can_climb_the_staircase_by_jumping() {
        let mut g = game();
        run(&mut g, &mut InputState::new(), 1); // place the player
        g.player.pos = glam::DVec3::new(2.5, 0.0, 7.5);
        g.yaw = 270.0; // face +X
        let mut input = InputState::new();
        press(&mut input, "KeyW", true);
        press(&mut input, "Space", true); // holding jump auto-jumps on landing
        let mut highest_ground: f64 = 0.0;
        for _ in 0..120 {
            run(&mut g, &mut input, 1);
            if g.player.on_ground {
                highest_ground = highest_ground.max(g.player.pos.y);
            }
        }
        assert_eq!(highest_ground, 6.0, "stood on the top step");
    }

    #[test]
    fn double_tap_jump_toggles_flight_and_sneak_descends() {
        let mut g = game();
        let mut input = InputState::new();
        run(&mut g, &mut input, 5);
        for _ in 0..2 {
            press(&mut input, "Space", true);
            run(&mut g, &mut input, 1);
            press(&mut input, "Space", false);
            run(&mut g, &mut input, 1);
        }
        assert!(g.player.flying);
        press(&mut input, "Space", true);
        run(&mut g, &mut input, 20);
        press(&mut input, "Space", false);
        assert!(g.player.pos.y > 5.0, "rose to {}", g.player.pos.y);
        press(&mut input, "ShiftLeft", true);
        run(&mut g, &mut input, 80);
        assert!(!g.player.flying && g.player.on_ground, "landing ends flight");
    }

    #[test]
    fn tunnel_ceiling_stops_jumps() {
        let mut g = game();
        run(&mut g, &mut InputState::new(), 1);
        g.player.pos = glam::DVec3::new(11.5, 0.0, -10.0);
        let mut input = InputState::new();
        press(&mut input, "Space", true);
        let mut peak: f64 = 0.0;
        for _ in 0..30 {
            run(&mut g, &mut input, 1);
            peak = peak.max(g.player.pos.y);
        }
        // Ceiling at y = 2, player is 1.8 tall → head stops at 2.0, feet at 0.2.
        assert!((peak - 0.2).abs() < 1e-6, "peak {peak}");
    }

    #[test]
    fn unloaded_terrain_acts_as_a_wall() {
        let mut g = game();
        g.yaw = 180.0; // north, toward z < -40 where nothing is loaded (radius 2 = z ≥ -32)
        let mut input = InputState::new();
        press(&mut input, "KeyW", true);
        press(&mut input, "ControlLeft", true);
        run(&mut g, &mut input, 400);
        assert!(g.player.pos.z >= -32.0, "stopped at the loaded edge, z = {}", g.player.pos.z);
        assert_eq!(g.player.pos.y, 0.0);
    }

    #[test]
    fn mouse_look_uses_minecraft_sensitivity_and_clamps_pitch() {
        let mut g = game();
        let s = Settings::default();
        let yaw0 = g.yaw;
        g.look(600.0, 0.0, &s); // 600 counts × 0.15° = 90°
        assert!(((g.yaw - yaw0).rem_euclid(360.0) - 90.0).abs() < 1e-3);
        g.look(0.0, 100_000.0, &s);
        assert_eq!(g.pitch, 90.0);
    }
}
