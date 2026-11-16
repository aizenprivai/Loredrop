//! The 3D view.
//!
//! There is no GPU on the calculator, so the view is drawn by *ray casting*:
//! for every pixel, a ray leaves the eye and steps through the block grid one
//! cell at a time (the Amanatides–Woo "voxel traversal" algorithm) until it
//! hits something. The cell it hits and the exact point on the face give the
//! block, the face and the texel to show. On top of that:
//!
//! * faces are shaded by direction like on PC (top bright, bottom dark),
//! * daylight comes from the sky: a face is lit only if the air in front of
//!   it has open sky straight above, so caves and overhangs are dark,
//! * leaves and glass let rays through their see-through texels,
//! * water tints whatever is seen through it,
//! * fog fades distant blocks into the sky colour,
//! * the targeted block gets a dark outline and mining cracks.
//!
//! The view is computed at a lower resolution (one ray per `pixel_size` x
//! `pixel_size` screen pixels) and sent to the screen row by row, so no full
//! frame buffer is needed (the calculator has little RAM).

use crate::blocks::*;
use crate::color::{mix, scale};
use crate::math::{abs, cos, ifloor, sin, sqrt, tan};
use crate::settings::Settings;
use crate::sky::Sky;
use crate::world::World;
use crate::worldgen::HEIGHT;
use crate::{Platform, SCREEN_W};

/// Height of the 3D view on screen (the rest is the status bar / hotbar).
pub const VIEW_H: usize = 200;
/// Largest supported pixel size.
const MAX_PIXEL: usize = 4;
/// Colour of water seen from above or below.
const WATER_COLOR: u16 = crate::color::hex(0x2F5FC8);
/// Outline colour of the targeted block.
const OUTLINE: u16 = crate::color::hex(0x101010);

/// The block the player is looking at, for the outline and cracks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target {
    /// Cell of the block.
    pub cell: [i32; 3],
    /// Mining progress: 0 = none, 1..=10 = crack stage + 1.
    pub crack: u8,
}

/// Camera for one frame.
#[derive(Clone, Copy)]
pub struct Camera {
    /// Eye position.
    pub eye: [f32; 3],
    /// Horizontal angle (radians, 0 = towards +Z).
    pub yaw: f32,
    /// Vertical angle (radians, positive = up).
    pub pitch: f32,
}

/// Per-frame constants shared by all rays.
struct Scene<'a> {
    world: &'a World,
    sky: &'a Sky,
    eye: [f32; 3],
    max_dist: f32,
    fog_start: f32,
    /// Brightness of sky-lit faces and of faces in caves (x256).
    sun_light: u32,
    cave_light: u32,
    target: Option<Target>,
    eye_in_water: bool,
    /// Size of one rendered pixel, in blocks, at distance 1 (for outlines
    /// that stay about one pixel thick at any distance).
    pixel_angle: f32,
    /// Highest non-air block in the window (see `World::ceiling`).
    ceiling: i32,
}

/// Draws the 3D view (screen rows `0..VIEW_H`).
pub fn render(
    p: &mut dyn Platform,
    world: &World,
    sky: &Sky,
    cam: Camera,
    settings: &Settings,
    target: Option<Target>,
) {
    let ps = (settings.pixel_size as usize).clamp(1, MAX_PIXEL);
    let (rw, rh) = (SCREEN_W / ps, VIEW_H / ps);

    // Camera basis: forward, right and up.
    let (cy, sy, cp, sp) = (cos(cam.yaw), sin(cam.yaw), cos(cam.pitch), sin(cam.pitch));
    let fwd = [-sy * cp, sp, cy * cp];
    let right = [-cy, 0.0, -sy];
    let up = [
        right[1] * fwd[2] - right[2] * fwd[1],
        right[2] * fwd[0] - right[0] * fwd[2],
        right[0] * fwd[1] - right[1] * fwd[0],
    ];
    let half_h = tan(settings.fov as f32 * (core::f32::consts::PI / 360.0));
    let half_w = half_h * rw as f32 / rh as f32;

    let eye_cell = world.get(ifloor(cam.eye[0]), ifloor(cam.eye[1]), ifloor(cam.eye[2]));
    let eye_in_water = def(eye_cell).layer == Layer::Water;
    let mut max_dist = settings.view_distance as f32;
    if eye_in_water {
        max_dist = max_dist.min(8.0);
    }
    // Brightness levels: the setting lifts the darkest values.
    let b = settings.brightness as f32;
    let night_floor = 0.12 + 0.05 * b;
    let sun_light = ((night_floor + (1.0 - night_floor) * sky.daylight) * 256.0) as u32;
    let cave_light = ((0.06 + 0.06 * b) * 256.0) as u32;

    let scene = Scene {
        world,
        sky,
        eye: cam.eye,
        max_dist,
        fog_start: max_dist * 0.7,
        sun_light,
        cave_light,
        target,
        eye_in_water,
        pixel_angle: 2.0 * half_h / rh as f32,
        ceiling: world.ceiling().min(HEIGHT as i32 - 1),
    };

    let mut buf = [0u16; SCREEN_W * MAX_PIXEL];
    let center_y = VIEW_H / 2;
    for j in 0..rh {
        let v = (1.0 - 2.0 * (j as f32 + 0.5) / rh as f32) * half_h;
        let row = [fwd[0] + up[0] * v, fwd[1] + up[1] * v, fwd[2] + up[2] * v];
        for i in 0..rw {
            let u = (2.0 * (i as f32 + 0.5) / rw as f32 - 1.0) * half_w;
            let d = [row[0] + right[0] * u, row[1] + right[1] * u, row[2] + right[2] * u];
            let inv = 1.0 / sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
            let c = scene.cast([d[0] * inv, d[1] * inv, d[2] * inv]);
            for k in 0..ps {
                buf[i * ps + k] = c;
            }
        }
        // Repeat the row `ps` times, then add the crosshair.
        for r in 1..ps {
            buf.copy_within(0..SCREEN_W, r * SCREEN_W);
        }
        for r in 0..ps {
            let y = j * ps + r;
            let line = &mut buf[r * SCREEN_W..(r + 1) * SCREEN_W];
            let cx = SCREEN_W / 2;
            if y == center_y {
                for px in &mut line[cx - 5..=cx + 5] {
                    *px = !*px;
                }
            } else if y + 5 >= center_y && y <= center_y + 5 {
                line[cx] = !line[cx];
            }
        }
        p.push_rect(0, (j * ps) as u16, SCREEN_W as u16, ps as u16, &buf[..SCREEN_W * ps]);
    }
}

