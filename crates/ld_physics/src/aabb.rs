//! Axis-aligned bounding boxes.

use glam::DVec3;

/// A box whose edges are parallel to the world axes, in world coordinates.
///
/// Uses `f64` so collision stays exact far from spawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    pub fn new(min: DVec3, max: DVec3) -> Self {
        Self { min, max }
    }

    /// The unit cube occupying block `(x, y, z)`.
    pub fn block(x: i32, y: i32, z: i32) -> Self {
        let min = DVec3::new(x as f64, y as f64, z as f64);
        Self { min, max: min + DVec3::ONE }
    }

    /// A box of `width` × `height` × `width` whose bottom centre is `feet`
    /// (how entities are positioned).
    pub fn from_feet(feet: DVec3, width: f64, height: f64) -> Self {
        let h = width / 2.0;
        Self {
            min: DVec3::new(feet.x - h, feet.y, feet.z - h),
            max: DVec3::new(feet.x + h, feet.y + height, feet.z + h),
        }
    }

    pub fn offset(&self, d: DVec3) -> Self {
        Self { min: self.min + d, max: self.max + d }
    }

    /// Grows the box in the direction of `d` only (the volume swept by a move).
    pub fn expand_towards(&self, d: DVec3) -> Self {
        Self { min: self.min + d.min(DVec3::ZERO), max: self.max + d.max(DVec3::ZERO) }
    }

    /// True if the boxes overlap (touching edges do not count).
    pub fn intersects(&self, o: &Aabb) -> bool {
        self.min.x < o.max.x
            && self.max.x > o.min.x
            && self.min.y < o.max.y
            && self.max.y > o.min.y
            && self.min.z < o.max.z
            && self.max.z > o.min.z
    }

    /// If `moving` travelling `dx` along X would enter `self`, shortens `dx` so
    /// it stops at the surface. Only applies when the boxes overlap on the
    /// other two axes. `clip_y`/`clip_z` are the same for their axes.
    pub fn clip_x(&self, moving: &Aabb, dx: f64) -> f64 {
        if !overlaps(moving.min.y, moving.max.y, self.min.y, self.max.y)
            || !overlaps(moving.min.z, moving.max.z, self.min.z, self.max.z)
        {
            return dx;
        }
        clip_axis(moving.min.x, moving.max.x, self.min.x, self.max.x, dx)
    }

    pub fn clip_y(&self, moving: &Aabb, dy: f64) -> f64 {
        if !overlaps(moving.min.x, moving.max.x, self.min.x, self.max.x)
            || !overlaps(moving.min.z, moving.max.z, self.min.z, self.max.z)
        {
            return dy;
        }
        clip_axis(moving.min.y, moving.max.y, self.min.y, self.max.y, dy)
    }

    pub fn clip_z(&self, moving: &Aabb, dz: f64) -> f64 {
        if !overlaps(moving.min.x, moving.max.x, self.min.x, self.max.x)
            || !overlaps(moving.min.y, moving.max.y, self.min.y, self.max.y)
        {
            return dz;
        }
        clip_axis(moving.min.z, moving.max.z, self.min.z, self.max.z, dz)
    }
}

/// Strict 1D overlap (touching is not overlapping).
fn overlaps(a_min: f64, a_max: f64, b_min: f64, b_max: f64) -> bool {
    a_min < b_max && a_max > b_min
}

/// 1D version of the clip: a moving segment [m_min, m_max] heading toward a
/// fixed segment [o_min, o_max].
fn clip_axis(m_min: f64, m_max: f64, o_min: f64, o_max: f64, d: f64) -> f64 {
    // Tolerance so floating-point error never lets a box sink into a wall.
    const EPS: f64 = 1e-7;
    if d > 0.0 && m_max <= o_min + EPS {
        d.min(o_min - m_max)
    } else if d < 0.0 && m_min >= o_max - EPS {
        d.max(o_max - m_min)
    } else {
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_stops_at_surface() {
        let wall = Aabb::block(2, 0, 0);
        let player = Aabb::new(DVec3::new(0.5, 0.0, 0.2), DVec3::new(1.5, 1.0, 0.8));
        assert_eq!(wall.clip_x(&player, 2.0), 0.5); // 1.5 → 2.0
        assert_eq!(wall.clip_x(&player, -2.0), -2.0); // moving away: unaffected
    }

    #[test]
    fn no_clip_when_not_aligned() {
        let wall = Aabb::block(2, 5, 0); // far above
        let player = Aabb::new(DVec3::new(0.5, 0.0, 0.2), DVec3::new(1.5, 1.0, 0.8));
        assert_eq!(wall.clip_x(&player, 2.0), 2.0);
    }
}
