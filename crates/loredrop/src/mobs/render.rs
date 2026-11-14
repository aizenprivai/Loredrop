//! Turning creatures into triangles: box models with walking, head,
//! hurt and death animations. Built on the CPU each frame (a few dozen
//! creatures × a few boxes is cheap) and drawn with the entity pipeline.

use super::{Arrow, Mob, Mobs};
use glam::{DVec3, Mat3, Vec3};
use ld_game::mobs::PartRole;
use ld_render::EntityVertex;
use ld_world::BlockId;

/// Texture-array layer for each creature texture name.
pub type LayerOf<'a> = &'a dyn Fn(&str) -> u32;

/// How a carried block looks: texture layers and tints per face (East,
/// West, Up, Down, South, North), as for dropped block items.
pub type BlockLook<'a> = &'a dyn Fn(BlockId) -> Option<([u32; 6], [u32; 6])>;

/// Arm angle (degrees, forward and slightly down) while carrying a block.
const CARRY_ARM_PITCH: f32 = -60.0;

/// Where a carried block sits in model space (blocks): between the hands,
/// which at `CARRY_ARM_PITCH` end ≈1.3 up and ≈1.1 forward on a Hollow.
const CARRY_OFFSET: Vec3 = Vec3::new(0.0, 1.3, 0.85);

/// Half the edge of a carried block (a half-size block, like a held item).
const CARRY_HALF_SIZE: f32 = 0.25;

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut d = (b - a) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    }
    if d < -180.0 {
        d += 360.0;
    }
    a + d * t
}

fn rot_xyz(deg: [f32; 3]) -> Mat3 {
    let [x, y, z] = deg.map(f32::to_radians);
    Mat3::from_rotation_y(y) * Mat3::from_rotation_x(x) * Mat3::from_rotation_z(z)
}

impl Mobs {
    /// Appends triangles for all creatures and arrows. `alpha` interpolates
    /// between the last two ticks; `camera` makes positions camera-relative.
    pub fn build_vertices(
        &self,
        camera: DVec3,
        alpha: f64,
        layer_of: LayerOf,
        block_look: BlockLook,
        arrow_layer: u32,
        out: &mut Vec<EntityVertex>,
    ) {
        for m in &self.list {
            self.build_mob(m, camera, alpha, layer_of, block_look, out);
        }
        for a in &self.arrows {
            build_arrow(a, camera, alpha, arrow_layer, out);
        }
    }

