//! Things that move: creatures, items lying on the ground, and arrows.
//!
//! Fixed-size lists (no heap): at most [`MAX_MOBS`] creatures,
//! [`MAX_ITEMS`] items and [`MAX_ARROWS`] arrows exist at once, all inside
//! the loaded window around the player.
//!
//! Creatures follow the PC game's numbers (health, speed, damage, drops)
//! with simpler brains (no full path-finding): animals wander and run away
//! when hurt; zombies and spiders chase and hit; skeletons keep their
//! distance and shoot; spiders stay calm in daylight unless hurt; zombies
//! and skeletons burn in daylight. The Hollow wanders in the dark moving
//! blocks around "as if building something only it can see", and fights
//! hard if you hit it or break a block it placed.
//!
//! Movement uses Minecraft's rule for creatures: acceleration =
//! speed² on the ground (a zombie, speed 0.23, walks 2.3 m/s; the player
//! walks 4.3 m/s, so you can outrun one).

use crate::blocks::{self, AIR, Block, LAVA, LORE_TABLET, Layer};
use crate::inventory::Inventory;
use crate::items::{Item, Rng, Stack, item_def};
use crate::math::{cos, ifloor, sin, sqrt};
use crate::mobs::*;
use crate::player::collide_box;
use crate::survival::Cause;
use crate::world::{SIZE, World};
use crate::worldgen::{HEIGHT, column_info};

/// Most creatures at once.
pub const MAX_MOBS: usize = 16;
/// Most items lying on the ground at once.
pub const MAX_ITEMS: usize = 24;
/// Most arrows in flight at once.
pub const MAX_ARROWS: usize = 6;
/// How many Hollow-placed blocks are remembered (breaking one angers Hollows).
const MAX_HOLLOW_BLOCKS: usize = 32;
/// Animals kept around the player.
const PASSIVE_CAP: usize = 6;
/// Hostile (and neutral) creatures kept around the player.
/// (On PC monsters appear 24–128 blocks away and take a while to arrive;
/// here the loaded area is only 16 blocks wide each way, so they come
/// less often and fewer at once to keep a similar pace.)
const HOSTILE_CAP: usize = 5;
/// Items disappear after 5 minutes, as on PC.
const ITEM_LIFETIME: u32 = 6000;
/// Ticks a creature can't be hurt again after a hit (PC: 10).
const MOB_HURT_TICKS: u8 = 10;

/// One creature.
#[derive(Clone, Copy, Debug)]
pub struct Mob {
    /// Entry in use.
    pub used: bool,
    /// Creature kind (index into `MOBS`).
    pub kind: u8,
    /// Feet position.
    pub pos: [f32; 3],
    /// Velocity (blocks per tick).
    pub vel: [f32; 3],
    /// Facing (same convention as the player: 0 = +Z).
    pub yaw: f32,
    /// Health points.
    pub health: f32,
    /// Ticks of red flash / invulnerability left after a hit.
    pub hurt: u8,
    /// Standing on something.
    pub on_ground: bool,
    hit_wall: bool,
    /// Provoked (neutral) — chases and attacks.
    pub angry: bool,
    /// Hurt by the player (spiders then attack even in daylight).
    provoked: bool,
    panic: u16,
    goal: [f32; 2],
    has_goal: bool,
    think: u16,
    attack_cd: u16,
    /// Ticks on fire left.
    pub fire: u16,
    /// Walk animation phase (radians) and strength (0..1).
    pub walk: f32,
    /// How strongly the legs swing (0 standing, 1 walking).
    pub walk_amount: f32,
    /// Block carried by a Hollow (`AIR` = none).
    pub carried: Block,
    action: u16,
}

impl Mob {
    /// An unused entry.
    pub const EMPTY: Mob = Mob {
        used: false,
        kind: 0,
        pos: [0.0; 3],
        vel: [0.0; 3],
        yaw: 0.0,
        health: 0.0,
        hurt: 0,
        on_ground: false,
        hit_wall: false,
        angry: false,
        provoked: false,
        panic: 0,
        goal: [0.0; 2],
        has_goal: false,
        think: 0,
        attack_cd: 0,
        fire: 0,
        walk: 0.0,
        walk_amount: 0.0,
        carried: AIR,
        action: 200,
    };
}

/// An item lying on the ground.
#[derive(Clone, Copy, Debug)]
pub struct ItemEntity {
    /// Entry in use.
    pub used: bool,
    /// What it is.
    pub stack: Stack,
    /// Position (bottom centre).
    pub pos: [f32; 3],
    vel: [f32; 3],
    /// Age in ticks (also spins the picture).
    pub age: u32,
    delay: u16,
}

impl ItemEntity {
    const EMPTY: ItemEntity =
        ItemEntity { used: false, stack: Stack::EMPTY, pos: [0.0; 3], vel: [0.0; 3], age: 0, delay: 0 };
}

