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
use crate::models::{EntityScene, MAX_VIEWS};
use crate::ramcache::RamTables;
use crate::settings::Settings;
use crate::sky::Sky;
use crate::world::World;
use crate::worldgen::HEIGHT;
use crate::{Platform, SCREEN_W};

/// Height of the 3D view on screen (the rest is the status bar / hotbar).
pub const VIEW_H: usize = 200;
/// Largest supported pixel size.
const MAX_PIXEL: usize = 4;
/// Screen rows sent to the display in one go. Each send is a call into the
/// calculator's system (with a fixed cost), so rows are batched.
pub const BATCH_ROWS: usize = 8;
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
    /// Torches close enough to light what is visible.
    torches: Torches,
    /// RAM copies of the block table and textures.
    ram: &'a RamTables,
}

/// Most torches taken into account in one frame (the nearest ones).
pub const MAX_TORCHES: usize = 16;

/// Torches near the camera, for block light.
#[derive(Clone, Copy)]
pub struct Torches {
    /// Positions.
    pub pos: [[i32; 3]; MAX_TORCHES],
    /// How many are used.
    pub count: usize,
}

impl Torches {
    /// The torches within `radius` blocks of `c` (nearest first if there
    /// are too many).
    pub fn near(world: &World, c: [f32; 3], radius: i32) -> Torches {
        let mut t = Torches { pos: [[0; 3]; MAX_TORCHES], count: 0 };
        let (cx, cy, cz) = (ifloor(c[0]), ifloor(c[1]), ifloor(c[2]));
        world.for_each_torch(|p| {
            let d = (p[0] - cx).abs() + (p[1] - cy).abs() + (p[2] - cz).abs();
            if d <= radius && t.count < MAX_TORCHES {
                t.pos[t.count] = p;
                t.count += 1;
            }
        });
        t
    }

    /// Torch light at a cell (x256): Minecraft's block light falls by one
    /// level per block from 14 at the torch (walls are ignored here).
    #[inline]
    pub fn light(&self, x: i32, y: i32, z: i32) -> u32 {
        let mut level = 0;
        for p in &self.pos[..self.count] {
            let d = (p[0] - x).abs() + (p[1] - y).abs() + (p[2] - z).abs();
            level = level.max(14 - d);
        }
        if level <= 0 {
            return 0;
        }
        // Brightness curve: dim light falls off quickly, like on PC.
        let l = level as u32;
        l * l * 256 / 196
    }
}

/// Fixed-point units per block for distances in the marching loop
/// (20 fraction bits: about a millionth of a block).
const FIX: f32 = (1u32 << 20) as f32;
/// Fixed-point "never": 1024 blocks, far past any view distance, and small
/// enough that adding one more step can't overflow a `u32`.
const FIX_NEVER: u32 = 1 << 30;

/// One block in fixed point.
const FIX_ONE: u32 = 1 << 20;

/// How many steps of `dt` an axis whose next boundary is at `tm` takes
/// before reaching distance `te` (boundaries at exactly `te` excluded).
#[inline(always)]
fn steps_before(tm: u32, dt: u32, te: u32) -> u32 {
    if tm >= te { 0 } else { (te - tm).div_ceil(dt) }
}

/// Distance in blocks (>= 0) to fixed point, saturating at `FIX_NEVER`.
#[inline(always)]
fn to_fix(x: f32) -> u32 {
    let v = x * FIX;
    if v >= FIX_NEVER as f32 { FIX_NEVER } else { v as u32 }
}

/// Fixed-point distance back to blocks.
#[inline(always)]
fn fix_to_f32(t: u32) -> f32 {
    t as f32 * (1.0 / FIX)
}

/// A ray's fixed set-up, to find its cell when it hits something.
struct Ray {
    /// Cell holding the eye.
    start: [i32; 3],
    /// Step sign per axis.
    s: [i32; 3],
    /// Distance to the first cell boundary per axis (fixed point).
    tm0: [u32; 3],
    /// Distance between cell boundaries per axis (fixed point).
    dt: [u32; 3],
}

