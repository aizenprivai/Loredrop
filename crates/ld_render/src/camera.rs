//! The view camera.

use glam::{DVec3, Mat4, Vec3};

/// Where the camera is and where it looks.
///
/// Angles follow Minecraft: `yaw` 0° looks toward +Z (south) and increases
/// clockwise seen from above (90° = west, −X); `pitch` is positive looking *down*.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    /// Eye position in world space (f64 for precision far from spawn).
    pub position: DVec3,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    /// Vertical field of view in degrees.
    pub fov_deg: f32,
}

impl Camera {
    /// Unit vector the camera looks along.
    pub fn forward(&self) -> Vec3 {
        let (ys, yc) = self.yaw_deg.to_radians().sin_cos();
        let (ps, pc) = self.pitch_deg.to_radians().sin_cos();
        Vec3::new(-ys * pc, -ps, yc * pc)
    }

    /// Camera-relative view-projection matrix: the camera sits at the origin
    /// and meshes are offset by `origin - camera` (see world.wgsl).
    ///
    /// Uses an infinite reverse-Z projection (no far plane; best precision).
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let view = glam::camera::rh::view::look_to_mat4(Vec3::ZERO, self.forward(), Vec3::Y);
        // wgpu uses DirectX-style clip space (Y up, depth 0..1).
        let proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(
            self.fov_deg.to_radians(),
            aspect,
            0.05,
        );
        proj * view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_matches_yaw_convention() {
        let mut c = Camera { position: DVec3::ZERO, yaw_deg: 0.0, pitch_deg: 0.0, fov_deg: 70.0 };
        assert!((c.forward() - Vec3::Z).length() < 1e-6);
        c.yaw_deg = 90.0;
        assert!((c.forward() + Vec3::X).length() < 1e-6);
        c.pitch_deg = 90.0;
        assert!((c.forward() + Vec3::Y).length() < 1e-5);
    }

    #[test]
    fn point_in_front_is_visible() {
        let c = Camera { position: DVec3::ZERO, yaw_deg: 0.0, pitch_deg: 0.0, fov_deg: 70.0 };
        let p = c.view_proj(16.0 / 9.0) * glam::Vec4::new(0.0, 0.0, 10.0, 1.0);
        let ndc = p.truncate() / p.w;
        assert!(p.w > 0.0 && ndc.x.abs() < 1e-5 && ndc.z > 0.0 && ndc.z < 1.0);
    }
}
