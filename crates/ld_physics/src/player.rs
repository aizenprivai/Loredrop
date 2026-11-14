//! The player's movement model.
//!
//! These constants and the update order follow Minecraft Java Edition's
//! movement so the game *feels* the same: the same walking/sprinting speed,
//! the same jump arc, the same slipperiness. Values are in blocks and
//! blocks-per-tick (20 ticks = 1 second).
//!
//! Resulting top speeds (verified by tests below):
//! * walking ≈ 4.3 blocks/s, sprinting ≈ 5.6 blocks/s,
//! * jump height ≈ 1.25 blocks,
//! * creative flying ≈ 10.9 blocks/s (≈ 21.6 sprint-flying).

use crate::{Aabb, VoxelCollider, move_and_collide};
use glam::DVec3;

/// Hitbox width (X and Z), in blocks.
pub const WIDTH: f64 = 0.6;
/// Hitbox height when standing.
pub const HEIGHT: f64 = 1.8;
/// Camera height above the feet when standing.
pub const EYE_HEIGHT: f64 = 1.62;
/// Camera height while sneaking.
pub const SNEAK_EYE_HEIGHT: f64 = 1.27;

/// Downward acceleration per tick.
pub const GRAVITY: f64 = 0.08;
/// Vertical velocity multiplier per tick (air resistance).
pub const VERTICAL_DRAG: f64 = 0.98;
/// Slipperiness of normal blocks. Ice will be 0.98 later.
pub const DEFAULT_SLIPPERINESS: f64 = 0.6;
/// Horizontal velocity multiplier per tick while airborne.
pub const AIR_FRICTION: f64 = 0.91;
/// Base walking acceleration (the player's movement-speed attribute).
pub const WALK_SPEED: f64 = 0.1;
/// Sprinting multiplies walking speed by this.
pub const SPRINT_MULTIPLIER: f64 = 1.3;
/// Horizontal acceleration while in the air.
pub const AIR_ACCELERATION: f64 = 0.02;
/// Initial upward velocity of a jump.
pub const JUMP_VELOCITY: f64 = 0.42;
/// Extra forward push when jumping while sprinting.
pub const SPRINT_JUMP_BOOST: f64 = 0.2;
/// Sneaking multiplies movement input by this.
pub const SNEAK_MULTIPLIER: f64 = 0.3;
/// Creative flying acceleration.
pub const FLY_SPEED: f64 = 0.05;
/// Vertical velocity multiplier per tick while flying.
pub const FLY_VERTICAL_DRAG: f64 = 0.6;
/// Velocity components smaller than this snap to zero (prevents endless creep).
const MIN_VELOCITY: f64 = 0.003;

/// Liquid the player is in (decided by the caller from the world).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fluid {
    #[default]
    None,
    Water,
    Lava,
}

/// Movement intent for one tick, already translated from key bindings.
#[derive(Debug, Clone, Copy, Default)]
pub struct MoveInput {
    /// +1 forward, -1 backward.
    pub forward: f64,
    /// +1 left, -1 right (Minecraft's convention).
    pub strafe: f64,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

/// The player's physical state.
#[derive(Debug, Clone)]
pub struct PlayerBody {
    /// Bottom-centre of the hitbox.
    pub pos: DVec3,
    /// Blocks per tick.
    pub vel: DVec3,
    pub on_ground: bool,
    pub flying: bool,
    pub sneaking: bool,
    pub sprinting: bool,
    /// True while pressing against a wall horizontally.
    pub horizontal_collision: bool,
    /// Liquid around the body this tick (set by the caller before `tick`).
    pub fluid: Fluid,
    /// Blocks fallen since last touching the ground (for fall damage).
    pub fall_distance: f64,
    /// Set on the tick the player lands: how far they fell. Read and cleared
    /// by the game (to apply fall damage).
    pub landed_after_fall: Option<f64>,
}

impl PlayerBody {
    pub fn new(pos: DVec3) -> Self {
        Self {
            pos,
            vel: DVec3::ZERO,
            on_ground: false,
            flying: false,
            sneaking: false,
            sprinting: false,
            horizontal_collision: false,
            fluid: Fluid::None,
            fall_distance: 0.0,
            landed_after_fall: None,
        }
    }

