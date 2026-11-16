//! The player's body: movement, gravity, swimming, flying and collisions.
//!
//! Runs at Minecraft's 20 ticks per second with Minecraft's numbers (walk
//! 4.3 m/s, sprint x1.3, jump 0.42, gravity 0.08 with 2 % drag per tick,
//! 0.6 x 1.8 hitbox, eyes at 1.62), so jumps, falls and speeds feel the same
//! as on PC.

use crate::blocks::{Layer, WATER, def};
use crate::math::{cos, ifloor, sin};
use crate::world::World;

/// Half the hitbox width.
pub const HALF_WIDTH: f32 = 0.3;
/// Hitbox height.
pub const BODY_HEIGHT: f32 = 1.8;
/// Eye height above the feet.
pub const EYE_HEIGHT: f32 = 1.62;
/// Eye height while sneaking.
pub const SNEAK_EYE_HEIGHT: f32 = 1.27;

/// What the player wants to do this tick.
#[derive(Clone, Copy, Default)]
pub struct MoveInput {
    /// +1 forward, -1 backward.
    pub forward: f32,
    /// +1 right, -1 left.
    pub strafe: f32,
    /// Jump / swim up / fly up.
    pub jump: bool,
    /// Sneak / fly down.
    pub sneak: bool,
    /// Sprint.
    pub sprint: bool,
}

/// The player's position and motion.
#[derive(Clone, Copy)]
pub struct Player {
    /// Feet position (centre of the bottom of the hitbox).
    pub pos: [f32; 3],
    /// Velocity in blocks per tick.
    pub vel: [f32; 3],
    /// Horizontal look angle in radians (0 = towards +Z, increases to the right... towards -X).
    pub yaw: f32,
    /// Vertical look angle in radians (positive = up).
    pub pitch: f32,
    /// Standing on something.
    pub on_ground: bool,
    /// In water (swim physics).
    pub in_water: bool,
    /// Flying (creative mode).
    pub flying: bool,
    /// Sneaking (affects eye height).
    pub sneaking: bool,
    /// Bumped into a wall during the last tick.
    pub hit_wall: bool,
}

impl Player {
    /// A player standing at `pos`.
    pub const fn new(pos: [f32; 3]) -> Self {
        Self {
            pos,
            vel: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            in_water: false,
            flying: false,
            sneaking: false,
            hit_wall: false,
        }
    }

    /// Eye position.
    pub fn eye(&self) -> [f32; 3] {
        let h = if self.sneaking { SNEAK_EYE_HEIGHT } else { EYE_HEIGHT };
        [self.pos[0], self.pos[1] + h, self.pos[2]]
    }

    /// Unit vector the player looks along.
    pub fn look_dir(&self) -> [f32; 3] {
        let cp = cos(self.pitch);
        [-sin(self.yaw) * cp, sin(self.pitch), cos(self.yaw) * cp]
    }

