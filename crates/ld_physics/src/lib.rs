//! # ld_physics — collision and movement
//!
//! * [`Aabb`] — axis-aligned bounding boxes, the shape of every collider.
//! * [`move_and_collide`] — moves a box through voxel terrain without passing
//!   through solid blocks.
//! * [`player`] — the player's movement model, with Minecraft-faithful
//!   constants (gravity, friction, jump height, sprint and fly speeds).
//!
//! Terrain is accessed through the [`VoxelCollider`] trait, so this crate
//! doesn't care whether blocks come from a test scene or the real chunk system.
//!
//! All physics runs at the fixed **20 ticks per second** simulation rate, and
//! every speed below is in **blocks per tick**.

mod aabb;
pub mod player;

pub use aabb::Aabb;

use glam::DVec3;
use ld_core::coords::BlockPos;

/// Anything that can answer "which collision boxes are at this block?".
pub trait VoxelCollider {
    /// Appends the collision boxes of the block at `pos` (in world space) to `out`.
    /// A full cube adds one box; air adds none. Slabs/stairs will add partial boxes.
    fn collision_boxes(&self, pos: BlockPos, out: &mut Vec<Aabb>);
}

/// Which axes were blocked during a move.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MoveResult {
    /// The movement that actually happened (≤ the requested movement).
    pub moved: DVec3,
    pub hit_x: bool,
    pub hit_y: bool,
    pub hit_z: bool,
}

impl MoveResult {
    /// True when downward movement was stopped by something below.
    pub fn landed(&self, requested: DVec3) -> bool {
        self.hit_y && requested.y < 0.0
    }
}

/// Moves `aabb` by `delta` through the world, stopping at solid blocks.
///
/// This is the classic Minecraft approach: gather every block box the moving
/// box could touch, then resolve one axis at a time — **Y first** (so you land
/// on the ground before sliding), then whichever horizontal axis is moving
/// more. Each axis movement is shortened so the box stops exactly touching the
/// obstacle. Resolving axes separately is what lets you slide along walls.
pub fn move_and_collide(world: &impl VoxelCollider, aabb: &Aabb, delta: DVec3) -> MoveResult {
    // Every block the box touches anywhere along its path.
    let sweep = aabb.expand_towards(delta);
    let mut boxes = Vec::new();
    let min = sweep.min.floor().as_ivec3();
    // Subtract a tiny epsilon so a box ending exactly on a block edge doesn't
    // include the next block.
    let max = (sweep.max - DVec3::splat(1e-7)).floor().as_ivec3();
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                world.collision_boxes(BlockPos::new(x, y, z), &mut boxes);
            }
        }
    }

    let mut b = *aabb;
    let mut d = delta;

    d.y = boxes.iter().fold(d.y, |dy, o| o.clip_y(&b, dy));
    b = b.offset(DVec3::new(0.0, d.y, 0.0));

    let x_first = d.x.abs() >= d.z.abs();
    if x_first {
        d.x = boxes.iter().fold(d.x, |dx, o| o.clip_x(&b, dx));
        b = b.offset(DVec3::new(d.x, 0.0, 0.0));
    }
    d.z = boxes.iter().fold(d.z, |dz, o| o.clip_z(&b, dz));
    b = b.offset(DVec3::new(0.0, 0.0, d.z));
    if !x_first {
        d.x = boxes.iter().fold(d.x, |dx, o| o.clip_x(&b, dx));
    }

    MoveResult { moved: d, hit_x: d.x != delta.x, hit_y: d.y != delta.y, hit_z: d.z != delta.z }
}
