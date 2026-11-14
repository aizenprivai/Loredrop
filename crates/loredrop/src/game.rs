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
    pub crafting_table: Option<ld_world::BlockId>,
    pub furnace: Option<ld_world::BlockId>,
    pub lit_furnace: Option<ld_world::BlockId>,
    pub lore_tablet: Option<ld_world::BlockId>,
    pub chiseled_sandstone: Option<ld_world::BlockId>,
    pub deepslate_bricks: Option<ld_world::BlockId>,
    pub cactus: Option<ld_world::BlockId>,
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
    pub recipes: std::sync::Arc<ld_game::RecipeBook>,
    /// A menu the game wants opened (set when using a block like a crafting table).
    pub open_request: Option<crate::screens::Screen>,
    pub drops: std::sync::Arc<ld_game::mining::DropTable>,
    /// Dropped items lying in the world.
    pub entities: crate::entities::ItemEntities,
    /// Survival mining: the block being broken and how far along (0..1).
    pub breaking: Option<(ld_core::coords::BlockPos, f32)>,
    /// Ticks to wait before starting to mine the next block (Minecraft: 5).
    pub(crate) break_delay: u32,
    pub(crate) rng: u64,
    /// Furnaces placed in the world, by position (created when first opened).
    pub furnaces: std::collections::HashMap<ld_core::coords::BlockPos, ld_game::furnace::Furnace>,
    /// Story fragments for lore tablets.
    pub lore: std::sync::Arc<ld_assets::Lore>,
    /// Fragments the player has read, in discovery order: (era, index within era).
    pub journal: Vec<(String, usize)>,
    /// Health, hunger and air (only used in Survival).
    pub stats: ld_game::stats::PlayerStats,
    pub difficulty: ld_game::stats::Difficulty,
    /// Ticks spent eating the held food (32 = done).
    pub eating: u32,
    /// Ticks since the last arm swing (drives the attack cooldown).
    pub ticks_since_swing: u32,
    /// Creatures and arrows.
    pub mobs: crate::mobs::Mobs,
    /// Creatures to place in front of the player once the ground has loaded (`--summon`).
    pub pending_summons: Vec<String>,
}

impl Game {
    pub fn new(
        world: World,
        items: std::sync::Arc<ld_game::ItemRegistry>,
        recipes: std::sync::Arc<ld_game::RecipeBook>,
        drops: std::sync::Arc<ld_game::mining::DropTable>,
        autopilot: bool,
    ) -> Self {
        let ids = GameIds {
            water: world.registry.id("water"),
            lava: world.registry.id("lava"),
            crafting_table: world.registry.id("crafting_table"),
            furnace: world.registry.id("furnace"),
            lit_furnace: world.registry.id("lit_furnace"),
            lore_tablet: world.registry.id("lore_tablet"),
            chiseled_sandstone: world.registry.id("chiseled_sandstone"),
            deepslate_bricks: world.registry.id("deepslate_bricks"),
            cactus: world.registry.id("cactus"),
        };
        // A starter hotbar of building blocks (used when starting in Creative).
        let mut inventory = ld_game::Inventory::default();
        for (i, name) in [
            "grass_block",
            "dirt",
            "stone",
            "cobblestone",
            "oak_planks",
            "oak_log",
            "glass",
            "furnace",
            "crafting_table",
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
            recipes,
            open_request: None,
            drops,
            entities: Default::default(),
            breaking: None,
            break_delay: 0,
            rng: 0x2545_F491_4F6C_DD1D,
            furnaces: Default::default(),
            lore: std::sync::Arc::new(ld_assets::Lore { fragments: Vec::new() }),
            journal: Vec::new(),
            stats: Default::default(),
            difficulty: ld_game::stats::Difficulty::Normal,
            eating: 0,
            ticks_since_swing: 100,
            mobs: crate::mobs::Mobs::new(std::sync::Arc::new(
                ld_game::mobs::MobRegistry::from_toml(
                    "mob = []",
                    &ld_game::ItemRegistry::empty(),
                    &ld_world::BiomeRegistry::empty(),
                )
                .expect("empty mob list"),
            )),
            pending_summons: Vec::new(),
        }
    }

