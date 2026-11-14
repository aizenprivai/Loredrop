//! Creatures: movement, AI, spawning, combat and despawning.
//!
//! * **Passive** creatures idle, wander to random nearby spots, and panic
//!   (run fast in random directions) for a few seconds after being hurt.
//! * **Hostile** creatures notice the player within 16 blocks (if they can
//!   see them) and chase along a path; melee attackers hit when in reach
//!   (once per second), ranged ones keep their distance and shoot arrows.
//!   Spiders climb walls.
//! * Creatures take damage from players with Minecraft's knockback and a
//!   10-tick invulnerability, fall over when they die, and drop loot.
//!
//! Spawning: animals appear in groups when a column is first loaded; hostile
//! creatures appear near the player in dark places with no sky above (caves
//! and covered areas — until the day/night cycle of Phase 8 makes the surface
//! dark at night). Peaceful difficulty spawns no hostiles.

pub mod path;
pub mod render;

use crate::world::World;
use glam::DVec3;
use ld_core::coords::{BlockPos, ChunkPos};
use ld_game::mobs::{Category, MobDef, MobKind, MobRegistry};
use ld_game::stats::Difficulty;
use ld_physics::player::Fluid;
use ld_physics::{Aabb, move_and_collide};
use path::{PathWorld, find_path, standable};
use std::sync::Arc;

/// What a creature is currently doing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Brain {
    /// Standing still for this many more ticks.
    Idle(u32),
    /// Walking along `path` to a random spot.
    Wander,
    /// Running around after being hurt, for this many more ticks.
    Panic(u32),
    /// Hunting the player.
    Chase,
}

pub struct Mob {
    pub kind: MobKind,
    pub pos: DVec3,
    pub prev_pos: DVec3,
    pub vel: DVec3,
    /// Body facing (degrees, Minecraft convention).
    pub yaw: f32,
    pub prev_yaw: f32,
    pub head_pitch: f32,
    pub on_ground: bool,
    pub health: f32,
    /// Ticks of the red "hurt" flash left (also the invulnerability window).
    pub hurt_time: u32,
    /// Ticks since death (0 = alive); removed at 20.
    pub death_time: u32,
    /// Distance walked, drives the leg animation.
    pub walk_dist: f32,
    pub brain: Brain,
    pub path: Vec<BlockPos>,
    path_timer: u32,
    attack_cooldown: u32,
    pub age: u32,
    horizontal_collision: bool,
}

/// An arrow in flight (shot by skeletons).
pub struct Arrow {
    pub pos: DVec3,
    pub prev_pos: DVec3,
    pub vel: DVec3,
    pub age: u32,
    /// Stuck in a block: stops moving and disappears after a while.
    pub stuck: bool,
}

/// Things that happened to the player this tick.
pub enum MobEvent {
    /// The player was hit: damage and the attacker's position (for knockback).
    HurtPlayer { damage: f32, from: DVec3 },
}

/// The player as creatures see them.
#[derive(Clone, Copy)]
pub struct Target {
    pub feet: DVec3,
    pub eyes: DVec3,
    pub aabb: Aabb,
    /// False in Creative or while dead: hostiles ignore the player.
    pub attackable: bool,
}

/// Maximum hostile creatures around the player (Minecraft: 70 per player).
const HOSTILE_CAP: usize = 50;

pub struct Mobs {
    pub list: Vec<Mob>,
    pub arrows: Vec<Arrow>,
    pub defs: Arc<MobRegistry>,
    next_id: u32,
    rng: u64,
    tick: u64,
}

/// Lets the pathfinder query the loaded world.
struct WorldView<'a>(&'a World);

impl PathWorld for WorldView<'_> {
    fn is_solid(&self, p: BlockPos) -> bool {
        match self.0.map.block(p) {
            Some(id) => self.0.registry.get(id).solid,
            None => true, // unloaded: treat as a wall
        }
    }
    fn is_water(&self, p: BlockPos) -> bool {
        self.0.map.block(p).is_some_and(|id| self.0.registry.get(id).name == "water")
    }
    fn is_dangerous(&self, p: BlockPos) -> bool {
        self.0
            .map
            .block(p)
            .is_some_and(|id| matches!(self.0.registry.get(id).name.as_str(), "lava" | "cactus"))
    }
}