/// An arrow shot by a skeleton.
#[derive(Clone, Copy, Debug)]
pub struct Arrow {
    /// Entry in use.
    pub used: bool,
    /// Position of the tip.
    pub pos: [f32; 3],
    /// Velocity.
    pub vel: [f32; 3],
    age: u16,
    damage: f32,
    shooter: u8,
}

impl Arrow {
    const EMPTY: Arrow = Arrow { used: false, pos: [0.0; 3], vel: [0.0; 3], age: 0, damage: 0.0, shooter: 0 };
}

/// What happened to the player during a tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct Events {
    /// Strongest hit taken (before armour), 0 if none.
    pub damage: f32,
    /// What dealt it.
    pub cause: Option<Cause>,
    /// Knock-back to add to the player's velocity.
    pub knock: [f32; 3],
}

/// The player as creatures see it.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    /// Feet position.
    pub pos: [f32; 3],
    /// Whether creatures may attack (survival).
    pub attackable: bool,
}

/// All moving things.
pub struct Entities {
    /// Creatures.
    pub mobs: [Mob; MAX_MOBS],
    /// Items on the ground.
    pub items: [ItemEntity; MAX_ITEMS],
    /// Arrows.
    pub arrows: [Arrow; MAX_ARROWS],
    hollow_blocks: [[i32; 3]; MAX_HOLLOW_BLOCKS],
    hollow_next: usize,
    rng: Rng,
    timer: u32,
}

impl Default for Entities {
    fn default() -> Self {
        Self::new()
    }
}

/// Horizontal unit vector and length from `a` to `b`.
fn flat_dir(a: [f32; 3], b: [f32; 3]) -> ([f32; 2], f32) {
    let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
    let len = sqrt(dx * dx + dz * dz);
    if len < 1e-4 { ([0.0, 0.0], 0.0) } else { ([dx / len, dz / len], len) }
}

/// Facing angle for a horizontal direction (inverse of `(-sin, cos)`).
fn yaw_of(d: [f32; 2]) -> f32 {
    libm::atan2f(-d[0], d[1])
}

/// Whether nothing opaque lies between two points.
fn line_of_sight(world: &World, a: [f32; 3], b: [f32; 3]) -> bool {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len = sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
    let steps = (len * 3.0) as i32 + 1;
    (1..steps).all(|i| {
        let t = i as f32 / steps as f32;
        !blocks::is_opaque(world.get(
            ifloor(a[0] + d[0] * t),
            ifloor(a[1] + d[1] * t),
            ifloor(a[2] + d[2] * t),
        ))
    })
}

impl Entities {
    /// No entities. `const` so it can live in a static.
    pub const fn new() -> Self {
        Self {
            mobs: [Mob::EMPTY; MAX_MOBS],
            items: [ItemEntity::EMPTY; MAX_ITEMS],
            arrows: [Arrow::EMPTY; MAX_ARROWS],
            hollow_blocks: [[i32::MIN; 3]; MAX_HOLLOW_BLOCKS],
            hollow_next: 0,
            rng: Rng(0xC0FF_EE11),
            timer: 0,
        }
    }

    /// Removes everything (new world).
    pub fn clear(&mut self, seed: u32) {
        *self = Self::new();
        self.rng = Rng(seed | 1);
    }

    /// Adds a creature. Returns its index, or `None` if the list is full.
    pub fn spawn(&mut self, kind: u8, pos: [f32; 3]) -> Option<usize> {
        let i = self.mobs.iter().position(|m| !m.used)?;
        let yaw = self.rng.unit() * core::f32::consts::TAU;
        self.mobs[i] = Mob { used: true, kind, pos, yaw, health: mob_def(kind).health, ..Mob::EMPTY };
        Some(i)
    }

    /// Drops a stack on the ground at `pos` (merging with a nearby equal
    /// stack). The oldest item vanishes if the list is full. `delay` is how
    /// long before it can be picked up (PC: 10 ticks, 40 when thrown).
    pub fn drop_item(&mut self, stack: Stack, pos: [f32; 3], delay: u16) {
        if stack.is_empty() {
            return;
        }
        let max = item_def(stack.item).max_stack;
        for it in self
            .items
            .iter_mut()
            .filter(|it| it.used && it.stack.item == stack.item && it.stack.wear == stack.wear)
        {
            let (dx, dy, dz) = (it.pos[0] - pos[0], it.pos[1] - pos[1], it.pos[2] - pos[2]);
            if dx * dx + dy * dy + dz * dz < 2.0 && it.stack.count as u16 + stack.count as u16 <= max as u16 {
                it.stack.count += stack.count;
                return;
            }
        }
        let i = match self.items.iter().position(|it| !it.used) {
            Some(i) => i,
            None => (0..MAX_ITEMS).max_by_key(|&i| self.items[i].age).unwrap_or(0),
        };
        let vel = [(self.rng.unit() - 0.5) * 0.2, 0.2, (self.rng.unit() - 0.5) * 0.2];
        self.items[i] = ItemEntity { used: true, stack, pos, vel, age: 0, delay };
    }