    pub fn aabb(&self) -> Aabb {
        Aabb::from_feet(self.pos, WIDTH, HEIGHT)
    }

    /// Camera height above the feet in the current pose.
    pub fn eye_height(&self) -> f64 {
        if self.sneaking && !self.flying { SNEAK_EYE_HEIGHT } else { EYE_HEIGHT }
    }

    /// Advances the body by one tick.
    ///
    /// `yaw_deg` is the look direction (0 = +Z/south, 90 = −X/west, like
    /// Minecraft). Sprint *eligibility* (key held/toggled) is decided by the
    /// caller; this function stops sprinting when the rules say so.
    pub fn tick(&mut self, input: MoveInput, yaw_deg: f64, world: &impl VoxelCollider) {
        self.sneaking = input.sneak && !self.flying;

        // Sprinting requires moving forward and not sneaking; hitting a wall stops it.
        if input.sprint && input.forward > 0.0 && !self.sneaking {
            self.sprinting = true;
        }
        if input.forward <= 0.0 || self.sneaking || self.horizontal_collision {
            self.sprinting = false;
        }

        // Inputs are scaled by 0.98 in Minecraft (a historical quirk we keep for feel).
        let mut fwd = input.forward * 0.98;
        let mut side = input.strafe * 0.98;
        if self.sneaking {
            fwd *= SNEAK_MULTIPLIER;
            side *= SNEAK_MULTIPLIER;
        }

        if self.fluid != Fluid::None && !self.flying {
            self.tick_in_fluid(input, fwd, side, yaw_deg, world);
            return;
        }

        if self.flying {
            if input.jump {
                self.vel.y += FLY_SPEED * 3.0;
            }
            if input.sneak {
                self.vel.y -= FLY_SPEED * 3.0;
            }
        } else if input.jump && self.on_ground {
            self.vel.y = JUMP_VELOCITY;
            if self.sprinting {
                let r = yaw_deg.to_radians();
                self.vel.x -= r.sin() * SPRINT_JUMP_BOOST;
                self.vel.z += r.cos() * SPRINT_JUMP_BOOST;
            }
        }

        // Acceleration depends on whether we stand on something.
        let friction =
            if self.on_ground && !self.flying { DEFAULT_SLIPPERINESS * AIR_FRICTION } else { AIR_FRICTION };
        let accel = if self.flying {
            FLY_SPEED * if self.sprinting { 2.0 } else { 1.0 }
        } else if self.on_ground {
            // Minecraft scales ground acceleration by 0.216/slip³ so slippery
            // blocks accelerate slower; for normal blocks this equals 1.
            let speed = WALK_SPEED * if self.sprinting { SPRINT_MULTIPLIER } else { 1.0 };
            speed * (0.216 / DEFAULT_SLIPPERINESS.powi(3))
        } else if self.sprinting {
            AIR_ACCELERATION * SPRINT_MULTIPLIER
        } else {
            AIR_ACCELERATION
        };
        self.vel += input_to_world(fwd, side, yaw_deg, accel);

        // Move with collision.
        let requested = self.vel;
        let result = move_and_collide(world, &self.aabb(), requested);
        self.pos += result.moved;
        self.horizontal_collision = result.hit_x || result.hit_z;
        self.on_ground = result.landed(requested);
        self.track_fall(result.moved.y);
        if result.hit_x {
            self.vel.x = 0.0;
        }
        if result.hit_y {
            self.vel.y = 0.0;
        }
        if result.hit_z {
            self.vel.z = 0.0;
        }

        // Landing ends creative flight, as in Minecraft.
        if self.flying && self.on_ground {
            self.flying = false;
        }

        // Gravity and drag are applied *after* moving (Minecraft order).
        if self.flying {
            self.vel.y *= FLY_VERTICAL_DRAG;
        } else {
            self.vel.y = (self.vel.y - GRAVITY) * VERTICAL_DRAG;
        }
        self.vel.x *= friction;
        self.vel.z *= friction;
        for c in [&mut self.vel.x, &mut self.vel.y, &mut self.vel.z] {
            if c.abs() < MIN_VELOCITY {
                *c = 0.0;
            }
        }
    }
}

impl PlayerBody {
    /// Counts fall distance while descending; reports it on landing.
    fn track_fall(&mut self, moved_y: f64) {
        if self.flying || self.fluid != Fluid::None {
            self.fall_distance = 0.0;
            return;
        }
        if moved_y < 0.0 {
            self.fall_distance -= moved_y; // include the final step of the fall
        }
        if self.on_ground {
            if self.fall_distance > 0.0 {
                self.landed_after_fall = Some(self.fall_distance);
            }
            self.fall_distance = 0.0;
        }
    }

