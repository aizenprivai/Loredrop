//! Drawing creatures, dropped items and arrows in the ray-cast view.
//!
//! Each thing is a handful of boxes (the PC game's box models: body, head,
//! legs...). Once per frame, [`build`] turns every visible entity into
//! oriented boxes in world space (with the walk animation applied) and
//! works out which rectangle of the screen it can cover. Then, for each
//! pixel inside that rectangle, the renderer intersects the pixel's ray
//! with the boxes ([`EntityScene::hit`]); if a box is closer than the
//! terrain, its textured face is shown instead.

use crate::blocks::{TEXTURES, TRANSPARENT};
use crate::color::{hex, mix, scale};
use crate::entities::Entities;
use crate::items::{ARROW, block_of, item_def};
use crate::math::{cos, ifloor, sin, sqrt};
use crate::mobs::{Role, mob_def};
use crate::world::World;

/// Most entities drawn in one frame (the nearest ones).
pub const MAX_VIEWS: usize = 10;
/// Most boxes per entity (the spider has 11).
pub const MAX_PARTS: usize = 11;

/// A box with any orientation.
#[derive(Clone, Copy, Default)]
pub struct Obb {
    center: [f32; 3],
    /// Unit axes (local x, y, z) in world space.
    axes: [[f32; 3]; 3],
    half: [f32; 3],
    /// Texture of each face: -x, +x, -y, +y, -z, +z.
    tex: [u8; 6],
    /// Screen rectangle (render pixels, inclusive) the box can cover, set
    /// by `place`; pixels outside it skip the box entirely.
    rect: [i16; 4],
}

/// One entity ready to draw.
#[derive(Clone, Copy)]
pub struct EntityView {
    parts: [Obb; MAX_PARTS],
    count: usize,
    /// Bounding sphere.
    center: [f32; 3],
    radius: f32,
    /// Light level (x256) at the entity.
    light: u32,
    /// Hurt flash (red) / burning (orange) tint, 0 = none.
    tint: u16,
    /// Screen rectangle in render pixels (inclusive), set by `place`.
    pub rect: [i32; 4],
}

impl EntityView {
    const EMPTY: EntityView = EntityView {
        parts: [Obb { center: [0.0; 3], axes: [[0.0; 3]; 3], half: [0.0; 3], tex: [0; 6], rect: [0; 4] };
            MAX_PARTS],
        count: 0,
        center: [0.0; 3],
        radius: 0.0,
        light: 256,
        tint: 0,
        rect: [0; 4],
    };
}

/// All entities to draw this frame. Big (~8 KB): kept in the game state,
/// not on the stack.
pub struct EntityScene {
    /// The entities.
    pub views: [EntityView; MAX_VIEWS],
    /// How many are used.
    pub count: usize,
}

impl Default for EntityScene {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Rotates `v` by `r` (x, then... applied as Rz, then Ry, then Rx).
fn rotate(v: [f32; 3], r: [f32; 3]) -> [f32; 3] {
    let (sx, cx, sy, cy, sz, cz) = (sin(r[0]), cos(r[0]), sin(r[1]), cos(r[1]), sin(r[2]), cos(r[2]));
    // Z
    let v = [v[0] * cz - v[1] * sz, v[0] * sz + v[1] * cz, v[2]];
    // Y
    let v = [v[0] * cy + v[2] * sy, v[1], -v[0] * sy + v[2] * cy];
    // X
    [v[0], v[1] * cx - v[2] * sx, v[1] * sx + v[2] * cx]
}

/// Facing basis for a yaw angle: local x, y, z in world space.
fn yaw_basis(yaw: f32) -> [[f32; 3]; 3] {
    let (s, c) = (sin(yaw), cos(yaw));
    [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]
}

fn to_world(b: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        b[0][0] * v[0] + b[1][0] * v[1] + b[2][0] * v[2],
        b[0][1] * v[0] + b[1][1] * v[1] + b[2][1] * v[2],
        b[0][2] * v[0] + b[1][2] * v[1] + b[2][2] * v[2],
    ]
}

impl EntityScene {
    /// An empty scene.
    pub const fn new() -> Self {
        Self { views: [EntityView::EMPTY; MAX_VIEWS], count: 0 }
    }