impl Mobs {
    pub fn new(defs: Arc<MobRegistry>) -> Self {
        Self { list: Vec::new(), arrows: Vec::new(), defs, next_id: 1, rng: 0x5DEE_CE66_D1CE_4E5B, tick: 0 }
    }

    fn rand(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.rand() * (hi - lo + 1) as f64) as i32
    }

    /// Adds a creature at `pos` (feet).
    pub fn spawn(&mut self, kind: MobKind, pos: DVec3) {
        let health = self.defs.get(kind).health;
        let yaw = (self.rand() * 360.0) as f32;
        self.list.push(Mob {
            kind,
            pos,
            prev_pos: pos,
            vel: DVec3::ZERO,
            yaw,
            prev_yaw: yaw,
            head_pitch: 0.0,
            on_ground: false,
            health,
            hurt_time: 0,
            death_time: 0,
            walk_dist: 0.0,
            brain: Brain::Idle(20),
            path: Vec::new(),
            path_timer: 0,
            attack_cooldown: 0,
            age: 0,
            horizontal_collision: false,
        });
        self.next_id += 1;
    }

    pub fn hitbox(&self, m: &Mob) -> Aabb {
        let d = self.defs.get(m.kind);
        Aabb::from_feet(m.pos, d.width, d.height)
    }

    /// Damages a creature, with knockback away from `from`. Returns false if it
    /// was invulnerable (hurt within the last 10 ticks) or already dead.
    pub fn hurt(&mut self, index: usize, damage: f32, from: DVec3, knockback: f64) -> bool {
        let m = &mut self.list[index];
        if m.death_time > 0 || m.hurt_time > 0 {
            return false;
        }
        m.health -= damage;
        m.hurt_time = 10;
        // Minecraft knockback: halve current velocity, push away, small hop.
        let away = glam::DVec2::new(m.pos.x - from.x, m.pos.z - from.z).normalize_or_zero();
        m.vel.x = m.vel.x / 2.0 + away.x * knockback;
        m.vel.z = m.vel.z / 2.0 + away.y * knockback;
        if m.on_ground {
            m.vel.y = (m.vel.y / 2.0 + knockback).min(0.4);
        }
        if m.health <= 0.0 {
            m.death_time = 1;
        } else if self.defs.get(m.kind).category == Category::Passive {
            m.brain = Brain::Panic(60);
            m.path.clear();
        }
        true
    }

    /// Advances every creature and arrow by one tick.
    pub fn tick(
        &mut self,
        world: &World,
        target: Target,
        difficulty: Difficulty,
    ) -> (Vec<MobEvent>, Vec<(DVec3, MobKind)>) {
        self.tick += 1;
        let mut events = Vec::new();
        let mut deaths = Vec::new();
        let view = WorldView(world);
        let defs = self.defs.clone();
        for i in 0..self.list.len() {
            let def = defs.get(self.list[i].kind);
            {
                let m = &mut self.list[i];
                m.prev_pos = m.pos;
                m.prev_yaw = m.yaw;
                m.age += 1;
                m.hurt_time = m.hurt_time.saturating_sub(1);
                m.attack_cooldown = m.attack_cooldown.saturating_sub(1);
            }
            if self.list[i].death_time > 0 {
                let m = &mut self.list[i];
                m.death_time += 1;
                if m.death_time == 20 {
                    deaths.push((m.pos, m.kind));
                }
                physics(m, def, world, None, 0.0);
                continue;
            }
            // --- Decide where to go ---
            let (goal_dir, speed_mod) = self.think(i, def, &view, target, difficulty, &mut events);
            let m = &mut self.list[i];
            physics(m, def, world, goal_dir, speed_mod);
        }
        self.list.retain(|m| m.death_time < 20);
        self.tick_arrows(world, target, &mut events);
        (events, deaths)
    }

    /// AI for creature `i`: returns the direction to move (if any) and a speed multiplier.
    fn think(
        &mut self,
        i: usize,
        def: &MobDef,
        view: &WorldView,
        target: Target,
        difficulty: Difficulty,
        events: &mut Vec<MobEvent>,
    ) -> (Option<DVec3>, f64) {
        let height = def.height.ceil() as i32;
        let hostile =
            def.category == Category::Hostile && target.attackable && difficulty != Difficulty::Peaceful;
        let m_pos = self.list[i].pos;
        let to_player = target.feet - m_pos;
        let dist = to_player.length();

        // Hostiles notice the player within 16 blocks if they can see them (or very close).
        if hostile {
            let eyes = m_pos + DVec3::new(0.0, def.height * 0.85, 0.0);
            let notices = dist < 16.0 && (dist < 4.0 || line_of_sight(view, eyes, target.eyes));
            let chasing = self.list[i].brain == Brain::Chase;
            if notices || (chasing && dist < 32.0) {
                self.list[i].brain = Brain::Chase;
            } else if chasing {
                self.list[i].brain = Brain::Idle(40);
                self.list[i].path.clear();
            }
        }

        let brain = self.list[i].brain;
        match brain {
            Brain::Idle(t) => {
                if t == 0 {
                    // Pick a random spot within 8 blocks to wander to.
                    let (dx, dz) = (self.range(-8, 8), self.range(-8, 8));
                    let start = feet_block(m_pos);
                    let goal = BlockPos::new(start.x + dx, start.y, start.z + dz);
                    let path = find_path(view, start, goal, height);
                    let m = &mut self.list[i];
                    m.path = path;
                    m.brain = if m.path.is_empty() { Brain::Idle(40) } else { Brain::Wander };
                } else {
                    self.list[i].brain = Brain::Idle(t - 1);
                }
                (None, 0.0)
            }
            Brain::Wander => {
                let dir = follow_path(&mut self.list[i]);
                if dir.is_none() {
                    let wait = self.range(40, 140) as u32;
                    self.list[i].brain = Brain::Idle(wait);
                }
                (dir, 1.0)
            }
            Brain::Panic(t) => {
                if self.list[i].path.is_empty() || t % 20 == 0 {
                    let (dx, dz) = (self.range(-6, 6), self.range(-6, 6));
                    let start = feet_block(m_pos);
                    self.list[i].path =
                        find_path(view, start, BlockPos::new(start.x + dx, start.y, start.z + dz), height);
                }
                let dir = follow_path(&mut self.list[i]);
                self.list[i].brain = if t == 0 { Brain::Idle(20) } else { Brain::Panic(t - 1) };
                (dir, 2.0)
            }
            Brain::Chase => {
                let m = &mut self.list[i];
                // Look at the player.
                m.yaw = yaw_towards(to_player);
                let horiz = glam::DVec2::new(to_player.x, to_player.z).length();
                m.head_pitch =
                    (-(target.eyes.y - (m.pos.y + def.height * 0.85)).atan2(horiz).to_degrees()) as f32;
                if def.ranged {
                    // Keep 6–14 blocks away and shoot every 2 seconds when in sight.
                    if m.attack_cooldown == 0 && dist < 16.0 {
                        let eyes = m.pos + DVec3::new(0.0, def.height * 0.85, 0.0);
                        if line_of_sight(view, eyes, target.eyes) {
                            m.attack_cooldown = 40;
                            let aim = target.eyes - DVec3::new(0.0, 0.3, 0.0) - eyes;
                            // Aim a little high to compensate for the arrow's drop.
                            let vel = (aim + DVec3::new(0.0, aim.length() * 0.12, 0.0)).normalize() * 1.6;
                            self.arrows.push(Arrow { pos: eyes, prev_pos: eyes, vel, age: 0, stuck: false });
                        }
                    }
                    let dir = if dist > 14.0 {
                        Some(DVec3::new(to_player.x, 0.0, to_player.z).normalize_or_zero())
                    } else if dist < 6.0 {
                        Some(DVec3::new(-to_player.x, 0.0, -to_player.z).normalize_or_zero())
                    } else {
                        None
                    };
                    return (dir, 1.0);
                }
                // Melee: attack when the hitboxes are close enough (Minecraft's reach rule).
                let reach = (def.width * 2.0).powi(2) + 0.6;
                let dxz = glam::DVec2::new(to_player.x, to_player.z).length_squared();
                if dxz <= reach && to_player.y.abs() < 2.0 && m.attack_cooldown == 0 {
                    m.attack_cooldown = 20;
                    let base = def.attack_damage;
                    let damage = match difficulty {
                        Difficulty::Easy => (base / 2.0 + 1.0).min(base),
                        Difficulty::Normal => base,
                        Difficulty::Hard => base * 1.5,
                        Difficulty::Peaceful => 0.0,
                    };
                    events.push(MobEvent::HurtPlayer { damage, from: m.pos });
                }
                // Re-plan the path twice a second; walk straight when close.
                m.path_timer = m.path_timer.saturating_sub(1);
                if dist < 2.5 {
                    m.path.clear();
                    return (Some(DVec3::new(to_player.x, 0.0, to_player.z).normalize_or_zero()), 1.0);
                }
                if m.path_timer == 0 || m.path.is_empty() {
                    m.path_timer = 10;
                    let start = feet_block(m.pos);
                    m.path = find_path(view, start, feet_block(target.feet), height);
                }
                (follow_path(m), 1.0)
            }
        }
    }

    fn tick_arrows(&mut self, world: &World, target: Target, events: &mut Vec<MobEvent>) {
        for a in &mut self.arrows {
            a.age += 1;
            a.prev_pos = a.pos;
            if a.stuck {
                continue;
            }
            // Step in small increments so fast arrows don't pass through things.
            let steps = 4;
            for _ in 0..steps {
                let next = a.pos + a.vel / steps as f64;
                if target.attackable
                    && target.aabb.intersects(&Aabb::new(next - DVec3::splat(0.1), next + DVec3::splat(0.1)))
                {
                    // Minecraft: arrow damage grows with speed (≈2 per block/tick, rounded up).
                    let damage = (a.vel.length() * 2.0).ceil() as f32;
                    events.push(MobEvent::HurtPlayer { damage, from: a.pos - a.vel });
                    a.age = 10_000; // remove
                    break;
                }
                let b = BlockPos::new(next.x.floor() as i32, next.y.floor() as i32, next.z.floor() as i32);
                if world.map.block(b).is_none_or(|id| world.registry.get(id).solid) {
                    a.stuck = true;
                    break;
                }
                a.pos = next;
            }
            a.vel.y -= 0.05;
            a.vel *= 0.99;
        }
        self.arrows.retain(|a| a.age < if a.stuck { 400 } else { 1200 });
    }

    // ------------------------------------------------------------------
    // Spawning and despawning
    // ------------------------------------------------------------------

    /// Animals for a freshly loaded column (about one column in ten gets a
    /// herd, like Minecraft's chunk-generation spawning).
    pub fn spawn_animals_in(&mut self, world: &World, chunk: ChunkPos) {
        if self.rand() > 0.1 {
            return;
        }
        let Some(col) = world.map.get(chunk) else { return };
        let biome = col.biome(8, 8);
        let candidates: Vec<(MobKind, u32, [u32; 2])> = self
            .defs
            .iter()
            .filter(|(_, d)| d.category == Category::Passive && d.spawn.surface.contains(&biome))
            .map(|(k, d)| (k, d.spawn.weight, d.spawn.group))
            .collect();
        let total: u32 = candidates.iter().map(|c| c.1).sum();
        if total == 0 {
            return;
        }
        let mut pick = (self.rand() * total as f64) as u32;
        let Some(&(kind, _, group)) = candidates.iter().find(|c| {
            if pick < c.1 {
                true
            } else {
                pick -= c.1;
                false
            }
        }) else {
            return;
        };
        let n = self.range(group[0] as i32, group[1] as i32);
        let grass = world.registry.id("grass_block");
        let height = self.defs.get(kind).height.ceil() as i32;
        let view = WorldView(world);
        for _ in 0..n {
            let (x, z) = (chunk.x * 16 + self.range(0, 15), chunk.z * 16 + self.range(0, 15));
            // Find the surface by scanning down from the sky.
            for y in (40..200).rev() {
                let p = BlockPos::new(x, y, z);
                let below = world.map.block(BlockPos::new(x, y - 1, z));
                if below.is_some() && below == grass && standable(&view, p, height) {
                    self.spawn(kind, DVec3::new(x as f64 + 0.5, y as f64, z as f64 + 0.5));
                    break;
                }
                if below.is_some_and(|id| world.registry.get(id).solid) {
                    break;
                }
            }
        }
    }

    /// Tries to spawn hostile creatures in dark spots 24–64 blocks from the player.
    pub fn spawn_hostiles(&mut self, world: &World, player: DVec3, difficulty: Difficulty) {
        if difficulty == Difficulty::Peaceful {
            self.list.retain(|m| self.defs.get(m.kind).category != Category::Hostile);
            return;
        }
        let hostiles =
            self.list.iter().filter(|m| self.defs.get(m.kind).category == Category::Hostile).count();
        if hostiles >= HOSTILE_CAP || !self.tick.is_multiple_of(20) {
            return;
        }
        let candidates: Vec<(MobKind, u32, [u32; 2], f64)> = self
            .defs
            .iter()
            .filter(|(_, d)| d.category == Category::Hostile && d.spawn.dark)
            .map(|(k, d)| (k, d.spawn.weight, d.spawn.group, d.height))
            .collect();
        let total: u32 = candidates.iter().map(|c| c.1).sum();
        if total == 0 {
            return;
        }
        let view = WorldView(world);
        for _ in 0..8 {
            let angle = self.rand() * std::f64::consts::TAU;
            let r = 24.0 + self.rand() * 40.0;
            let x = (player.x + angle.cos() * r).floor() as i32;
            let z = (player.z + angle.sin() * r).floor() as i32;
            let y = (player.y as i32 - 40 + self.range(0, 56)).clamp(-60, 300);
            let mut pick = (self.rand() * total as f64) as u32;
            let &(kind, _, group, h) = candidates
                .iter()
                .find(|c| {
                    if pick < c.1 {
                        true
                    } else {
                        pick -= c.1;
                        false
                    }
                })
                .expect("total > 0");
            let p = BlockPos::new(x, y, z);
            if !standable(&view, p, h.ceil() as i32) || view.is_water(p) || !roofed(world, p) {
                continue;
            }
            let n = self.range(group[0] as i32, group[1] as i32);
            for k in 0..n {
                let q = BlockPos::new(x + k % 2, y, z + k / 2);
                if standable(&view, q, h.ceil() as i32) {
                    self.spawn(kind, DVec3::new(q.x as f64 + 0.5, q.y as f64, q.z as f64 + 0.5));
                }
            }
            break;
        }
    }

    /// Removes creatures in unloaded areas, and hostiles far from the player
    /// (instantly beyond 128 blocks, now and then beyond 32 — like Minecraft).
    pub fn despawn(&mut self, world: &World, player: DVec3) {
        let defs = self.defs.clone();
        let mut r = self.rand();
        self.list.retain(|m| {
            r = (r * 9301.0 + 0.4929) % 1.0; // cheap per-creature randomness
            let loaded = world.is_loaded(feet_block(m.pos).chunk());
            let d = m.pos.distance(player);
            let hostile = defs.get(m.kind).category == Category::Hostile;
            loaded && !(hostile && (d > 128.0 || (d > 32.0 && r < 1.0 / 800.0)))
        });
    }
}