    fn build_mob(
        &self,
        m: &Mob,
        camera: DVec3,
        alpha: f64,
        layer_of: LayerOf,
        block_look: BlockLook,
        out: &mut Vec<EntityVertex>,
    ) {
        let def = self.defs.get(m.kind);
        let pos = m.prev_pos.lerp(m.pos, alpha);
        let base = (pos - camera).as_vec3();
        let yaw = lerp_angle(m.prev_yaw, m.yaw, alpha as f32).to_radians();
        // Body rotation: maps the model's +z (front) onto the yaw direction
        // (−sin yaw, 0, cos yaw), Minecraft's convention.
        let body = Mat3::from_rotation_y(-yaw);
        // Dying creatures tip over sideways over 20 ticks, like Minecraft.
        let tilt = if m.death_time > 0 {
            ((m.death_time as f32 + alpha as f32) / 20.0).min(1.0) * 90.0
        } else {
            0.0
        };
        let tilt_m = Mat3::from_rotation_z(tilt.to_radians());
        // Walk cycle.
        let swing_amount = ((m.pos - m.prev_pos).length() as f32 * 6.0).min(1.0);
        let phase = m.walk_dist * 3.0;
        // Red tint while hurt or dying.
        let tint = if m.hurt_time > 0 || m.death_time > 0 { 0xFF_8080 } else { 0xFF_FFFF };
        let wing_flap = if m.on_ground { 0.0 } else { (m.age as f32 * 1.5).sin() * 60.0 };
        let carried = if m.death_time == 0 { m.carrying.and_then(block_look) } else { None };
        for part in &def.parts {
            let mut anim = [0.0f32; 3];
            match part.role {
                // Arms held out in front while carrying a block.
                PartRole::Arm if carried.is_some() => anim[0] = CARRY_ARM_PITCH,
                PartRole::Leg | PartRole::Arm => {
                    let p = phase + if part.phase == 1 { std::f32::consts::PI } else { 0.0 };
                    if def.name == "spider" {
                        anim[1] = p.sin() * 20.0 * swing_amount; // spider legs sweep sideways
                    } else {
                        anim[0] = p.sin() * 40.0 * swing_amount;
                    }
                }
                PartRole::Head => anim[0] = m.head_pitch.clamp(-40.0, 40.0),
                PartRole::Wing => anim[2] = if part.pivot[0] < 0.0 { -wing_flap } else { wing_flap },
                PartRole::None => {}
            }
            let part_rot = rot_xyz(anim) * rot_xyz(part.rot);
            let pivot = Vec3::from(part.pivot) / 16.0;
            let [x, y, z, w, h, d] = part.cuboid;
            let min = Vec3::new(x, y, z) / 16.0;
            let max = min + Vec3::new(w, h, d) / 16.0;
            let corner = |cx: bool, cy: bool, cz: bool| {
                let p = Vec3::new(
                    if cx { max.x } else { min.x },
                    if cy { max.y } else { min.y },
                    if cz { max.z } else { min.z },
                );
                let local = part_rot * (p - pivot) + pivot;
                base + body * (tilt_m * local)
            };
            let side = layer_of(&part.tex);
            let front = part.front.as_deref().map(layer_of).unwrap_or(side);
            // (corners bottom-left, bottom-right, top-right, top-left, layer, shade)
            let faces = [
                (
                    [
                        corner(false, false, true),
                        corner(true, false, true),
                        corner(true, true, true),
                        corner(false, true, true),
                    ],
                    front,
                    0.8,
                ),
                (
                    [
                        corner(true, false, false),
                        corner(false, false, false),
                        corner(false, true, false),
                        corner(true, true, false),
                    ],
                    side,
                    0.8,
                ),
                (
                    [
                        corner(true, false, true),
                        corner(true, false, false),
                        corner(true, true, false),
                        corner(true, true, true),
                    ],
                    side,
                    0.6,
                ),
                (
                    [
                        corner(false, false, false),
                        corner(false, false, true),
                        corner(false, true, true),
                        corner(false, true, false),
                    ],
                    side,
                    0.6,
                ),
                (
                    [
                        corner(false, true, true),
                        corner(true, true, true),
                        corner(true, true, false),
                        corner(false, true, false),
                    ],
                    side,
                    1.0,
                ),
                (
                    [
                        corner(false, false, false),
                        corner(true, false, false),
                        corner(true, false, true),
                        corner(false, false, true),
                    ],
                    side,
                    0.5,
                ),
            ];
            for (p, layer, shade) in faces {
                push_quad(out, p, layer, shade_color(tint, shade));
            }
        }
        if let Some((layers, tints)) = carried {
            // `cube` turns by `angle` the same way `body` does for `yaw`.
            let center = base + body * CARRY_OFFSET;
            crate::entities::cube(out, center, CARRY_HALF_SIZE, yaw, layers, tints);
        }
    }
}

fn shade_color(c: u32, k: f32) -> u32 {
    let ch = |s: u32| (((c >> s) & 255) as f32 * k) as u32;
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

fn push_quad(out: &mut Vec<EntityVertex>, p: [Vec3; 4], layer: u32, color: u32) {
    let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    for i in [0, 1, 2, 0, 2, 3] {
        out.push(EntityVertex { pos: p[i].to_array(), uv: uv[i], layer, color });
    }
}

/// An arrow: two crossed flat sprites aligned with its flight direction.
fn build_arrow(a: &Arrow, camera: DVec3, alpha: f64, layer: u32, out: &mut Vec<EntityVertex>) {
    let pos = (a.prev_pos.lerp(a.pos, alpha) - camera).as_vec3();
    let dir = a.vel.as_vec3().normalize_or(Vec3::Z);
    let side = dir.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = side.cross(dir);
    let (l, w) = (0.4, 0.25);
    for perp in [side, up] {
        // The sprite's diagonal runs bottom-left → top-right; align it with the flight.
        let a0 = pos - dir * l - perp * w;
        let a1 = pos - dir * l + perp * w;
        let a2 = pos + dir * l + perp * w;
        let a3 = pos + dir * l - perp * w;
        push_quad(out, [a0, a3, a2, a1], layer, 0xFF_FFFF);
    }
}