    /// Throws a stack from the player's eye in direction `dir` (drop key).
    pub fn throw_item(&mut self, stack: Stack, eye: [f32; 3], dir: [f32; 3]) {
        self.drop_item(stack, [eye[0], eye[1] - 0.3, eye[2]], 40);
        if let Some(it) = self.items.iter_mut().filter(|it| it.used && it.age == 0).last() {
            it.vel = [dir[0] * 0.3, dir[1] * 0.3 + 0.1, dir[2] * 0.3];
        }
    }

    /// Picks up items close to the player into `inv`. Returns true if
    /// anything was picked up.
    pub fn pickup(&mut self, player: [f32; 3], inv: &mut Inventory) -> bool {
        let mut any = false;
        for it in self.items.iter_mut().filter(|it| it.used && it.age >= it.delay as u32) {
            let (dx, dy, dz) = (it.pos[0] - player[0], it.pos[1] - player[1], it.pos[2] - player[2]);
            if dx.abs() < 1.3 && dz.abs() < 1.3 && (-0.6..2.3).contains(&dy) {
                let left = inv.add(it.stack);
                if left < it.stack.count {
                    any = true;
                }
                it.stack.count = left;
                if left == 0 {
                    it.used = false;
                }
            }
        }
        any
    }

    /// Remembers a block placed by a Hollow.
    fn remember_hollow_block(&mut self, p: [i32; 3]) {
        self.hollow_blocks[self.hollow_next] = p;
        self.hollow_next = (self.hollow_next + 1) % MAX_HOLLOW_BLOCKS;
    }

    /// The player broke the block at `p`: if a Hollow placed it, every
    /// Hollow within 16 blocks becomes angry. Returns true if that happened.
    pub fn block_broken(&mut self, p: [i32; 3]) -> bool {
        let Some(k) = self.hollow_blocks.iter().position(|&h| h == p) else { return false };
        self.hollow_blocks[k] = [i32::MIN; 3];
        let c = [p[0] as f32, p[1] as f32, p[2] as f32];
        let mut any = false;
        for m in self.mobs.iter_mut().filter(|m| m.used && m.kind == HOLLOW) {
            let (dx, dz) = (m.pos[0] - c[0], m.pos[2] - c[2]);
            if dx * dx + dz * dz < 256.0 {
                m.angry = true;
                any = true;
            }
        }
        any
    }

    /// The creature hit by a ray from `eye` along unit `dir` within `reach`,
    /// and the distance to it.
    pub fn pick(&self, eye: [f32; 3], dir: [f32; 3], reach: f32) -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        for (i, m) in self.mobs.iter().enumerate().filter(|(_, m)| m.used) {
            let d = mob_def(m.kind);
            let h = d.width / 2.0;
            let min = [m.pos[0] - h, m.pos[1], m.pos[2] - h];
            let max = [m.pos[0] + h, m.pos[1] + d.height, m.pos[2] + h];
            let (mut t0, mut t1) = (0.0f32, reach);
            let mut hit = true;
            for a in 0..3 {
                if dir[a].abs() < 1e-6 {
                    if eye[a] < min[a] || eye[a] > max[a] {
                        hit = false;
                    }
                    continue;
                }
                let (mut ta, mut tb) = ((min[a] - eye[a]) / dir[a], (max[a] - eye[a]) / dir[a]);
                if ta > tb {
                    core::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
            }
            if hit && t0 <= t1 && best.is_none_or(|(_, bt)| t0 < bt) {
                best = Some((i, t0));
            }
        }
        best
    }

    /// Hurts creature `i` by `amount`, knocking it away from `from`.
    /// Returns true if it died (its drops are then on the ground).
    pub fn hurt(&mut self, i: usize, amount: f32, from: Option<[f32; 3]>) -> bool {
        let m = &mut self.mobs[i];
        if !m.used || m.hurt > 0 {
            return false;
        }
        m.health -= amount;
        m.hurt = MOB_HURT_TICKS;
        if let Some(f) = from {
            let (d, _) = flat_dir(f, m.pos);
            m.vel[0] += d[0] * 0.4;
            m.vel[2] += d[1] * 0.4;
            if m.on_ground {
                m.vel[1] = 0.36;
            }
            m.provoked = true;
            m.angry = true;
            m.panic = 100;
        }
        if m.health > 0.0 {
            return false;
        }
        let (kind, pos, carried) = (m.kind, m.pos, m.carried);
        m.used = false;
        let mut rng = self.rng;
        let drops = mob_def(kind).drops;
        for d in drops {
            if d.chance < 1.0 && rng.unit() >= d.chance {
                continue;
            }
            let n = if d.max > d.min {
                d.min + (rng.next_u32() % (d.max - d.min + 1) as u32) as u8
            } else {
                d.min
            };
            if n > 0 {
                self.drop_item(Stack::new(d.item, n), [pos[0], pos[1] + 0.5, pos[2]], 10);
            }
        }
        if carried != AIR {
            self.drop_item(Stack::new(carried as Item, 1), [pos[0], pos[1] + 0.5, pos[2]], 10);
        }
        self.rng = rng;
        true
    }