    fn push(&mut self) -> Option<&mut EntityView> {
        if self.count == MAX_VIEWS {
            return None;
        }
        self.count += 1;
        let v = &mut self.views[self.count - 1];
        v.count = 0;
        v.tint = 0;
        Some(v)
    }

    /// Ray `o + t d` against every box of view `vi`: nearest hit distance
    /// and colour (fog not applied).
    /// `(px, py)` is the render pixel (boxes whose rectangle misses it are skipped).
    #[allow(clippy::too_many_arguments)]
    pub fn hit(
        &self,
        vi: usize,
        px: i32,
        py: i32,
        o: [f32; 3],
        d: [f32; 3],
        max_t: f32,
    ) -> Option<(f32, u16)> {
        let v = &self.views[vi];
        let mut best: Option<(f32, u16)> = None;
        let (px, py) = (px as i16, py as i16);
        for b in &v.parts[..v.count] {
            if px < b.rect[0] || px > b.rect[2] || py < b.rect[1] || py > b.rect[3] {
                continue;
            }
            let rel = [o[0] - b.center[0], o[1] - b.center[1], o[2] - b.center[2]];
            let lo = [dot(rel, b.axes[0]), dot(rel, b.axes[1]), dot(rel, b.axes[2])];
            let ld = [dot(d, b.axes[0]), dot(d, b.axes[1]), dot(d, b.axes[2])];
            let (mut t0, mut t1) = (0.0f32, max_t);
            let mut face = 0usize;
            let mut ok = true;
            for a in 0..3 {
                if ld[a].abs() < 1e-7 {
                    if lo[a].abs() > b.half[a] {
                        ok = false;
                        break;
                    }
                    continue;
                }
                let inv = 1.0 / ld[a];
                let mut ta = (-b.half[a] - lo[a]) * inv;
                let mut tb = (b.half[a] - lo[a]) * inv;
                // Entering through the face facing the ray.
                let mut f = a * 2;
                if ta > tb {
                    core::mem::swap(&mut ta, &mut tb);
                    f = a * 2 + 1;
                }
                if ta > t0 {
                    t0 = ta;
                    face = f;
                }
                if tb < t1 {
                    t1 = tb;
                }
                if t0 > t1 {
                    ok = false;
                    break;
                }
            }
            if !ok || best.is_some_and(|(bt, _)| t0 >= bt) {
                continue;
            }
            // Texel on the face: u, v in 0..1 across the face.
            let a = face / 2;
            let p = [lo[0] + ld[0] * t0, lo[1] + ld[1] * t0, lo[2] + ld[2] * t0];
            let norm = |k: usize| ((p[k] / b.half[k]) * 0.5 + 0.5).clamp(0.0, 0.999);
            let (u, w) = match a {
                0 => (norm(2), 1.0 - norm(1)),
                1 => (norm(0), norm(2)),
                _ => (norm(0), 1.0 - norm(1)),
            };
            let texel = TEXTURES[b.tex[face] as usize][((w * 16.0) as usize) * 16 + (u * 16.0) as usize];
            if texel == TRANSPARENT {
                continue;
            }
            let shade = match face {
                3 => 256,
                2 => 128,
                0 | 1 => 160,
                _ => 210,
            };
            let mut c = scale(texel, (v.light * shade) >> 8);
            if v.tint != 0 {
                c = mix(c, v.tint, 110);
            }
            best = Some((t0, c));
        }
        best
    }