/// True if there is a solid block somewhere above `p` (no sky access).
fn roofed(world: &World, p: BlockPos) -> bool {
    for y in (p.y + 2)..320 {
        if world.map.block(BlockPos::new(p.x, y, p.z)).is_some_and(|id| world.registry.get(id).is_opaque()) {
            return true;
        }
    }
    false
}

fn feet_block(p: DVec3) -> BlockPos {
    BlockPos::new(p.x.floor() as i32, (p.y + 0.01).floor() as i32, p.z.floor() as i32)
}

/// Minecraft yaw facing along `dir`.
fn yaw_towards(dir: DVec3) -> f32 {
    (-dir.x).atan2(dir.z).to_degrees() as f32
}

/// Whether nothing solid blocks the straight line between two points.
fn line_of_sight(view: &WorldView, from: DVec3, to: DVec3) -> bool {
    let d = to - from;
    let n = (d.length() * 2.0).ceil() as i32;
    (1..n).all(|i| {
        let p = from + d * (i as f64 / n as f64);
        let b = BlockPos::new(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        view.0.map.block(b).is_some_and(|id| !view.0.registry.get(id).is_opaque())
    })
}

/// Direction toward the next path node (popping reached nodes), or `None` at the end.
fn follow_path(m: &mut Mob) -> Option<DVec3> {
    while let Some(&next) = m.path.first() {
        let target = DVec3::new(next.x as f64 + 0.5, next.y as f64, next.z as f64 + 0.5);
        let d = target - m.pos;
        if glam::DVec2::new(d.x, d.z).length() < 0.35 && d.y.abs() < 1.2 {
            m.path.remove(0);
            continue;
        }
        let dir = DVec3::new(d.x, 0.0, d.z).normalize_or_zero();
        m.yaw = yaw_towards(dir);
        return Some(dir);
    }
    None
}

/// Moves one creature: walking toward `dir`, gravity, swimming, jumping up
/// steps, wall-climbing spiders, collisions.
fn physics(m: &mut Mob, def: &MobDef, world: &World, dir: Option<DVec3>, speed_mod: f64) {
    let feet = feet_block(m.pos);
    let in_water = world.map.block(feet).is_some_and(|id| world.registry.get(id).name == "water");
    let fluid = if in_water { Fluid::Water } else { Fluid::None };
    if let Some(d) = dir {
        // Mob acceleration: speed attribute × modifier, scaled so a zombie walks ≈ 2.3 blocks/s.
        let accel =
            def.speed as f64 * speed_mod * if m.on_ground || fluid == Fluid::Water { 0.25 } else { 0.05 };
        m.vel.x += d.x * accel;
        m.vel.z += d.z * accel;
        if m.horizontal_collision && (m.on_ground || fluid == Fluid::Water) {
            m.vel.y = 0.42; // jump up a step
        }
        if m.horizontal_collision && def.name == "spider" {
            m.vel.y = 0.2; // spiders climb walls
        }
    }
    let requested = m.vel;
    let r = move_and_collide(world, &Aabb::from_feet(m.pos, def.width, def.height), requested);
    m.pos += r.moved;
    m.horizontal_collision = r.hit_x || r.hit_z;
    m.on_ground = r.landed(requested);
    if r.hit_x {
        m.vel.x = 0.0;
    }
    if r.hit_y {
        m.vel.y = 0.0;
    }
    if r.hit_z {
        m.vel.z = 0.0;
    }
    m.walk_dist += glam::DVec2::new(r.moved.x, r.moved.z).length() as f32;
    if fluid == Fluid::Water {
        m.vel *= 0.8;
        m.vel.y += 0.02; // creatures float and paddle upward in water
    } else {
        m.vel.y = (m.vel.y - 0.08) * 0.98;
        let f = if m.on_ground { 0.546 } else { 0.91 };
        m.vel.x *= f;
        m.vel.z *= f;
    }
}