    /// Advances everything one tick. `daylight` is 0 (night) to 1 (day).
    pub fn tick(&mut self, world: &mut World, player: Target, daylight: f32) -> Events {
        let mut ev = Events::default();
        self.timer = self.timer.wrapping_add(1);
        if self.timer.is_multiple_of(40) {
            self.spawn_tick(world, player.pos, daylight);
        }
        for i in 0..MAX_MOBS {
            if self.mobs[i].used {
                self.tick_mob(i, world, player, daylight, &mut ev);
            }
        }
        self.tick_items(world);
        self.tick_arrows(world, player, &mut ev);
        ev
    }

    fn tick_mob(&mut self, i: usize, world: &mut World, player: Target, daylight: f32, ev: &mut Events) {
        let mut m = self.mobs[i];
        let d = mob_def(m.kind);
        // Leave the world when outside the loaded window, or far away (hostile).
        let (to_player, dist) = flat_dir(m.pos, player.pos);
        let far = dist > (SIZE / 2) as f32 - 1.0;
        if far || !world.is_loaded(ifloor(m.pos[0]), ifloor(m.pos[2])) || m.pos[1] < -10.0 {
            self.mobs[i].used = false;
            return;
        }
        m.hurt = m.hurt.saturating_sub(1);
        m.attack_cd = m.attack_cd.saturating_sub(1);

        let feet = world.get(ifloor(m.pos[0]), ifloor(m.pos[1] + 0.3), ifloor(m.pos[2]));
        let in_water = blocks::def(feet).layer == Layer::Water;
        let in_lava = feet == LAVA;
        let head_y = ifloor(m.pos[1] + d.height * 0.9);
        let bright = daylight > 0.55 && world.sees_sky(ifloor(m.pos[0]), head_y, ifloor(m.pos[2]));

        // --- Decide where to go.
        let chase = player.attackable
            && dist < 16.0
            && match d.category {
                Category::Hostile => !d.calm_in_light || m.provoked || !bright,
                Category::Neutral => m.angry,
                Category::Passive => false,
            };
        let mut wish = [0.0f32; 2];
        let mut speed = d.speed;
        let mut jump = false;
        if chase {
            m.yaw = yaw_of(to_player);
            if m.kind == HOLLOW {
                speed *= d.angry_speed;
            }
            if d.ranged {
                // Keep 6–12 blocks away and shoot when in sight.
                if dist < 6.0 {
                    wish = [-to_player[0], -to_player[1]];
                } else if dist > 12.0 {
                    wish = to_player;
                }
                let eye = [m.pos[0], m.pos[1] + d.height * 0.85, m.pos[2]];
                let target = [player.pos[0], player.pos[1] + 1.4, player.pos[2]];
                if m.attack_cd == 0 && line_of_sight(world, eye, target) {
                    self.shoot(eye, target, dist, d.attack, m.kind);
                    m.attack_cd = 40;
                }
            } else {
                wish = to_player;
                let dy = player.pos[1] - m.pos[1];
                let reach = d.width / 2.0 + 1.0;
                if dist < reach && dy.abs() < 1.6 && m.attack_cd == 0 {
                    if d.attack > ev.damage {
                        ev.damage = d.attack;
                        ev.cause = Some(Cause::Mob(m.kind));
                        ev.knock = [to_player[0] * 0.4, 0.3, to_player[1] * 0.4];
                    }
                    m.attack_cd = 20;
                }
                // Spiders leap at their prey.
                if m.kind == SPIDER
                    && m.on_ground
                    && (2.0..6.0).contains(&dist)
                    && self.rng.next_u32().is_multiple_of(10)
                {
                    m.vel[1] = 0.4;
                    m.vel[0] += to_player[0] * 0.4;
                    m.vel[2] += to_player[1] * 0.4;
                }
            }
        } else {
            if m.panic > 0 {
                m.panic -= 1;
                speed *= 2.0;
                if !m.has_goal || m.think == 0 {
                    // Run somewhere away from here.
                    let a = self.rng.unit() * core::f32::consts::TAU;
                    m.goal = [m.pos[0] + sin(a) * 8.0, m.pos[2] + cos(a) * 8.0];
                    m.has_goal = true;
                    m.think = 30;
                }
            }
            if m.think == 0 {
                m.think = 60 + (self.rng.next_u32() % 140) as u16;
                if self.rng.next_u32() & 1 == 0 {
                    let a = self.rng.unit() * core::f32::consts::TAU;
                    let r = 2.0 + self.rng.unit() * 5.0;
                    m.goal = [m.pos[0] + sin(a) * r, m.pos[2] + cos(a) * r];
                    m.has_goal = true;
                } else {
                    m.has_goal = false;
                }
            }
            m.think = m.think.saturating_sub(1);
            if m.has_goal {
                let (dir, len) = flat_dir(m.pos, [m.goal[0], 0.0, m.goal[1]]);
                if len < 0.8 {
                    m.has_goal = false;
                } else {
                    wish = dir;
                    m.yaw = yaw_of(dir);
                }
            }
            if m.kind == HOLLOW {
                self.hollow_work(&mut m, world, player.pos);
            }
        }
        // Don't walk off cliffs or into lava.
        if wish != [0.0, 0.0] && !safe_ahead(world, m.pos, wish) && !chase {
            wish = [0.0, 0.0];
            m.has_goal = false;
        }
        if m.hit_wall && wish != [0.0, 0.0] {
            jump = true;
        }

        // --- Fire, lava, daylight.
        if in_water {
            m.fire = 0;
        } else if d.burns_in_daylight && bright {
            m.fire = m.fire.max(100);
        }
        let mut damage = 0.0;
        if in_lava {
            m.fire = 300;
            damage = 4.0;
        } else if m.fire > 0 {
            m.fire -= 1;
            if m.fire.is_multiple_of(20) {
                damage = 1.0;
            }
        }

        // --- Move (Minecraft creature physics).
        let (half, h) = (d.width / 2.0 - 0.01, d.height);
        if in_water || in_lava {
            let drag = if in_lava { 0.5 } else { 0.8 };
            m.vel[0] += wish[0] * 0.02;
            m.vel[2] += wish[1] * 0.02;
            m.vel[1] += 0.04; // creatures paddle up
            move_box(world, &mut m.pos, &mut m.vel, half, h, &mut m.on_ground, &mut m.hit_wall);
            m.vel[0] *= drag;
            m.vel[2] *= drag;
            m.vel[1] = m.vel[1] * drag - 0.02;
        } else {
            let accel = if m.on_ground { speed * speed } else { 0.02 * speed };
            m.vel[0] += wish[0] * accel;
            m.vel[2] += wish[1] * accel;
            if jump && m.on_ground {
                m.vel[1] = 0.42;
            }
            move_box(world, &mut m.pos, &mut m.vel, half, h, &mut m.on_ground, &mut m.hit_wall);
            let f = if m.on_ground { 0.546 } else { 0.91 };
            m.vel[0] *= f;
            m.vel[2] *= f;
            m.vel[1] = (m.vel[1] - 0.08) * 0.98;
        }
        let moved = sqrt(m.vel[0] * m.vel[0] + m.vel[2] * m.vel[2]);
        m.walk += moved * 6.0;
        m.walk_amount = (moved * 8.0).min(1.0);
        self.mobs[i] = m;
        if damage > 0.0 {
            self.hurt(i, damage, None);
        }
    }