    /// Advances the world by one tick.
    pub fn tick(&mut self, input: &mut InputState, settings: &Settings) {
        self.prev_pos = self.player.pos;
        self.prev_eye = self.eye;
        self.tick += 1;
        self.ticks_since_swing = self.ticks_since_swing.saturating_add(1);

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

        if self.input_blocked || self.stats.is_dead() {
            mv = MoveInput::default(); // a menu is open (or dead): stand still
        }
        let survival = self.mode == GameMode::Survival;
        if survival && !self.stats.can_sprint() {
            mv.sprint = false; // too hungry to sprint
        }
        self.player.fluid = self.fluid_at_body();
        let (was_on_ground, pos_before) = (self.player.on_ground, self.player.pos);
        self.player.tick(mv, self.yaw as f64, &self.world);
        if survival {
            self.survival_tick(mv, was_on_ground, pos_before);
        }
        if !self.input_blocked {
            self.interact(input);
        }
        let player_box = self.player.aabb();
        self.entities.tick(&self.world, &player_box, &mut self.inventory, &self.items);
        self.tick_furnaces();
        self.tick_mobs();

        // Smoothly lower/raise the camera when sneaking (Minecraft eases this too).
        self.eye += (self.player.eye_height() - self.eye) * 0.5;

        // Safety net: in Creative, falling out of the world brings you back.
        if self.mode == GameMode::Creative && self.player.pos.y < -128.0 {
            self.player = PlayerBody::new(self.spawn);
            self.prev_pos = self.player.pos;
            self.placed = false;
        }
    }

    /// Which liquid the player's body is in (lava wins over water).
    fn fluid_at_body(&self) -> ld_physics::player::Fluid {
        use ld_physics::player::Fluid;
        let p = self.player.pos;
        let mut fluid = Fluid::None;
        for dy in [0.1, 0.9] {
            let b = self.world.map.block(ld_core::coords::BlockPos::new(
                p.x.floor() as i32,
                (p.y + dy).floor() as i32,
                p.z.floor() as i32,
            ));
            if b.is_some() && b == self.ids.lava {
                return Fluid::Lava;
            }
            if b.is_some() && b == self.ids.water {
                fluid = Fluid::Water;
            }
        }
        fluid
    }

    /// The block at the player's eyes.
    fn block_at_eyes(&self) -> Option<ld_world::BlockId> {
        let p = self.player.pos + DVec3::new(0.0, self.eye, 0.0);
        self.world.map.block(ld_core::coords::BlockPos::new(
            p.x.floor() as i32,
            p.y.floor() as i32,
            p.z.floor() as i32,
        ))
    }

    /// Hurts the player (Survival only), reducing by armour where it applies.
    pub fn hurt(&mut self, amount: f32, source: ld_game::stats::DamageSource) {
        if self.mode != GameMode::Survival {
            return;
        }
        let reduced = if source.armor_applies() { self.armor_reduce(amount) } else { amount };
        self.stats.damage(reduced, source);
    }