    /// Advances one tick (1/20 s).
    pub fn tick(&mut self, world: &World, input: MoveInput, auto_jump: bool) {
        self.sneaking = input.sneak && !self.flying;
        let feet = world.get(ifloor(self.pos[0]), ifloor(self.pos[1] + 0.4), ifloor(self.pos[2]));
        self.in_water = def(feet).layer == Layer::Water || feet == WATER;

        // Wished horizontal direction in world space.
        let (fx, fz) = (-sin(self.yaw), cos(self.yaw));
        let (rx, rz) = (-cos(self.yaw), -sin(self.yaw));
        let mut wx = input.forward * fx + input.strafe * rx;
        let mut wz = input.forward * fz + input.strafe * rz;
        let len = crate::math::sqrt(wx * wx + wz * wz);
        if len > 1.0 {
            wx /= len;
            wz /= len;
        }
        // Minecraft scales strafing input by 0.98 and sneaking by 0.3.
        let mut speed_mul = 0.98;
        if self.sneaking {
            speed_mul *= 0.3;
        }

        if self.flying {
            let accel = 0.05 * if input.sprint { 2.0 } else { 1.0 };
            self.vel[0] += wx * accel * speed_mul;
            self.vel[2] += wz * accel * speed_mul;
            self.vel[1] = if input.jump {
                0.375
            } else if input.sneak {
                -0.375
            } else {
                self.vel[1] * 0.6
            };
            self.move_by(world);
            self.vel[0] *= 0.91;
            self.vel[2] *= 0.91;
            if self.on_ground && input.sneak {
                self.flying = false;
            }
            return;
        }

        if self.in_water {
            let accel = 0.02;
            self.vel[0] += wx * accel * speed_mul;
            self.vel[2] += wz * accel * speed_mul;
            if input.jump {
                self.vel[1] += 0.04;
            }
            self.move_by(world);
            self.vel[0] *= 0.8;
            self.vel[2] *= 0.8;
            self.vel[1] = self.vel[1] * 0.8 - 0.02;
            // Hop out onto a ledge when swimming against it.
            if self.hit_wall && input.jump {
                self.vel[1] = 0.3;
            }
            return;
        }

        // Walking: 0.1 acceleration on the ground (0.13 sprinting), 0.02 in
        // the air; friction 0.546 on the ground (0.6 slipperiness x 0.91).
        let accel = if self.on_ground { 0.1 } else { 0.02 } * if input.sprint { 1.3 } else { 1.0 };
        self.vel[0] += wx * accel * speed_mul;
        self.vel[2] += wz * accel * speed_mul;
        let wants_move = input.forward != 0.0 || input.strafe != 0.0;
        // Auto-jump uses last tick's wall bump, so the jump starts before
        // this tick's movement (as a real jump does).
        let bumped = auto_jump && self.hit_wall && wants_move && !self.sneaking;
        if (input.jump || bumped) && self.on_ground {
            self.jump(input.sprint);
        }
        self.move_by(world);
        let friction = if self.on_ground { 0.546 } else { 0.91 };
        self.vel[0] *= friction;
        self.vel[2] *= friction;
        self.vel[1] = (self.vel[1] - 0.08) * 0.98;
    }

    fn jump(&mut self, sprint: bool) {
        self.vel[1] = 0.42;
        if sprint {
            // Sprint-jumping gives a small forward boost, as on PC.
            self.vel[0] += -sin(self.yaw) * 0.2;
            self.vel[2] += cos(self.yaw) * 0.2;
        }
    }

    /// Moves by the current velocity, stopping at solid blocks: first along
    /// Y, then X, then Z, like Minecraft.
    fn move_by(&mut self, world: &World) {
        let wanted = self.vel;
        let dy = collide(world, self.pos, 1, wanted[1]);
        self.pos[1] += dy;
        let dx = collide(world, self.pos, 0, wanted[0]);
        self.pos[0] += dx;
        let dz = collide(world, self.pos, 2, wanted[2]);
        self.pos[2] += dz;
        self.on_ground = wanted[1] < 0.0 && dy > wanted[1];
        self.hit_wall = dx != wanted[0] || dz != wanted[2];
        if dy != wanted[1] {
            self.vel[1] = 0.0;
        }
        if dx != wanted[0] {
            self.vel[0] = 0.0;
        }
        if dz != wanted[2] {
            self.vel[2] = 0.0;
        }
        let b = crate::world::BORDER as f32;
        self.pos[0] = self.pos[0].clamp(-b, b);
        self.pos[2] = self.pos[2].clamp(-b, b);
    }

    /// Whether the block cell `(x, y, z)` overlaps the player's hitbox.
    pub fn overlaps_block(&self, x: i32, y: i32, z: i32) -> bool {
        let (min, max) = bounds(self.pos);
        (x as f32) < max[0]
            && (x + 1) as f32 > min[0]
            && (y as f32) < max[1]
            && (y + 1) as f32 > min[1]
            && (z as f32) < max[2]
            && (z + 1) as f32 > min[2]
    }
}

/// Hitbox corners for feet position `p`.
fn bounds(p: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    ([p[0] - HALF_WIDTH, p[1], p[2] - HALF_WIDTH], [p[0] + HALF_WIDTH, p[1] + BODY_HEIGHT, p[2] + HALF_WIDTH])
}