    /// The Hollow's block moving: every so often pick up a nearby block it
    /// likes, or put the carried one down somewhere close.
    fn hollow_work(&mut self, m: &mut Mob, world: &mut World, player: [f32; 3]) {
        if m.action > 0 {
            m.action -= 1;
            return;
        }
        m.action = 200 + (self.rng.next_u32() % 300) as u16;
        let d = mob_def(m.kind);
        let (bx, by, bz) = (ifloor(m.pos[0]), ifloor(m.pos[1]), ifloor(m.pos[2]));
        for _ in 0..8 {
            let x = bx + (self.rng.next_u32() % 5) as i32 - 2;
            let z = bz + (self.rng.next_u32() % 5) as i32 - 2;
            let y = by + (self.rng.next_u32() % 3) as i32 - 1;
            if !world.is_loaded(x, z) {
                continue;
            }
            if m.carried == AIR {
                let b = world.get(x, y, z);
                if b != LORE_TABLET && d.carries.contains(&(b as Item)) && world.get(x, y + 1, z) == AIR {
                    world.set(x, y, z, AIR);
                    m.carried = b;
                    return;
                }
            } else {
                let free = world.get(x, y, z) == AIR && blocks::def(world.get(x, y - 1, z)).solid;
                let near_player =
                    (x as f32 + 0.5 - player[0]).abs() < 1.0 && (z as f32 + 0.5 - player[2]).abs() < 1.0;
                let inside_self = x == bx && z == bz;
                if free && !near_player && !inside_self {
                    world.set(x, y, z, m.carried);
                    m.carried = AIR;
                    self.remember_hollow_block([x, y, z]);
                    return;
                }
            }
        }
    }