impl Ray {
    /// The current cell, from the distances to the next boundaries `tm`:
    /// on each axis `tm = tm0 + k * dt` exactly, where `k` is the number of
    /// steps taken along it.
    #[inline]
    fn cell(&self, tm: [u32; 3]) -> [i32; 3] {
        let mut c = self.start;
        for a in 0..3 {
            c[a] += self.s[a] * ((tm[a] - self.tm0[a]) / self.dt[a]) as i32;
        }
        c
    }
}

impl Scene<'_> {
    #[inline]
    fn torch_light(&self, x: i32, y: i32, z: i32) -> u32 {
        if self.torches.count == 0 { 0 } else { self.torches.light(x, y, z) }
    }
}

/// Brightness (x256) of places that see the sky and of places that don't
/// (caves). The brightness setting lifts the darkest values.
pub fn light_levels(sky: &Sky, settings: &Settings) -> (u32, u32) {
    let b = settings.brightness as f32;
    let night_floor = 0.12 + 0.05 * b;
    let sun_light = ((night_floor + (1.0 - night_floor) * sky.daylight) * 256.0) as u32;
    let cave_light = ((0.06 + 0.06 * b) * 256.0) as u32;
    (sun_light, cave_light)
}

/// Draws the 3D view (screen rows `0..VIEW_H`), one ray per
/// `pixel_size` x `pixel_size` screen pixels.
#[allow(clippy::too_many_arguments)]
pub fn render(
    p: &mut dyn Platform,
    world: &World,
    sky: &Sky,
    cam: Camera,
    settings: &Settings,
    target: Option<Target>,
    ents: &mut EntityScene,
    pixel_size: usize,
    lines: &mut [u16; SCREEN_W * BATCH_ROWS],
    ram: &RamTables,
) {
    // One ray per `ps` x `ps` screen pixels. 320x200 isn't always a
    // multiple of `ps` (3), so the last column/row is cut off.
    let ps = pixel_size.clamp(1, MAX_PIXEL);
    let (rw, rh) = (SCREEN_W.div_ceil(ps), VIEW_H.div_ceil(ps));
    let (sw, sh) = (SCREEN_W as f32 / ps as f32, VIEW_H as f32 / ps as f32);

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
    let half_w = half_h * SCREEN_W as f32 / VIEW_H as f32;

    let eye_cell = world.get(ifloor(cam.eye[0]), ifloor(cam.eye[1]), ifloor(cam.eye[2]));
    let eye_in_water = def(eye_cell).layer == Layer::Water;
    let mut max_dist = settings.view_distance as f32;
    if eye_in_water {
        max_dist = max_dist.min(8.0);
    }
    let (sun_light, cave_light) = light_levels(sky, settings);

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
        pixel_angle: 2.0 * half_h / sh,
        ceiling: world.ceiling().min(HEIGHT as i32 - 1),
        torches: Torches::near(world, cam.eye, max_dist as i32 + 14),
        ram,
    };

    ents.place(cam.eye, fwd, right, up, half_w, half_h, rw, rh);

    // Rows waiting in `lines` and the screen row where they start.
    let (mut filled, mut first_row) = (0usize, 0usize);
    let center_y = VIEW_H / 2;
    for j in 0..rh {
        let v = (1.0 - 2.0 * (j as f32 + 0.5) / sh) * half_h;
        let row = [fwd[0] + up[0] * v, fwd[1] + up[1] * v, fwd[2] + up[2] * v];
        // Entities whose screen rectangle covers this row.
        let mut active = [0usize; MAX_VIEWS];
        let mut n_active = 0;
        for (k, view) in ents.views[..ents.count].iter().enumerate() {
            if view.rect[1] <= j as i32 && j as i32 <= view.rect[3] {
                active[n_active] = k;
                n_active += 1;
            }
        }
        for i in 0..rw {
            let u = (2.0 * (i as f32 + 0.5) / sw - 1.0) * half_w;
            let d = [row[0] + right[0] * u, row[1] + right[1] * u, row[2] + right[2] * u];
            let inv = 1.0 / sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
            let d = [d[0] * inv, d[1] * inv, d[2] * inv];
            let (mut c, t) = scene.cast(d);
            if n_active > 0 {
                let mut best = t.min(max_dist);
                for &k in &active[..n_active] {
                    let r = ents.views[k].rect;
                    if (i as i32) < r[0] || (i as i32) > r[2] {
                        continue;
                    }
                    if let Some((te, ce)) = ents.hit(k, i as i32, j as i32, cam.eye, d, best, ram) {
                        best = te;
                        c = scene.finish(ce, te, None, false, d);
                    }
                }
            }
            let base = filled * SCREEN_W + i * ps;
            for k in 0..ps.min(SCREEN_W - i * ps) {
                lines[base + k] = c;
            }
        }
        // Repeat the row `ps` times (fewer for a cut-off last row), then
        // add the crosshair.
        let rows = ps.min(VIEW_H - j * ps);
        let row0 = filled * SCREEN_W;
        for r in 1..rows {
            lines.copy_within(row0..row0 + SCREEN_W, row0 + r * SCREEN_W);
        }
        for r in 0..rows {
            let y = j * ps + r;
            let line = &mut lines[row0 + r * SCREEN_W..row0 + (r + 1) * SCREEN_W];
            let cx = SCREEN_W / 2;
            if y == center_y {
                for px in &mut line[cx - 5..=cx + 5] {
                    *px = !*px;
                }
            } else if y + 5 >= center_y && y <= center_y + 5 {
                line[cx] = !line[cx];
            }
        }
        filled += rows;
        // Send when the batch is full (or this was the last row).
        if filled + ps > BATCH_ROWS || j + 1 == rh {
            p.push_rect(0, first_row as u16, SCREEN_W as u16, filled as u16, &lines[..SCREEN_W * filled]);
            first_row += filled;
            filled = 0;
        }
    }
}