    /// Computes each view's screen rectangle for a camera, dropping views
    /// that are off-screen. `half_w`/`half_h` are the view's tangents,
    /// `rw`/`rh` the render size.
    #[allow(clippy::too_many_arguments)]
    pub fn place(
        &mut self,
        eye: [f32; 3],
        fwd: [f32; 3],
        right: [f32; 3],
        up: [f32; 3],
        half_w: f32,
        half_h: f32,
        rw: usize,
        rh: usize,
    ) {
        // Screen rectangle covered by a sphere, or None if off-screen.
        let project = |center: [f32; 3], radius: f32| -> Option<[i32; 4]> {
            let rel = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
            let z = dot(rel, fwd);
            if z < -radius {
                return None;
            }
            let rect = if z <= radius + 0.05 {
                [0, 0, rw as i32 - 1, rh as i32 - 1]
            } else {
                let x = dot(rel, right) / z;
                let y = dot(rel, up) / z;
                let cx = (x / half_w + 1.0) * 0.5 * rw as f32;
                let cy = (1.0 - y / half_h) * 0.5 * rh as f32;
                let r = radius / z / half_w * rw as f32 * 0.5 * 1.15 + 1.0;
                [(cx - r) as i32, (cy - r) as i32, (cx + r) as i32, (cy + r) as i32]
            };
            let rect =
                [rect[0].max(0), rect[1].max(0), rect[2].min(rw as i32 - 1), rect[3].min(rh as i32 - 1)];
            (rect[0] <= rect[2] && rect[1] <= rect[3]).then_some(rect)
        };
        let mut k = 0;
        for i in 0..self.count {
            let mut v = self.views[i];
            let Some(rect) = project(v.center, v.radius) else { continue };
            v.rect = rect;
            // Each box gets its own (smaller) rectangle.
            for b in v.parts[..v.count].iter_mut() {
                let r = sqrt(dot(b.half, b.half));
                b.rect = match project(b.center, r) {
                    Some(r) => [r[0] as i16, r[1] as i16, r[2] as i16, r[3] as i16],
                    None => [1, 1, 0, 0], // empty
                };
            }
            self.views[k] = v;
            k += 1;
        }
        self.count = k;
    }
}