    /// Swimming (Minecraft's liquid movement): slow acceleration, strong
    /// drag, weak gravity. Holding jump swims upward; pushing against a wall
    /// while in water climbs out.
    fn tick_in_fluid(
        &mut self,
        input: MoveInput,
        fwd: f64,
        side: f64,
        yaw_deg: f64,
        world: &impl VoxelCollider,
    ) {
        let lava = self.fluid == Fluid::Lava;
        if input.jump {
            self.vel.y += 0.04;
        }
        self.vel += input_to_world(fwd, side, yaw_deg, 0.02);
        let requested = self.vel;
        let result = move_and_collide(world, &self.aabb(), requested);
        self.pos += result.moved;
        self.horizontal_collision = result.hit_x || result.hit_z;
        self.on_ground = result.landed(requested);
        if result.hit_x {
            self.vel.x = 0.0;
        }
        if result.hit_z {
            self.vel.z = 0.0;
        }
        if result.hit_y {
            self.vel.y = 0.0;
        }
        let drag = if lava { 0.5 } else { 0.8 };
        self.vel.x *= drag;
        self.vel.z *= drag;
        self.vel.y = self.vel.y * drag - 0.02;
        if self.horizontal_collision && input.jump {
            self.vel.y = 0.3; // climb out onto the bank
        }
        self.sprinting = false;
        self.track_fall(result.moved.y);
    }
}

/// Converts forward/strafe input into a world-space velocity change.
fn input_to_world(forward: f64, strafe: f64, yaw_deg: f64, accel: f64) -> DVec3 {
    let len2 = forward * forward + strafe * strafe;
    if len2 < 1e-7 {
        return DVec3::ZERO;
    }
    // Diagonal movement is normalised so it isn't faster than straight.
    let scale = if len2 > 1.0 { accel / len2.sqrt() } else { accel };
    let (f, s) = (forward * scale, strafe * scale);
    let (sin, cos) = yaw_deg.to_radians().sin_cos();
    DVec3::new(s * cos - f * sin, 0.0, f * cos + s * sin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ld_core::coords::BlockPos;

    /// Flat infinite floor: every block with y < 0 is solid, plus optional walls.
    struct Flat {
        wall_x: Option<i32>,
    }
    impl VoxelCollider for Flat {
        fn collision_boxes(&self, p: BlockPos, out: &mut Vec<Aabb>) {
            if p.y < 0 || self.wall_x == Some(p.x) {
                out.push(Aabb::block(p.x, p.y, p.z));
            }
        }
    }

    fn settle(body: &mut PlayerBody, w: &Flat) {
        for _ in 0..40 {
            body.tick(MoveInput::default(), 0.0, w);
        }
    }

    #[test]
    fn falls_and_lands_on_floor() {
        let w = Flat { wall_x: None };
        let mut b = PlayerBody::new(DVec3::new(0.5, 10.0, 0.5));
        settle(&mut b, &w);
        assert!(b.on_ground);
        assert!(b.pos.y.abs() < 1e-6, "feet at y=0, got {}", b.pos.y);
    }

    fn top_speed(sprint: bool) -> f64 {
        let w = Flat { wall_x: None };
        let mut b = PlayerBody::new(DVec3::new(0.5, 0.0, 0.5));
        settle(&mut b, &w);
        let input = MoveInput { forward: 1.0, sprint, ..Default::default() };
        for _ in 0..100 {
            b.tick(input, 0.0, &w);
        }
        let z0 = b.pos.z;
        for _ in 0..20 {
            b.tick(input, 0.0, &w);
        }
        b.pos.z - z0 // blocks per second
    }

    #[test]
    fn walk_and_sprint_speeds_match_minecraft() {
        let walk = top_speed(false);
        let sprint = top_speed(true);
        assert!((walk - 4.317).abs() < 0.05, "walk {walk}");
        assert!((sprint - 5.612).abs() < 0.07, "sprint {sprint}");
    }

    #[test]
    fn jump_height_is_about_1_25_blocks() {
        let w = Flat { wall_x: None };
        let mut b = PlayerBody::new(DVec3::new(0.5, 0.0, 0.5));
        settle(&mut b, &w);
        b.tick(MoveInput { jump: true, ..Default::default() }, 0.0, &w);
        let mut peak: f64 = 0.0;
        for _ in 0..30 {
            b.tick(MoveInput::default(), 0.0, &w);
            peak = peak.max(b.pos.y);
        }
        assert!((peak - 1.2522).abs() < 0.01, "peak {peak}");
        assert!(b.on_ground);
    }

    #[test]
    fn wall_blocks_movement_and_stops_sprint() {
        let w = Flat { wall_x: Some(3) };
        let mut b = PlayerBody::new(DVec3::new(0.5, 0.0, 0.5));
        settle(&mut b, &w);
        // yaw -90 faces +X (east)
        let input = MoveInput { forward: 1.0, sprint: true, ..Default::default() };
        for _ in 0..60 {
            b.tick(input, -90.0, &w);
        }
        assert!((b.pos.x - (3.0 - WIDTH / 2.0)).abs() < 1e-6, "x {}", b.pos.x);
        assert!(!b.sprinting);
    }

    #[test]
    fn water_slows_falls_and_resets_fall_distance() {
        let w = Flat { wall_x: None };
        let mut b = PlayerBody::new(DVec3::new(0.5, 30.0, 0.5));
        for _ in 0..20 {
            b.tick(MoveInput::default(), 0.0, &w);
        }
        assert!(b.fall_distance > 5.0, "falling through air");
        b.fluid = Fluid::Water;
        b.tick(MoveInput::default(), 0.0, &w);
        assert_eq!(b.fall_distance, 0.0, "water cancels fall damage");
        for _ in 0..40 {
            b.tick(MoveInput::default(), 0.0, &w);
        }
        assert!(b.vel.y > -0.15, "sinks slowly in water: {}", b.vel.y);
        let y = b.pos.y;
        for _ in 0..20 {
            b.tick(MoveInput { jump: true, ..Default::default() }, 0.0, &w);
        }
        assert!(b.pos.y > y, "holding jump swims up");
    }

    #[test]
    fn landing_reports_fall_distance() {
        let w = Flat { wall_x: None };
        let mut b = PlayerBody::new(DVec3::new(0.5, 10.0, 0.5));
        let mut landed = None;
        for _ in 0..60 {
            b.tick(MoveInput::default(), 0.0, &w);
            if let Some(d) = b.landed_after_fall.take() {
                landed = Some(d);
            }
        }
        assert!((landed.unwrap() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn yaw_convention_matches_minecraft() {
        // yaw 0 → +Z, yaw 90 → −X; strafe +1 at yaw 0 → +X (left of a +Z-facing player).
        let v = input_to_world(1.0, 0.0, 0.0, 1.0);
        assert!((v.z - 1.0).abs() < 1e-9);
        let v = input_to_world(1.0, 0.0, 90.0, 1.0);
        assert!((v.x + 1.0).abs() < 1e-9);
        let v = input_to_world(0.0, 1.0, 0.0, 1.0);
        assert!((v.x - 1.0).abs() < 1e-9);
    }

    #[test]
    fn flying_speed_and_landing_ends_flight() {
        let w = Flat { wall_x: None };
        let mut b = PlayerBody::new(DVec3::new(0.5, 5.0, 0.5));
        b.flying = true;
        let input = MoveInput { forward: 1.0, ..Default::default() };
        for _ in 0..100 {
            b.tick(input, 0.0, &w);
        }
        let z0 = b.pos.z;
        for _ in 0..20 {
            b.tick(input, 0.0, &w);
        }
        let speed = b.pos.z - z0;
        assert!((speed - 10.89).abs() < 0.15, "fly speed {speed}");
        assert!((b.pos.y - 5.0).abs() < 1e-9, "hovering keeps height");
        for _ in 0..40 {
            b.tick(MoveInput { sneak: true, ..Default::default() }, 0.0, &w);
        }
        assert!(!b.flying && b.on_ground);
    }
}