impl Scene<'_> {
    /// Colour seen along unit direction `d` from the eye.
    #[inline]
    fn cast(&self, d: [f32; 3]) -> u16 {
        let o = self.eye;
        let mut cell = [ifloor(o[0]), ifloor(o[1]), ifloor(o[2])];
        let mut step = [0i32; 3];
        let mut t_delta = [f32::INFINITY; 3];
        let mut t_max = [f32::INFINITY; 3];
        for a in 0..3 {
            if d[a] > 0.0 {
                step[a] = 1;
                t_delta[a] = 1.0 / d[a];
                t_max[a] = (cell[a] as f32 + 1.0 - o[a]) * t_delta[a];
            } else if d[a] < 0.0 {
                step[a] = -1;
                t_delta[a] = -1.0 / d[a];
                t_max[a] = (o[a] - cell[a] as f32) * t_delta[a];
            }
        }
        // Where the ray entered water and the colour of the water there
        // (None = not in water).
        let mut water_from: Option<(f32, u16)> = if self.eye_in_water {
            Some((0.0, scale(WATER_COLOR, self.sun_light.max(self.cave_light * 2))))
        } else {
            None
        };
        loop {
            let axis = if t_max[0] < t_max[1] {
                if t_max[0] < t_max[2] { 0 } else { 2 }
            } else if t_max[1] < t_max[2] {
                1
            } else {
                2
            };
            let t = t_max[axis];
            if t > self.max_dist {
                return self.finish(self.sky.fog, self.max_dist, water_from, true, d);
            }
            cell[axis] += step[axis];
            t_max[axis] += t_delta[axis];
            let y = cell[1];
            // Above every block of the window: a ray that isn't going down
            // can only reach the sky.
            if y > self.ceiling {
                if step[1] >= 0 {
                    return self.finish(self.sky.color(d), t, water_from, true, d);
                }
                continue;
            }
            let b = if y < 0 { BEDROCK } else { self.world.get_fast(cell[0], y, cell[2]) };
            if b == AIR {
                continue;
            }
            let bd = def(b);
            if bd.layer == Layer::Water {
                if water_from.is_none() {
                    // Surface colour from the water texture, for ripples.
                    let hit = [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
                    let (fu, fv) = match axis {
                        0 => (hit[2] - cell[2] as f32, hit[1] - y as f32),
                        2 => (hit[0] - cell[0] as f32, hit[1] - y as f32),
                        _ => (hit[0] - cell[0] as f32, hit[2] - cell[2] as f32),
                    };
                    let tu = ((fu * 16.0) as i32).clamp(0, 15) as usize;
                    let tv = ((fv * 16.0) as i32).clamp(0, 15) as usize;
                    let texel = TEXTURES[bd.tex[0] as usize][tv * 16 + tu];
                    let lit = y + 1 >= HEIGHT as i32 || self.world.sees_sky(cell[0], y + 1, cell[2]);
                    let light = if lit { self.sun_light } else { self.cave_light * 2 };
                    water_from = Some((t, scale(texel, light)));
                }
                continue;
            }
            // Exact hit point and the texel under it.
            let hit = [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
            let (fu, fv, face) = match axis {
                0 => (hit[2] - cell[2] as f32, hit[1] - y as f32, 1),
                2 => (hit[0] - cell[0] as f32, hit[1] - y as f32, 1),
                _ => (hit[0] - cell[0] as f32, hit[2] - cell[2] as f32, if step[1] < 0 { 0 } else { 2 }),
            };
            let tu = ((fu * 16.0) as i32).clamp(0, 15) as usize;
            let tv = if axis == 1 {
                ((fv * 16.0) as i32).clamp(0, 15) as usize
            } else {
                (15 - (fv * 16.0) as i32).clamp(0, 15) as usize
            };
            let mut c = TEXTURES[bd.tex[face] as usize][tv * 16 + tu];
            if c == TRANSPARENT {
                continue; // see-through texel of leaves or glass
            }

            // Lighting.
            let shade: u32 = match (axis, face) {
                (1, 0) => 256,
                (1, _) => 128,
                (0, _) => 154,
                _ => 205,
            };
            let light = if bd.layer == Layer::Lava {
                256
            } else {
                // The air cell the ray came from decides if the face sees the sky.
                let (nx, ny, nz) = (
                    cell[0] - if axis == 0 { step[0] } else { 0 },
                    y - if axis == 1 { step[1] } else { 0 },
                    cell[2] - if axis == 2 { step[2] } else { 0 },
                );
                let lit = ny >= HEIGHT as i32 || (ny >= 0 && self.world.sees_sky(nx, ny, nz));
                ((if lit { self.sun_light } else { self.cave_light }) * shade) >> 8
            };
            c = scale(c, light);

            // Outline and cracks on the targeted block.
            if let Some(tg) = self.target
                && tg.cell == [cell[0], y, cell[2]]
            {
                let w = (self.pixel_angle * t * 0.8).clamp(0.01, 0.08);
                let edge = fu < w || fu > 1.0 - w || fv < w || fv > 1.0 - w;
                if edge {
                    c = OUTLINE;
                } else if tg.crack > 0 && (CRACKS[(tg.crack - 1).min(9) as usize][tv] >> tu) & 1 == 1 {
                    c = scale(c, 90);
                }
            }
            return self.finish(c, t, water_from, false, d);
        }
    }

    /// Applies water tint and fog to a hit (or sky) colour at distance `t`.
    #[inline]
    fn finish(&self, c: u16, t: f32, water_from: Option<(f32, u16)>, is_sky: bool, _d: [f32; 3]) -> u16 {
        let mut c = c;
        if let Some((w, water)) = water_from {
            // The deeper the ray went through water, the more it shows.
            let depth = t - w;
            let k = 120.0 + depth * 18.0;
            c = mix(c, water, if k > 235.0 { 235 } else { k as u32 });
        }
        if !is_sky && t > self.fog_start {
            let f = (t - self.fog_start) / (self.max_dist - self.fog_start);
            c = mix(c, self.sky.fog, (f * 256.0) as u32);
        }
        c
    }
}

/// The first block (not air or liquid) within `reach` along unit direction
/// `d` from `eye`, and the face it was entered through (as the offset of the
/// cell in front of that face).
pub fn pick(world: &World, eye: [f32; 3], d: [f32; 3], reach: f32) -> Option<([i32; 3], [i32; 3])> {
    let mut cell = [ifloor(eye[0]), ifloor(eye[1]), ifloor(eye[2])];
    let mut step = [0i32; 3];
    let mut t_delta = [f32::INFINITY; 3];
    let mut t_max = [f32::INFINITY; 3];
    for a in 0..3 {
        if abs(d[a]) > 1e-9 {
            step[a] = if d[a] > 0.0 { 1 } else { -1 };
            t_delta[a] = 1.0 / abs(d[a]);
            let edge = if d[a] > 0.0 { cell[a] as f32 + 1.0 - eye[a] } else { eye[a] - cell[a] as f32 };
            t_max[a] = edge * t_delta[a];
        }
    }
    loop {
        let axis = (0..3).fold(0, |m, a| if t_max[a] < t_max[m] { a } else { m });
        if t_max[axis] > reach {
            return None;
        }
        cell[axis] += step[axis];
        t_max[axis] += t_delta[axis];
        let b = world.get(cell[0], cell[1], cell[2]);
        if !matches!(def(b).layer, Layer::Air | Layer::Water | Layer::Lava) {
            let mut normal = [0; 3];
            normal[axis] = -step[axis];
            return Some((cell, normal));
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;

    #[test]
    fn pick_finds_the_floor() {
        let mut w = Box::new(World::new());
        w.reset(3);
        w.update(0, 0, usize::MAX);
        for y in 0..HEIGHT as i32 {
            w.set(0, y, 0, if y <= 10 { STONE } else { AIR });
        }
        let hit = pick(&w, [0.5, 12.6, 0.5], [0.0, -1.0, 0.0], 4.5);
        assert_eq!(hit, Some(([0, 10, 0], [0, 1, 0])));
        assert_eq!(pick(&w, [0.5, 20.0, 0.5], [0.0, -1.0, 0.0], 4.5), None);
    }
}