    fn shoot(&mut self, from: [f32; 3], to: [f32; 3], dist: f32, damage: f32, shooter: u8) {
        let Some(i) = self.arrows.iter().position(|a| !a.used) else { return };
        // Aim a little high to make up for gravity, as skeletons do on PC.
        let d = [to[0] - from[0], to[1] - from[1] + dist * 0.12, to[2] - from[2]];
        let len = sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).max(1e-3);
        let s = 1.6 / len;
        self.arrows[i] =
            Arrow { used: true, pos: from, vel: [d[0] * s, d[1] * s, d[2] * s], age: 0, damage, shooter };
    }

    fn tick_items(&mut self, world: &World) {
        for it in self.items.iter_mut().filter(|it| it.used) {
            it.age += 1;
            if it.age > ITEM_LIFETIME || !world.is_loaded(ifloor(it.pos[0]), ifloor(it.pos[2])) {
                it.used = false;
                continue;
            }
            let b = world.get(ifloor(it.pos[0]), ifloor(it.pos[1] + 0.1), ifloor(it.pos[2]));
            if blocks::def(b).layer == Layer::Water {
                it.vel[1] += 0.05; // items float
            }
            it.vel[1] -= 0.04;
            let (mut ground, mut wall) = (false, false);
            move_box(world, &mut it.pos, &mut it.vel, 0.125, 0.25, &mut ground, &mut wall);
            let f = if ground { 0.6 * 0.98 } else { 0.98 };
            it.vel[0] *= f;
            it.vel[2] *= f;
            it.vel[1] *= 0.98;
        }
    }

    fn tick_arrows(&mut self, world: &World, player: Target, ev: &mut Events) {
        for a in self.arrows.iter_mut().filter(|a| a.used) {
            a.age += 1;
            if a.age > 200 {
                a.used = false;
                continue;
            }
            // Step in small pieces so arrows don't pass through walls.
            for _ in 0..4 {
                for k in 0..3 {
                    a.pos[k] += a.vel[k] / 4.0;
                }
                if blocks::def(world.get(ifloor(a.pos[0]), ifloor(a.pos[1]), ifloor(a.pos[2]))).solid {
                    a.used = false;
                    break;
                }
                let p = player.pos;
                let inside = (a.pos[0] - p[0]).abs() < 0.4
                    && (a.pos[2] - p[2]).abs() < 0.4
                    && a.pos[1] > p[1]
                    && a.pos[1] < p[1] + 1.9;
                if inside && player.attackable {
                    if a.damage > ev.damage {
                        ev.damage = a.damage;
                        ev.cause = Some(Cause::Mob(a.shooter));
                        let (d, _) = flat_dir([0.0; 3], [a.vel[0], 0.0, a.vel[2]]);
                        ev.knock = [d[0] * 0.3, 0.2, d[1] * 0.3];
                    }
                    a.used = false;
                    break;
                }
            }
            a.vel[0] *= 0.99;
            a.vel[2] *= 0.99;
            a.vel[1] = a.vel[1] * 0.99 - 0.05;
        }
    }

    /// Creatures appear around the player: animals on grass in their
    /// biomes, hostile creatures and Hollows in the dark (night or caves).
    fn spawn_tick(&mut self, world: &World, player: [f32; 3], daylight: f32) {
        let passive =
            self.mobs.iter().filter(|m| m.used && mob_def(m.kind).category == Category::Passive).count();
        let hostile =
            self.mobs.iter().filter(|m| m.used && mob_def(m.kind).category != Category::Passive).count();
        let (px, pz) = (ifloor(player[0]), ifloor(player[2]));
        // Animals: rarely, on grass, in their biomes.
        if passive < PASSIVE_CAP && self.rng.next_u32().is_multiple_of(4) {
            let (x, z) = self.random_column(px, pz, 8, 14);
            if let Some(y) = surface(world, x, z) {
                let top = world.get(x, y - 1, z);
                if matches!(top, blocks::GRASS_BLOCK | blocks::SNOWY_GRASS_BLOCK) {
                    let biome = column_info(world.seed, x, z).biome;
                    let kind =
                        self.pick_kind(|d| d.category == Category::Passive && d.surface.contains(&biome));
                    if let Some(k) = kind {
                        self.spawn_group(world, k, x, y, z, PASSIVE_CAP - passive);
                    }
                }
            }
        }
        // Monsters: in the dark, not too close.
        if hostile < HOSTILE_CAP && self.timer.is_multiple_of(120) {
            for _ in 0..3 {
                let (x, z) = self.random_column(px, pz, 12, 15);
                // At night try the surface; otherwise a random height (caves).
                let y = match surface(world, x, z) {
                    Some(s) if daylight < 0.3 && self.rng.next_u32() & 1 == 0 => s,
                    _ => 1 + (self.rng.next_u32() % (HEIGHT as u32 - 3)) as i32,
                };
                let ok = world.get(x, y, z) == AIR
                    && world.get(x, y + 1, z) == AIR
                    && blocks::is_opaque(world.get(x, y - 1, z))
                    && (!world.sees_sky(x, y, z) || daylight < 0.3);
                if ok {
                    if let Some(k) = self.pick_kind(|d| d.dark) {
                        self.spawn_group(world, k, x, y, z, HOSTILE_CAP - hostile);
                    }
                    break;
                }
            }
        }
    }

    fn random_column(&mut self, px: i32, pz: i32, min: i32, max: i32) -> (i32, i32) {
        let a = self.rng.unit() * core::f32::consts::TAU;
        let r = min as f32 + self.rng.unit() * (max - min) as f32;
        (px + (sin(a) * r) as i32, pz + (cos(a) * r) as i32)
    }

    /// Picks a creature kind by spawn weight among those `allowed`.
    fn pick_kind(&mut self, allowed: impl Fn(&MobDef) -> bool) -> Option<u8> {
        let total: u32 = MOBS.iter().filter(|d| allowed(d)).map(|d| d.weight as u32).sum();
        if total == 0 {
            return None;
        }
        let mut r = self.rng.next_u32() % total;
        for (k, d) in MOBS.iter().enumerate().filter(|(_, d)| allowed(d)) {
            if r < d.weight as u32 {
                return Some(k as u8);
            }
            r -= d.weight as u32;
        }
        None
    }

    fn spawn_group(&mut self, world: &World, kind: u8, x: i32, y: i32, z: i32, room: usize) {
        let (lo, hi) = mob_def(kind).group;
        let n = (lo as u32 + self.rng.next_u32() % (hi - lo + 1) as u32) as usize;
        for k in 0..n.min(room) {
            let (gx, gz) = if k == 0 {
                (x, z)
            } else {
                (x + (self.rng.next_u32() % 5) as i32 - 2, z + (self.rng.next_u32() % 5) as i32 - 2)
            };
            let gy = if k == 0 {
                y
            } else {
                (y - 1..=y + 1)
                    .find(|&yy| {
                        world.get(gx, yy, gz) == AIR
                            && world.get(gx, yy + 1, gz) == AIR
                            && blocks::def(world.get(gx, yy - 1, gz)).solid
                    })
                    .unwrap_or(-1)
            };
            if gy >= 0 {
                self.spawn(kind, [gx as f32 + 0.5, gy as f32, gz as f32 + 0.5]);
            }
        }
    }
}