/// Turns the nearest entities into boxes for this frame.
pub fn build(
    scene: &mut EntityScene,
    ents: &Entities,
    world: &World,
    eye: [f32; 3],
    max_dist: f32,
    sun_light: u32,
    cave_light: u32,
) {
    scene.count = 0;
    // Nearest first: mobs, items and arrows by distance.
    let mut cand = [(0u8, 0u8, 0f32); 48];
    let mut n = 0;
    let dist2 = |p: [f32; 3]| {
        (p[0] - eye[0]) * (p[0] - eye[0])
            + (p[1] - eye[1]) * (p[1] - eye[1])
            + (p[2] - eye[2]) * (p[2] - eye[2])
    };
    let max2 = (max_dist + 2.0) * (max_dist + 2.0);
    for (i, m) in ents.mobs.iter().enumerate().filter(|(_, m)| m.used) {
        let d = dist2(m.pos);
        if d < max2 && n < cand.len() {
            cand[n] = (0, i as u8, d);
            n += 1;
        }
    }
    for (i, it) in ents.items.iter().enumerate().filter(|(_, it)| it.used) {
        let d = dist2(it.pos);
        if d < max2 && n < cand.len() {
            cand[n] = (1, i as u8, d);
            n += 1;
        }
    }
    for (i, a) in ents.arrows.iter().enumerate().filter(|(_, a)| a.used) {
        let d = dist2(a.pos);
        if d < max2 && n < cand.len() {
            cand[n] = (2, i as u8, d);
            n += 1;
        }
    }
    cand[..n].sort_unstable_by(|a, b| a.2.total_cmp(&b.2));
    let light_at = |p: [f32; 3]| {
        let (x, y, z) = (ifloor(p[0]), ifloor(p[1] + 0.5), ifloor(p[2]));
        if y >= crate::worldgen::HEIGHT as i32 || world.sees_sky(x, y, z) {
            sun_light
        } else {
            cave_light * 2
        }
    };
    for &(what, i, _) in &cand[..n] {
        let Some(v) = scene.push() else { break };
        match what {
            0 => {
                let m = &ents.mobs[i as usize];
                let def = mob_def(m.kind);
                let basis = yaw_basis(m.yaw);
                for part in def.parts.iter().take(MAX_PARTS) {
                    let swing = sin(m.walk + part.phase as f32 * core::f32::consts::PI) * 0.7 * m.walk_amount;
                    // The PC models' x axis points the other way, which
                    // flips the sense of their y and z rotations here.
                    let mut rot = [part.rot[0], -part.rot[1], -part.rot[2]];
                    match part.role {
                        Role::Leg | Role::Arm => rot[0] += swing,
                        Role::Wing if !m.on_ground => rot[2] += sin(m.walk * 4.0) * 0.6,
                        _ => {}
                    }
                    let local_c = [
                        part.min[0] + part.size[0] / 2.0,
                        part.min[1] + part.size[1] / 2.0,
                        part.min[2] + part.size[2] / 2.0,
                    ];
                    let off = rotate(
                        [local_c[0] - part.pivot[0], local_c[1] - part.pivot[1], local_c[2] - part.pivot[2]],
                        rot,
                    );
                    let c = [part.pivot[0] + off[0], part.pivot[1] + off[1], part.pivot[2] + off[2]];
                    let cw = to_world(&basis, c);
                    let axes = [
                        to_world(&basis, rotate([1.0, 0.0, 0.0], rot)),
                        to_world(&basis, rotate([0.0, 1.0, 0.0], rot)),
                        to_world(&basis, rotate([0.0, 0.0, 1.0], rot)),
                    ];
                    let t = part.tex;
                    v.parts[v.count] = Obb {
                        center: [m.pos[0] + cw[0], m.pos[1] + cw[1], m.pos[2] + cw[2]],
                        axes,
                        half: [part.size[0] / 2.0, part.size[1] / 2.0, part.size[2] / 2.0],
                        tex: [t, t, t, t, t, part.front],
                        rect: [0; 4],
                    };
                    v.count += 1;
                }
                // A carried block (the Hollow) is held in front of the chest.
                if m.carried != crate::blocks::AIR && v.count < MAX_PARTS {
                    let c = to_world(&basis, [0.0, def.height * 0.6, 0.45]);
                    let tx = crate::blocks::def(m.carried).tex;
                    v.parts[v.count] = Obb {
                        center: [m.pos[0] + c[0], m.pos[1] + c[1], m.pos[2] + c[2]],
                        axes: basis,
                        half: [0.25; 3],
                        tex: [tx[1], tx[1], tx[2], tx[0], tx[1], tx[1]],
                        rect: [0; 4],
                    };
                    v.count += 1;
                }
                v.center = [m.pos[0], m.pos[1] + def.height / 2.0, m.pos[2]];
                v.radius = 0.0;
                v.light = light_at(m.pos);
                if m.hurt > 0 {
                    v.tint = hex(0xFF2020);
                } else if m.fire > 0 {
                    v.tint = hex(0xFF8020);
                }
            }
            1 => {
                let it = &ents.items[i as usize];
                let bob = sin(it.age as f32 * 0.1) * 0.05;
                let basis = yaw_basis(it.age as f32 * 0.05);
                let center = [it.pos[0], it.pos[1] + 0.2 + bob, it.pos[2]];
                let (half, tex) = match block_of(it.stack.item) {
                    Some(b) => {
                        let tx = crate::blocks::def(b).tex;
                        ([0.125; 3], [tx[1], tx[1], tx[2], tx[0], tx[1], tx[1]])
                    }
                    None => {
                        let ic = item_def(it.stack.item).icon;
                        ([0.2, 0.2, 0.015], [ic; 6])
                    }
                };
                v.parts[0] = Obb { center, axes: basis, half, tex, rect: [0; 4] };
                v.count = 1;
                v.center = center;
                v.light = light_at(it.pos);
            }
            _ => {
                let a = &ents.arrows[i as usize];
                let len = sqrt(dot(a.vel, a.vel)).max(1e-4);
                let z = [a.vel[0] / len, a.vel[1] / len, a.vel[2] / len];
                let side = sqrt(z[0] * z[0] + z[2] * z[2]).max(1e-4);
                let x = [z[2] / side, 0.0, -z[0] / side];
                let y = [z[1] * x[2] - z[2] * x[1], z[2] * x[0] - z[0] * x[2], z[0] * x[1] - z[1] * x[0]];
                let ic = item_def(ARROW).icon;
                let center = [a.pos[0] - z[0] * 0.25, a.pos[1] - z[1] * 0.25, a.pos[2] - z[2] * 0.25];
                v.parts[0] =
                    Obb { center, axes: [x, y, z], half: [0.03, 0.03, 0.25], tex: [ic; 6], rect: [0; 4] };
                v.count = 1;
                v.center = center;
                v.light = light_at(a.pos);
            }
        }
        // Bounding sphere radius from the boxes' corners.
        let mut r: f32 = 0.0;
        for b in &v.parts[..v.count] {
            let rel = [b.center[0] - v.center[0], b.center[1] - v.center[1], b.center[2] - v.center[2]];
            let ext = b.half[0] + b.half[1] + b.half[2];
            r = r.max(sqrt(dot(rel, rel)) + ext);
        }
        v.radius = r;
    }
}