    /// Worn armour pieces' stats.
    fn worn_armor(&self) -> impl Iterator<Item = ld_game::ArmorInfo> + '_ {
        self.inventory.armor.iter().flatten().filter_map(|s| match &self.items.get(s.item).kind {
            ld_game::ItemKind::Armor(a) => Some(a.clone()),
            _ => None,
        })
    }

    /// Reduces damage by worn armour (Minecraft's formula) and wears the
    /// armour down: each piece loses max(1, damage / 4) durability.
    pub fn armor_reduce(&mut self, amount: f32) -> f32 {
        let (defense, toughness) =
            self.worn_armor().fold((0.0, 0.0), |(d, t), a| (d + a.defense as f32, t + a.toughness));
        if defense <= 0.0 {
            return amount;
        }
        let wear = ((amount / 4.0) as u16).max(1);
        for i in 0..4 {
            if let Some(stack) = self.inventory.armor[i]
                && let ld_game::ItemKind::Armor(a) = &self.items.get(stack.item).kind
            {
                let damage = stack.damage + wear;
                self.inventory.armor[i] =
                    (damage < a.durability).then_some(ld_game::ItemStack { damage, ..stack });
            }
        }
        ld_game::stats::armor_reduction(amount, defense, toughness)
    }

    /// Total armour points worn (0–20).
    pub fn armor_points(&self) -> i32 {
        self.worn_armor().map(|a| a.defense).sum()
    }

    /// Hunger, environmental damage and breath, once per Survival tick.
    fn survival_tick(&mut self, mv: MoveInput, was_on_ground: bool, pos_before: DVec3) {
        use ld_game::stats::{DamageSource, fall_damage};
        use ld_physics::player::Fluid;
        let fluid = self.player.fluid;
        // Exhaustion from movement (Minecraft values).
        let moved =
            glam::DVec2::new(self.player.pos.x - pos_before.x, self.player.pos.z - pos_before.z).length();
        if fluid == Fluid::Water {
            self.stats.exhaust(0.01 * moved as f32);
        } else if self.player.sprinting {
            self.stats.exhaust(0.1 * moved as f32);
        }
        if was_on_ground && !self.player.on_ground && self.player.vel.y > 0.0 && fluid == Fluid::None {
            self.stats.exhaust(if self.player.sprinting { 0.2 } else { 0.05 });
        }
        let _ = mv;
        // Falling.
        if let Some(d) = self.player.landed_after_fall.take() {
            let dmg = fall_damage(d);
            if dmg > 0.0 {
                self.hurt(dmg, DamageSource::Fall);
            }
        }
        // Lava and fire.
        if fluid == Fluid::Lava {
            self.hurt(4.0, DamageSource::Lava);
            self.stats.fire_ticks = self.stats.fire_ticks.max(300);
        }
        // Stuck inside a solid block.
        if let Some(id) = self.block_at_eyes()
            && self.world.registry.get(id).is_opaque()
            && self.world.registry.get(id).solid
        {
            self.hurt(1.0, DamageSource::Suffocation);
        }
        // Touching a cactus.
        if self.touching(self.ids.cactus) {
            self.hurt(1.0, DamageSource::Cactus);
        }
        // Below the world.
        if self.player.pos.y < -128.0 {
            self.hurt(4.0, DamageSource::Void);
        }
        let eyes_in_water = self.block_at_eyes().is_some() && self.block_at_eyes() == self.ids.water;
        self.stats.tick(self.difficulty, eyes_in_water, fluid == Fluid::Water);
    }

    /// Whether the player's hitbox (slightly enlarged) touches this block type.
    fn touching(&self, block: Option<ld_world::BlockId>) -> bool {
        let Some(block) = block else { return false };
        let a = self.player.aabb();
        let (min, max) = (
            (a.min - DVec3::splat(0.01)).floor().as_ivec3(),
            (a.max + DVec3::splat(0.01)).floor().as_ivec3(),
        );
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    if self.world.map.block(ld_core::coords::BlockPos::new(x, y, z)) == Some(block) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Creatures: spawning, AI, their attacks on the player, deaths and loot.
    fn tick_mobs(&mut self) {
        if !self.pending_summons.is_empty() && self.placed {
            let names = std::mem::take(&mut self.pending_summons);
            let fwd = self.look_dir();
            let side = glam::DVec3::new(fwd.z, 0.0, -fwd.x).normalize_or_zero();
            let n = names.len() as f64;
            for (i, name) in names.iter().enumerate() {
                match self.mobs.defs.id(name) {
                    Some(kind) => {
                        let base =
                            self.player.pos + glam::DVec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero() * 5.0;
                        let p = base + side * ((i as f64 - (n - 1.0) / 2.0) * 2.2);
                        // Put it on the ground at that spot.
                        let (x, z) = (p.x.floor() as i32, p.z.floor() as i32);
                        let y = (self.player.pos.y as i32 - 8..self.player.pos.y as i32 + 8)
                            .rev()
                            .find(|&y| {
                                let solid = |yy| {
                                    self.world
                                        .map
                                        .block(ld_core::coords::BlockPos::new(x, yy, z))
                                        .is_some_and(|b| self.world.registry.get(b).solid)
                                };
                                solid(y - 1) && !solid(y) && !solid(y + 1)
                            })
                            .unwrap_or(self.player.pos.y as i32);
                        self.mobs.spawn(kind, glam::DVec3::new(p.x, y as f64, p.z));
                    }
                    None => log::warn!("--summon: unknown creature '{name}'"),
                }
            }
        }
        for chunk in std::mem::take(&mut self.world.newly_loaded) {
            self.mobs.spawn_animals_in(&self.world, chunk);
        }
        self.mobs.spawn_hostiles(&self.world, self.player.pos, self.difficulty);
        let target = crate::mobs::Target {
            feet: self.player.pos,
            eyes: self.player.pos + DVec3::new(0.0, self.eye, 0.0),
            aabb: self.player.aabb(),
            attackable: self.mode == GameMode::Survival && !self.stats.is_dead(),
        };
        let (events, deaths) = self.mobs.tick(&self.world, target, self.difficulty);
        for e in events {
            let crate::mobs::MobEvent::HurtPlayer { damage, from } = e;
            let before = self.stats.health;
            self.hurt(damage, ld_game::stats::DamageSource::Attack);
            if self.stats.health < before {
                // Knockback away from the attacker (Minecraft strength 0.4).
                let away = glam::DVec2::new(self.player.pos.x - from.x, self.player.pos.z - from.z)
                    .normalize_or_zero();
                self.player.vel.x = self.player.vel.x / 2.0 + away.x * 0.4;
                self.player.vel.z = self.player.vel.z / 2.0 + away.y * 0.4;
                if self.player.on_ground {
                    self.player.vel.y = 0.4;
                }
            }
        }
        for (pos, kind) in deaths {
            let drops = self.mobs.defs.get(kind).drops.clone();
            for s in ld_game::mining::roll_drops(&drops, || self.next_random_pub()) {
                self.entities.drop_from_block(
                    pos.x.floor() as i32,
                    pos.y.floor() as i32,
                    pos.z.floor() as i32,
                    s,
                );
            }
        }
        self.mobs.despawn(&self.world, self.player.pos);
    }

    /// Smelts in every furnace and swaps the block between lit and unlit.
    fn tick_furnaces(&mut self) {
        let (Some(unlit), Some(lit)) = (self.ids.furnace, self.ids.lit_furnace) else { return };
        let mut changes = Vec::new();
        for (pos, f) in self.furnaces.iter_mut() {
            if f.tick(&self.recipes, &self.items) {
                changes.push((*pos, if f.is_lit() { lit } else { unlit }));
            }
        }
        for (pos, block) in changes {
            self.world.set_block(pos, block);
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

    /// Current attack strength (0..1) with the held weapon.
    pub fn attack_strength(&self) -> f32 {
        let (_, speed) = ld_game::combat::weapon_stats(self.inventory.held(), &self.items);
        ld_game::combat::attack_strength(self.ticks_since_swing, speed)
    }

    /// Called when the player dies: everything carried spills on the ground
    /// (as in Minecraft) and the inventory empties.
    pub fn on_death(&mut self) {
        let p = self.player.pos;
        let (x, y, z) = (p.x.floor() as i32, (p.y + 0.5).floor() as i32, p.z.floor() as i32);
        let inv = std::mem::take(&mut self.inventory);
        for s in inv.slots.into_iter().chain(inv.armor).chain(std::iter::once(inv.offhand)).flatten() {
            self.entities.drop_from_block(x, y, z, s);
        }
        self.breaking = None;
        self.eating = 0;
    }

    /// Brings the player back at the world spawn with full health and hunger.
    pub fn respawn(&mut self) {
        self.stats = Default::default();
        self.player = PlayerBody::new(self.spawn);
        self.prev_pos = self.spawn;
        self.placed = false;
    }

    /// Sets the starting game mode. Survival starts with an empty inventory,
    /// like a fresh Minecraft world; Creative keeps the starter hotbar.
    pub fn set_start_mode(&mut self, mode: GameMode) {
        self.mode = mode;
        if mode == GameMode::Survival {
            self.inventory = ld_game::Inventory::default();
            self.player.flying = false;
        }
    }

    /// Developer start options (`--pos`, `--look`, `--fly`).
    pub fn apply_dev_start(&mut self, pos: Option<[f64; 3]>, look: Option<[f32; 2]>, fly: bool) {
        if let Some([x, y, z]) = pos {
            // Only the starting position: respawning still uses the world spawn.
            self.player.pos = DVec3::new(x, y, z);
            self.prev_pos = self.player.pos;
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
        let drops = Arc::new(ld_game::mining::DropTable::new(&blocks, &items).unwrap());
        Game::new(world, items, Arc::new(ld_game::RecipeBook::default()), drops, false)
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
    fn survival_fall_damage_and_creative_immunity() {
        let mut g = game();
        g.set_start_mode(GameMode::Survival);
        let mut input = InputState::new();
        run(&mut g, &mut input, 1);
        g.player.pos = glam::DVec3::new(0.5, 20.0, 0.5);
        for _ in 0..80 {
            run(&mut g, &mut input, 1);
            if g.player.on_ground {
                break;
            }
        }
        assert_eq!(g.stats.health, 3.0, "a 20-block fall deals 17 damage");
        let mut c = game();
        run(&mut c, &mut input, 1);
        c.player.pos = glam::DVec3::new(0.5, 40.0, 0.5);
        run(&mut c, &mut input, 120);
        assert_eq!(c.stats.health, 20.0, "Creative takes no damage");
    }

    #[test]
    fn armor_reduces_attack_damage_and_wears_down() {
        let mut g = game();
        g.set_start_mode(GameMode::Survival);
        for (i, piece) in ["iron_helmet", "iron_chestplate", "iron_leggings", "iron_boots"].iter().enumerate()
        {
            g.inventory.armor[i] = Some(ld_game::ItemStack::new(g.items.id(piece).unwrap(), 1));
        }
        assert_eq!(g.armor_points(), 15);
        g.hurt(6.0, ld_game::stats::DamageSource::Attack);
        assert!((g.stats.health - (20.0 - 3.12)).abs() < 1e-4);
        assert!(g.inventory.armor.iter().all(|a| a.unwrap().damage == 1));
    }

    #[test]
    fn eating_an_apple_takes_32_ticks() {
        let mut g = game();
        g.set_start_mode(GameMode::Survival);
        g.stats.food = 10;
        g.inventory.slots[0] = Some(ld_game::ItemStack::new(g.items.id("apple").unwrap(), 2));
        g.pitch = -90.0; // look at the sky so right-click doesn't hit a block
        let mut input = InputState::new();
        press(&mut input, "MouseRight", true);
        run(&mut g, &mut input, 31);
        assert_eq!(g.stats.food, 10, "not yet");
        run(&mut g, &mut input, 1);
        assert_eq!(g.stats.food, 14);
        assert_eq!(g.inventory.slots[0].unwrap().count, 1);
    }

    #[test]
    fn dying_drops_everything_and_respawning_restores_the_player() {
        let mut g = game();
        g.set_start_mode(GameMode::Survival);
        let dirt = g.items.id("dirt").unwrap();
        g.inventory.slots[3] = Some(ld_game::ItemStack::new(dirt, 20));
        let mut input = InputState::new();
        run(&mut g, &mut input, 1);
        g.player.pos = glam::DVec3::new(0.5, 40.0, 0.5);
        for _ in 0..120 {
            run(&mut g, &mut input, 1);
            if g.stats.is_dead() {
                break;
            }
        }
        assert_eq!(g.stats.death, Some(ld_game::stats::DamageSource::Fall));
        g.on_death();
        assert_eq!(g.inventory.count_of(dirt), 0);
        assert_eq!(
            g.entities.list.iter().map(|e| e.stack.count as u32).sum::<u32>(),
            20,
            "items spilled on the ground"
        );
        g.respawn();
        run(&mut g, &mut input, 5);
        assert!(!g.stats.is_dead() && g.stats.health == 20.0 && g.stats.food == 20);
        assert!(g.player.pos.y < 2.0, "back at spawn");
    }

    #[test]
    fn attack_cooldown_resets_on_swing() {
        let mut g = game();
        let mut input = InputState::new();
        run(&mut g, &mut input, 30);
        assert_eq!(g.attack_strength(), 1.0);
        g.pitch = -90.0;
        press(&mut input, "MouseLeft", true);
        run(&mut g, &mut input, 1);
        press(&mut input, "MouseLeft", false);
        assert!(g.attack_strength() < 0.5, "a swing at the air resets the cooldown");
        run(&mut g, &mut input, 6);
        assert_eq!(g.attack_strength(), 1.0, "bare hand recharges in 5 ticks");
    }

    /// The test game with the real creature definitions loaded.
    fn game_with_mobs() -> Game {
        let mut g = game();
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/");
        let mobs = ld_game::mobs::MobRegistry::from_toml(
            &std::fs::read_to_string(format!("{dir}mobs.toml")).unwrap(),
            &g.items,
            &g.world.biomes,
        )
        .unwrap();
        g.mobs = crate::mobs::Mobs::new(Arc::new(mobs));
        g
    }

    #[test]
    fn zombie_chases_and_hurts_a_survival_player() {
        let mut g = game_with_mobs();
        g.set_start_mode(GameMode::Survival);
        let mut input = InputState::new();
        run(&mut g, &mut input, 1);
        let zombie = g.mobs.defs.id("zombie").unwrap();
        g.mobs.spawn(zombie, glam::DVec3::new(0.5, 0.0, -6.5));
        run(&mut g, &mut input, 100);
        assert!(g.stats.health < 20.0, "the zombie reached and hit the player (health {})", g.stats.health);
        // Creative players are ignored.
        let mut c = game_with_mobs();
        run(&mut c, &mut input, 1);
        c.mobs.spawn(zombie, glam::DVec3::new(0.5, 0.0, -2.5));
        run(&mut c, &mut input, 100);
        assert_eq!(c.stats.health, 20.0);
    }

    #[test]
    fn killing_a_cow_drops_its_loot() {
        let mut g = game_with_mobs();
        g.set_start_mode(GameMode::Survival);
        let mut input = InputState::new();
        run(&mut g, &mut input, 1);
        g.yaw = 180.0; // facing −Z
        g.pitch = 25.0;
        let cow = g.mobs.defs.id("cow").unwrap();
        g.mobs.spawn(cow, glam::DVec3::new(0.5, 0.0, -2.0));
        g.mobs.list[0].brain = crate::mobs::Brain::Idle(10_000); // stand still for the test
        // Fully charged punches (1 damage each): 10 hits for 10 health.
        for _ in 0..40 {
            if g.mobs.list.first().is_none_or(|m| m.death_time > 0) {
                break;
            }
            // Keep the cow in front of the player for the test.
            if let Some(m) = g.mobs.list.first_mut() {
                m.pos = glam::DVec3::new(0.5, 0.0, -2.0);
                m.vel = glam::DVec3::ZERO;
            }
            press(&mut input, "MouseLeft", true);
            run(&mut g, &mut input, 1);
            press(&mut input, "MouseLeft", false);
            run(&mut g, &mut input, 11); // wait out the invulnerability + cooldown
        }
        run(&mut g, &mut input, 30); // death animation, then loot
        assert!(g.mobs.list.is_empty(), "the cow died");
        let loot: Vec<_> = g.entities.list.iter().map(|e| g.items.get(e.stack.item).name.clone()).collect();
        assert!(loot.iter().any(|n| n == "beef"), "dropped beef: {loot:?}");
    }

    #[test]
    fn peaceful_removes_hostiles() {
        let mut g = game_with_mobs();
        g.difficulty = ld_game::stats::Difficulty::Peaceful;
        let zombie = g.mobs.defs.id("zombie").unwrap();
        g.mobs.spawn(zombie, glam::DVec3::new(0.5, 0.0, -6.5));
        run(&mut g, &mut InputState::new(), 2);
        assert!(g.mobs.list.is_empty());
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
