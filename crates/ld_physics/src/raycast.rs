//! Finding the block the player is looking at.
//!
//! Uses the classic grid-walking algorithm (Amanatides & Woo, 1987): step
//! from one block boundary to the next along the ray, so every block the ray
//! passes through is visited exactly once, in order, without sampling gaps.

use glam::DVec3;
use ld_core::coords::BlockPos;

/// The result of a ray hitting a block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    /// The block that was hit.
    pub pos: BlockPos,
    /// Which face was hit, as the outward normal (e.g. `[0, 1, 0]` = top).
    /// All zeros if the ray started inside the block.
    pub normal: [i32; 3],
    /// Distance from the origin to the hit point, in blocks.
    pub distance: f64,
}

impl RayHit {
    /// The block in front of the hit face (where a new block would be placed).
    pub fn adjacent(&self) -> BlockPos {
        BlockPos::new(self.pos.x + self.normal[0], self.pos.y + self.normal[1], self.pos.z + self.normal[2])
    }
}

/// Casts a ray from `origin` along `dir` for up to `max_distance` blocks and
/// returns the first block for which `is_target` is true.
pub fn raycast(
    origin: DVec3,
    dir: DVec3,
    max_distance: f64,
    mut is_target: impl FnMut(BlockPos) -> bool,
) -> Option<RayHit> {
    let dir = dir.normalize_or_zero();
    if dir == DVec3::ZERO {
        return None;
    }
    let mut cell = origin.floor().as_ivec3();
    let step = dir.signum().as_ivec3();
    // Distance along the ray to cross one whole block on each axis.
    let t_delta = DVec3::new(1.0 / dir.x.abs(), 1.0 / dir.y.abs(), 1.0 / dir.z.abs());
    // Distance along the ray to the first boundary on each axis.
    let next_boundary = |o: f64, c: i32, s: i32| if s > 0 { c as f64 + 1.0 - o } else { o - c as f64 };
    let mut t_max = DVec3::new(
        next_boundary(origin.x, cell.x, step.x) * t_delta.x,
        next_boundary(origin.y, cell.y, step.y) * t_delta.y,
        next_boundary(origin.z, cell.z, step.z) * t_delta.z,
    );
    let mut normal = [0, 0, 0];
    let mut t = 0.0;
    loop {
        let pos = BlockPos::new(cell.x, cell.y, cell.z);
        if is_target(pos) {
            return Some(RayHit { pos, normal, distance: t });
        }
        // Advance across whichever boundary is closest.
        if t_max.x < t_max.y && t_max.x < t_max.z {
            t = t_max.x;
            cell.x += step.x;
            t_max.x += t_delta.x;
            normal = [-step.x, 0, 0];
        } else if t_max.y < t_max.z {
            t = t_max.y;
            cell.y += step.y;
            t_max.y += t_delta.y;
            normal = [0, -step.y, 0];
        } else {
            t = t_max.z;
            cell.z += step.z;
            t_max.z += t_delta.z;
            normal = [0, 0, -step.z];
        }
        if t > max_distance {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_block_ahead_with_correct_face() {
        // Block at (3, 0, 0); looking along +X from (0.5, 0.5, 0.5).
        let hit =
            raycast(DVec3::new(0.5, 0.5, 0.5), DVec3::X, 10.0, |p| p == BlockPos::new(3, 0, 0)).unwrap();
        assert_eq!(hit.pos, BlockPos::new(3, 0, 0));
        assert_eq!(hit.normal, [-1, 0, 0], "west face");
        assert!((hit.distance - 2.5).abs() < 1e-9);
        assert_eq!(hit.adjacent(), BlockPos::new(2, 0, 0));
    }

    #[test]
    fn looking_down_hits_top_face_and_respects_reach() {
        let floor = |p: BlockPos| p.y < 0;
        let hit = raycast(DVec3::new(0.5, 1.62, 0.5), DVec3::NEG_Y, 4.5, floor).unwrap();
        assert_eq!(hit.pos, BlockPos::new(0, -1, 0));
        assert_eq!(hit.normal, [0, 1, 0]);
        assert!(raycast(DVec3::new(0.5, 10.0, 0.5), DVec3::NEG_Y, 4.5, floor).is_none(), "out of reach");
    }

    #[test]
    fn diagonal_ray_visits_blocks_in_order_without_gaps() {
        let mut visited = Vec::new();
        raycast(DVec3::new(0.2, 0.3, 0.4), DVec3::new(1.0, 0.7, 0.3), 6.0, |p| {
            visited.push(p);
            false
        });
        for w in visited.windows(2) {
            let d = (w[1].x - w[0].x).abs() + (w[1].y - w[0].y).abs() + (w[1].z - w[0].z).abs();
            assert_eq!(d, 1, "consecutive cells must share a face: {:?} → {:?}", w[0], w[1]);
        }
        assert!(visited.len() > 6);
    }

    #[test]
    fn negative_coordinates_work() {
        let hit = raycast(DVec3::new(-0.5, 0.5, -0.5), DVec3::NEG_Z, 10.0, |p| p.z == -4).unwrap();
        assert_eq!(hit.pos, BlockPos::new(-1, 0, -4));
        assert_eq!(hit.normal, [0, 0, 1]);
    }
}