impl Scene<'_> {
    /// Colour seen along unit direction `d` from the eye.
    #[inline]
    /// Returns the colour and the distance of what was hit (infinite for sky).
    fn cast(&self, d: [f32; 3]) -> (u16, f32) {
        // One copy of the marching loop per direction octant, so the step
        // signs are constants inside it (an axis the ray doesn't move along
        // counts as positive; it is never stepped).
        match (d[0] < 0.0, d[1] < 0.0, d[2] < 0.0) {
            (false, false, false) => self.march::<1, 1, 1>(d),
            (false, false, true) => self.march::<1, 1, -1>(d),
            (false, true, false) => self.march::<1, -1, 1>(d),
            (false, true, true) => self.march::<1, -1, -1>(d),
            (true, false, false) => self.march::<-1, 1, 1>(d),
            (true, false, true) => self.march::<-1, 1, -1>(d),
            (true, true, false) => self.march::<-1, -1, 1>(d),
            (true, true, true) => self.march::<-1, -1, -1>(d),
        }
    }

    /// The ray marching loop for one direction octant (step signs `SX`,
    /// `SY`, `SZ`).
    ///
    /// This is the hottest code in the game, written so that everything it
    /// touches fits in the calculator CPU's registers:
    ///
    /// * distances along the ray are **fixed-point integers** (`FIX` units
    ///   per block), so stepping needs no floating-point maths at all;
    /// * the step signs are constants, so moving the storage index is one or
    ///   two instructions per step;
    /// * the cell coordinates are not tracked step by step: each axis's
    ///   distance is `start + k * delta` exactly, so the number of steps `k`
    ///   (and the cell) is recovered by one division when something is hit.
    ///
    /// Shading a hit (and water, plants, see-through texels) lives in a
    /// separate function, as it happens once or twice per ray.
    #[inline(never)]
    fn march<const SX: i32, const SY: i32, const SZ: i32>(&self, d: [f32; 3]) -> (u16, f32) {
        let o = self.eye;
        let s = [SX, SY, SZ];
        let start = [ifloor(o[0]), ifloor(o[1]), ifloor(o[2])];
        let mut dt = [FIX_NEVER; 3];
        let mut tm0 = [FIX_NEVER; 3];
        for a in 0..3 {
            if d[a] != 0.0 {
                let inv = 1.0 / abs(d[a]);
                dt[a] = to_fix(inv).max(1);
                let dist = if s[a] > 0 { start[a] as f32 + 1.0 - o[a] } else { o[a] - start[a] as f32 };
                tm0[a] = to_fix(dist * inv);
            }
        }
        let ray = Ray { start, s, tm0, dt };
        let max = to_fix(self.max_dist);
        let ceiling = self.ceiling as u32;
        let (mut idx, add, mask) = World::index_stepper(start, s);
        let [mut t0, mut t1, mut t2] = tm0;
        let [d0, d1, d2] = dt;
        // Where the ray entered water and the colour of the water there
        // (None = not in water).
        let mut water_from: Option<(f32, u16)> = if self.eye_in_water {
            Some((0.0, scale(WATER_COLOR, self.sun_light.max(self.cave_light * 2))))
        } else {
            None
        };
        let fog = |water_from| (self.finish(self.sky.fog, self.max_dist, water_from, true, d), f32::INFINITY);
        loop {
            // March until a filled cell (or the end of the ray).
            let (axis, t, b) = loop {
                // Step along the axis whose next cell boundary is nearest.
                // Every step moves forward by at least the smallest delta,
                // so the `max` test always ends the loop.
                let (axis, t) = if t0 < t1 {
                    if t0 < t2 { (0, t0) } else { (2, t2) }
                } else if t1 < t2 {
                    (1, t1)
                } else {
                    (2, t2)
                };
                if t > max {
                    return fog(water_from);
                }
                // Move the storage index one cell along `axis` (wrapping x
                // and z inside the window); `add`/`mask` are constants here.
                match axis {
                    0 => t0 += d0,
                    1 => t1 += d1,
                    _ => t2 += d2,
                }
                idx = (idx & !mask[axis]) | (idx.wrapping_add(add[axis]) & mask[axis]);
                let y = World::index_y(idx);
                // One unsigned compare catches both "below the world" (y < 0
                // wraps to a huge number) and "above every block".
                if y as u32 > ceiling {
                    if y < 0 {
                        break (axis, t, BEDROCK);
                    }
                    // Above every block of the window: a ray that isn't
                    // going down can only reach the sky.
                    if SY > 0 {
                        return (
                            self.finish(self.sky.color(d), fix_to_f32(t), water_from, true, d),
                            f32::INFINITY,
                        );
                    }
                    continue;
                }
                if !self.world.is_filled(idx) {
                    // Sky skipping: a ray that isn't going down, and is
                    // already above every block of this 4x4 group of
                    // columns, can't hit anything until it leaves the group.
                    if SY > 0 && y > self.world.group_high_at(idx) {
                        // Distance to where the ray leaves the group: on x
                        // and z, the cells left inside the group come from
                        // the index's low bits (groups are 4 cells wide).
                        let (xl, zl) = World::index_xz(idx);
                        let nx = if SX > 0 { 3 - (xl & 3) } else { xl & 3 };
                        let nz = if SZ > 0 { 3 - (zl & 3) } else { zl & 3 };
                        let ex = t0.saturating_add(nx * d0);
                        let ez = t2.saturating_add(nz * d2);
                        let te = ex.min(ez);
                        if te > max {
                            return fog(water_from);
                        }
                        // Only worth it when the jump skips more than a cell.
                        if te > t + FIX_ONE {
                            // Advance every axis to just before `te`: the exit
                            // axis by its cells left, the others by the
                            // boundaries they cross before `te`.
                            let (m0, m2) = if ex <= ez {
                                (nx, steps_before(t2, d2, te))
                            } else {
                                (steps_before(t0, d0, te), nz)
                            };
                            let m1 = steps_before(t1, d1, te);
                            t0 += m0 * d0;
                            t1 += m1 * d1;
                            t2 += m2 * d2;
                            for (a, m) in [(0, m0), (1, m1), (2, m2)] {
                                idx = (idx & !mask[a]) | (idx.wrapping_add(add[a].wrapping_mul(m)) & mask[a]);
                            }
                        }
                    }
                    continue;
                }
                let b = self.world.get_index(idx);
                if b != AIR {
                    break (axis, t, b);
                }
            };
            let cell = ray.cell([t0, t1, t2]);
            let t_exit = fix_to_f32(t0.min(t1).min(t2));
            if let Some(hit) = self.surface(b, axis, s, cell, fix_to_f32(t), t_exit, d, &mut water_from) {
                return hit;
            }
        }
    }

    /// Colour of block `b` hit by the ray, or `None` if the ray goes on
    /// (water, see-through texels, missed plant planes). `axis` is the axis
    /// of the face it crossed, `t` where it entered the cell and `t_exit`
    /// where it leaves it.
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    fn surface(
        &self,
        b: Block,
        axis: usize,
        step: [i32; 3],
        cell: [i32; 3],
        t: f32,
        t_exit: f32,
        d: [f32; 3],
        water_from: &mut Option<(f32, u16)>,
    ) -> Option<(u16, f32)> {
        let o = self.eye;
        let y = cell[1];
        let bd = self.ram.def(b);
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
                let texel = self.ram.texel(bd.tex[0], tv * 16 + tu);
                let lit = y + 1 >= HEIGHT as i32 || self.world.sees_sky(cell[0], y + 1, cell[2]);
                let light = if lit { self.sun_light } else { self.cave_light * 2 };
                *water_from = Some((t, scale(texel, light)));
            }
            return None;
        }
        if bd.layer == Layer::Cross {
            // Plants and torches: two see-through planes along the
            // cell's diagonals (local x - z = 0 and x + z = 1).
            let (lx, lz) = (o[0] - cell[0] as f32, o[2] - cell[2] as f32);
            let mut best: Option<(f32, u16)> = None;
            for (den, num) in [(d[0] - d[2], lz - lx), (d[0] + d[2], 1.0 - lx - lz)] {
                if den.abs() < 1e-6 {
                    continue;
                }
                let tp = num / den;
                if tp < t - 1e-4 || tp > t_exit + 1e-4 || best.is_some_and(|(bt, _)| tp >= bt) {
                    continue;
                }
                let (px, py) = (lx + d[0] * tp, o[1] + d[1] * tp - y as f32);
                if !(0.0..1.0).contains(&px) || !(0.0..1.0).contains(&py) {
                    continue;
                }
                let tu = ((px * 16.0) as usize).min(15);
                let tv = (15 - (py * 16.0) as i32).clamp(0, 15) as usize;
                let texel = self.ram.texel(bd.tex[1], tv * 16 + tu);
                if texel != TRANSPARENT {
                    best = Some((tp, texel));
                }
            }
            let (tp, texel) = best?;
            let light = if bd.light > 0 {
                256 // torches glow
            } else {
                let base =
                    if self.world.sees_sky(cell[0], y, cell[2]) { self.sun_light } else { self.cave_light };
                base.max(self.torch_light(cell[0], y, cell[2]))
            };
            let mut c = scale(texel, (light * 230) >> 8);
            if self.target.is_some_and(|tg| tg.cell == [cell[0], y, cell[2]]) {
                c = mix(c, 0xFFFF, 60); // targeted plant: slightly brighter
            }
            return Some((self.finish(c, tp, *water_from, false, d), tp));
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
        let mut c = self.ram.texel(bd.tex[face], tv * 16 + tu);
        if c == TRANSPARENT {
            return None; // see-through texel of leaves or glass
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
            let base = if lit { self.sun_light } else { self.cave_light };
            (base.max(self.torch_light(nx, ny, nz)) * shade) >> 8
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
        Some((self.finish(c, t, *water_from, false, d), t))
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