/// The first air cell above the highest solid block of a column.
fn surface(world: &World, x: i32, z: i32) -> Option<i32> {
    if !world.is_loaded(x, z) {
        return None;
    }
    let top = (0..HEIGHT as i32 - 1).rev().find(|&y| blocks::def(world.get(x, y, z)).solid)?;
    (world.get(x, top + 1, z) == AIR && world.get(x, top + 2, z) == AIR).then_some(top + 1)
}

/// Whether stepping one block in direction `d` is safe (no drop of more
/// than 3 blocks, no lava).
fn safe_ahead(world: &World, pos: [f32; 3], d: [f32; 2]) -> bool {
    let (x, z) = (ifloor(pos[0] + d[0] * 0.8), ifloor(pos[2] + d[1] * 0.8));
    let y = ifloor(pos[1] + 0.1);
    for k in 0..4 {
        let b = world.get(x, y - k, z);
        if b == LAVA {
            return false;
        }
        if blocks::def(b).solid {
            return true;
        }
    }
    false
}

/// Moves an upright box by its velocity with collisions (Y, then X, then Z).
fn move_box(
    world: &World,
    pos: &mut [f32; 3],
    vel: &mut [f32; 3],
    half: f32,
    h: f32,
    on_ground: &mut bool,
    hit_wall: &mut bool,
) {
    let want = *vel;
    let dy = collide_box(world, *pos, half, h, 1, want[1]);
    pos[1] += dy;
    let dx = collide_box(world, *pos, half, h, 0, want[0]);
    pos[0] += dx;
    let dz = collide_box(world, *pos, half, h, 2, want[2]);
    pos[2] += dz;
    *on_ground = want[1] < 0.0 && dy > want[1];
    *hit_wall = dx != want[0] || dz != want[2];
    if dy != want[1] {
        vel[1] = 0.0;
    }
    if dx != want[0] {
        vel[0] = 0.0;
    }
    if dz != want[2] {
        vel[2] = 0.0;
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::blocks::*;
    use std::boxed::Box;

    /// A flat stone world at y = 10 (feet at 11) around the origin.
    fn flat() -> Box<World> {
        let mut w = Box::new(World::new());
        w.reset(5);
        w.update(0, 0, usize::MAX);
        for x in -16..16 {
            for z in -16..16 {
                for y in 0..HEIGHT as i32 {
                    w.set(x, y, z, if y <= 10 { STONE } else { AIR });
                }
            }
        }
        w
    }

    fn player_at(x: f32, z: f32) -> Target {
        Target { pos: [x, 11.0, z], attackable: true }
    }

    #[test]
    fn zombie_walks_to_the_player_and_hits() {
        let mut w = flat();
        let mut e = Box::new(Entities::new());
        e.timer = 1; // keep the spawner quiet for this test
        let i = e.spawn(ZOMBIE, [0.5, 11.0, 0.5]).unwrap();
        let mut hit = 0.0;
        for t in 0..400 {
            e.timer = 1 + t * 40 + 1;
            let ev = e.tick(&mut w, player_at(8.5, 0.5), 0.0);
            hit = f32::max(hit, ev.damage);
        }
        assert!(e.mobs[i].pos[0] > 6.0, "zombie at {:?}", e.mobs[i].pos);
        assert_eq!(hit, 3.0);
    }

    #[test]
    fn zombie_speed_is_minecraft_like() {
        let mut w = flat();
        let mut e = Box::new(Entities::new());
        // Within 15 blocks of the player, or it would leave the loaded area.
        let i = e.spawn(ZOMBIE, [-6.5, 11.0, 0.5]).unwrap();
        for t in 0..20 {
            e.timer = t * 40 + 1;
            e.tick(&mut w, player_at(7.5, 0.5), 0.0);
        }
        let x0 = e.mobs[i].pos[0];
        for t in 0..20 {
            e.timer = t * 40 + 1;
            e.tick(&mut w, player_at(7.5, 0.5), 0.0);
        }
        let speed = e.mobs[i].pos[0] - x0; // blocks per second
        assert!((1.8..2.8).contains(&speed), "zombie speed {speed} m/s");
    }

    #[test]
    fn killing_drops_loot_and_items_get_picked_up() {
        let mut w = flat();
        let mut e = Box::new(Entities::new());
        let i = e.spawn(COW, [3.5, 11.0, 0.5]).unwrap();
        let mut died = false;
        for _ in 0..20 {
            died |= e.hurt(i, 4.0, Some([0.5, 11.0, 0.5]));
            for k in 0..10 {
                e.timer = k * 40 + 1;
                e.tick(&mut w, Target { pos: [0.5, 11.0, 0.5], attackable: false }, 1.0);
            }
            if died {
                break;
            }
        }
        assert!(died);
        let beef = e.items.iter().filter(|it| it.used && it.stack.item == crate::items::BEEF).count();
        assert!(beef > 0, "cows always drop beef");
        // Walk onto the items and pick them up.
        let spot = e.items.iter().find(|it| it.used).unwrap().pos;
        for k in 0..20 {
            e.timer = k * 40 + 1;
            e.tick(&mut w, Target { pos: spot, attackable: false }, 1.0);
        }
        let mut inv = Inventory::new();
        assert!(e.pickup([spot[0], 11.0, spot[2]], &mut inv));
        assert!(inv.count(crate::items::BEEF) >= 1);
    }

    #[test]
    fn hollow_moves_blocks_and_gets_angry() {
        let mut w = flat();
        let mut e = Box::new(Entities::new());
        let i = e.spawn(HOLLOW, [0.5, 11.0, 0.5]).unwrap();
        // Give it something to carry: dirt around it.
        for x in -2..=2 {
            for z in -2..=2 {
                w.set(x, 10, z, DIRT);
            }
        }
        let mut carried = false;
        for t in 0..3000 {
            e.timer = t * 40 + 1;
            e.tick(&mut w, player_at(10.5, 0.5), 0.0);
            carried |= e.mobs[i].carried != AIR;
        }
        assert!(carried, "the Hollow picked something up");
        // Hitting one makes only that one angry.
        e.mobs = [Mob::EMPTY; MAX_MOBS];
        let a = e.spawn(HOLLOW, [2.5, 11.0, 0.5]).unwrap();
        let b = e.spawn(HOLLOW, [4.5, 11.0, 0.5]).unwrap();
        assert!(!e.mobs[a].angry);
        e.hurt(a, 1.0, Some([10.5, 11.0, 0.5]));
        assert!(e.mobs[a].angry && !e.mobs[b].angry);
    }

    #[test]
    fn skeleton_shoots() {
        let mut w = flat();
        let mut e = Box::new(Entities::new());
        e.spawn(SKELETON, [0.5, 11.0, 0.5]).unwrap();
        let mut hit = false;
        for t in 0..200 {
            e.timer = t * 40 + 1;
            hit |= e.tick(&mut w, player_at(9.5, 0.5), 0.0).damage > 0.0;
        }
        assert!(hit, "an arrow reached the player");
    }
}