/// How far the hitbox at `pos` can move by `delta` along `axis` before
/// touching a solid block.
fn collide(world: &World, pos: [f32; 3], axis: usize, delta: f32) -> f32 {
    if delta == 0.0 {
        return 0.0;
    }
    const EPS: f32 = 1e-4;
    let (min, max) = bounds(pos);
    // Blocks the moving box could touch.
    let mut lo = [0i32; 3];
    let mut hi = [0i32; 3];
    for a in 0..3 {
        let (mut l, mut h) = (min[a], max[a]);
        if a == axis {
            if delta < 0.0 {
                l += delta;
            } else {
                h += delta;
            }
        }
        lo[a] = ifloor(l + EPS);
        hi[a] = ifloor(h - EPS);
    }
    let mut d = delta;
    for x in lo[0]..=hi[0] {
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                if !def(world.get(x, y, z)).solid {
                    continue;
                }
                let c = [x, y, z];
                if d > 0.0 {
                    let face = c[axis] as f32;
                    if face >= max[axis] - EPS {
                        d = d.min(face - max[axis]);
                    }
                } else {
                    let face = (c[axis] + 1) as f32;
                    if face <= min[axis] + EPS {
                        d = d.max(face - min[axis]);
                    }
                }
            }
        }
    }
    if d.abs() < EPS { 0.0 } else { d }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::blocks::*;
    use std::boxed::Box;

    /// A world with a flat stone floor at y = 10 (top face at 11) around the origin.
    fn flat() -> Box<World> {
        let mut w = Box::new(World::new());
        w.reset(5);
        w.update(0, 0, usize::MAX);
        for x in -15..15 {
            for z in -15..15 {
                for y in 0..crate::worldgen::HEIGHT as i32 {
                    w.set(x, y, z, if y <= 10 { STONE } else { AIR });
                }
            }
        }
        w
    }

    #[test]
    fn falls_and_lands() {
        let w = flat();
        let mut p = Player::new([0.5, 20.0, 0.5]);
        for _ in 0..100 {
            p.tick(&w, MoveInput::default(), false);
        }
        assert!(p.on_ground);
        assert!((p.pos[1] - 11.0).abs() < 1e-3, "y = {}", p.pos[1]);
    }

    #[test]
    fn walking_speed_matches_minecraft() {
        let w = flat();
        let mut p = Player::new([0.5, 11.0, 0.5]);
        p.tick(&w, MoveInput::default(), false);
        let input = MoveInput { forward: 1.0, ..Default::default() };
        for _ in 0..40 {
            p.tick(&w, input, false);
        }
        let z0 = p.pos[2];
        for _ in 0..20 {
            p.tick(&w, input, false);
        }
        let speed = p.pos[2] - z0; // blocks per second
        assert!((speed - 4.317).abs() < 0.15, "speed = {speed}");
    }

    #[test]
    fn jump_height_is_about_1_25() {
        let w = flat();
        let mut p = Player::new([0.5, 11.0, 0.5]);
        p.tick(&w, MoveInput::default(), false);
        p.tick(&w, MoveInput::default(), false);
        assert!(p.on_ground);
        let mut top: f32 = 0.0;
        p.tick(&w, MoveInput { jump: true, ..Default::default() }, false);
        for _ in 0..30 {
            p.tick(&w, MoveInput::default(), false);
            top = top.max(p.pos[1] - 11.0);
        }
        assert!((top - 1.25).abs() < 0.05, "jump height = {top}");
    }

    #[test]
    fn walls_stop_and_auto_jump_climbs() {
        let mut w = flat();
        for x in -15..15 {
            w.set(x, 11, 3, STONE);
        }
        let input = MoveInput { forward: 1.0, ..Default::default() };
        let mut p = Player::new([0.5, 11.0, 0.5]);
        for _ in 0..60 {
            p.tick(&w, input, false);
        }
        assert!((p.pos[2] - (3.0 - HALF_WIDTH)).abs() < 1e-3, "z = {}", p.pos[2]);
        // With auto-jump the player gets on top of the wall (and walks on past it).
        let mut p = Player::new([0.5, 11.0, 0.5]);
        let mut stood_on_wall = false;
        for _ in 0..60 {
            p.tick(&w, input, true);
            stood_on_wall |= p.on_ground && (p.pos[1] - 12.0).abs() < 1e-3;
        }
        assert!(stood_on_wall && p.pos[2] > 4.0, "auto-jump: {:?}", p.pos);
    }
}
