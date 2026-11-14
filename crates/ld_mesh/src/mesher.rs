//! Face culling + greedy meshing + ambient occlusion.

use crate::neighborhood::Neighborhood;
use crate::table::MeshTable;
use crate::{PackedVertex, WHITE};
use ld_world::{BlockId, RenderLayer, Shape};

/// Output for one section. Every 4 vertices form one quad; the renderer uses
/// a shared index buffer (0,1,2, 0,2,3, …) so no indices are stored here.
#[derive(Debug, Default, Clone)]
pub struct SectionMesh {
    pub opaque: Vec<PackedVertex>,
    pub cutout: Vec<PackedVertex>,
    /// Translucent faces (water, stained glass), drawn last with blending.
    pub translucent: Vec<PackedVertex>,
}

impl SectionMesh {
    pub fn is_empty(&self) -> bool {
        self.opaque.is_empty() && self.cutout.is_empty() && self.translucent.is_empty()
    }

    pub fn quad_count(&self) -> usize {
        (self.opaque.len() + self.cutout.len() + self.translucent.len()) / 4
    }
}

/// Axes of each face, indexed like `ld_world::Face`: (normal axis, u axis,
/// v axis, normal points toward +). `u × v` points out of the face, so the
/// corners (0,0) → (1,0) → (1,1) → (0,1) are counter-clockwise from outside.
const FACE_AXES: [(usize, usize, usize, bool); 6] = [
    (0, 1, 2, true),  // East  +X: u=Y, v=Z
    (0, 2, 1, false), // West  −X: u=Z, v=Y
    (1, 2, 0, true),  // Up    +Y: u=Z, v=X
    (1, 0, 2, false), // Down  −Y: u=X, v=Z
    (2, 0, 1, true),  // South +Z: u=X, v=Y
    (2, 1, 0, false), // North −Z: u=Y, v=X
];

/// Everything that must match for two faces to merge into one quad.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FaceKey {
    texture: u16,
    tint: u8,
    /// Packed RGB biome colour.
    color: u32,
    /// Four 2-bit AO values, corner order (0,0) (1,0) (1,1) (0,1).
    ao: u8,
    layer: u8, // 0 opaque, 1 cutout, 2 translucent
}

/// Meshes the inner 16³ of `n`.
pub fn mesh_section(n: &Neighborhood, table: &MeshTable) -> SectionMesh {
    let mut out = SectionMesh::default();
    let mut mask: [Option<FaceKey>; 256] = [None; 256];
    let colors = blended_colors(n, table);

    for (face, &(na, ua, va, positive)) in FACE_AXES.iter().enumerate() {
        let step = if positive { 1 } else { -1 };
        for d in 0..16 {
            // 1. Build a 16×16 mask of visible faces in this slice.
            for v in 0..16 {
                for u in 0..16 {
                    let mut p = [0i32; 3];
                    p[na] = d;
                    p[ua] = u;
                    p[va] = v;
                    mask[(v * 16 + u) as usize] = visible_face(n, table, &colors, p, face, na, ua, va, step);
                }
            }
            // 2. Greedily merge equal neighbours into rectangles.
            for v in 0..16usize {
                let mut u = 0usize;
                while u < 16 {
                    let Some(key) = mask[v * 16 + u] else {
                        u += 1;
                        continue;
                    };
                    let mut w = 1;
                    while u + w < 16 && mask[v * 16 + u + w] == Some(key) {
                        w += 1;
                    }
                    let mut h = 1;
                    'grow: while v + h < 16 {
                        for k in 0..w {
                            if mask[(v + h) * 16 + u + k] != Some(key) {
                                break 'grow;
                            }
                        }
                        h += 1;
                    }
                    for dv in 0..h {
                        for du in 0..w {
                            mask[(v + dv) * 16 + u + du] = None;
                        }
                    }
                    emit_quad(
                        &mut out,
                        face,
                        (na, ua, va, positive),
                        d as u32,
                        u as u32,
                        v as u32,
                        w as u32,
                        h as u32,
                        key,
                    );
                    u += w;
                }
            }
        }
    }
    mesh_plants(n, table, &colors, &mut out);
    out
}

/// Blended biome colours per column: `[x + z*16][kind - 1]` for grass,
/// foliage and water, averaging the 3×3 surrounding columns.
fn blended_colors(n: &Neighborhood, table: &MeshTable) -> Vec<[u32; 3]> {
    let mut out = vec![[WHITE; 3]; 256];
    for z in 0..16 {
        for x in 0..16 {
            for kind in 1..=3u8 {
                let mut acc = [0f32; 3];
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let c = table.biome_color(n.biome(x + dx, z + dz), kind);
                        for i in 0..3 {
                            acc[i] += c[i];
                        }
                    }
                }
                let to8 = |v: f32| ((v / 9.0).clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
                out[(z * 16 + x) as usize][kind as usize - 1] =
                    (to8(acc[0]) << 16) | (to8(acc[1]) << 8) | to8(acc[2]);
            }
        }
    }
    out
}

#[inline]
fn color_for(colors: &[[u32; 3]], x: i32, z: i32, tint: u8) -> u32 {
    if tint == 0 { WHITE } else { colors[(z * 16 + x) as usize][tint as usize - 1] }
}

/// Two crossed diagonal panes per plant, each emitted in both windings so it
/// is visible from both sides (back-face culling stays on for everything else).
fn mesh_plants(n: &Neighborhood, table: &MeshTable, colors: &[[u32; 3]], out: &mut SectionMesh) {
    for y in 0..16 {
        for z in 0..16 {
            for x in 0..16 {
                let id = n.get(x, y, z);
                if id == BlockId::AIR {
                    continue;
                }
                let info = table.get(id);
                if info.shape != Shape::Cross {
                    continue;
                }
                let (xu, yu, zu) = (x as u32, y as u32, z as u32);
                let tex = info.textures[0];
                let tint = info.tints[0];
                let color = color_for(colors, x, z, tint);
                let panes: [(u32, [[u32; 3]; 4]); 2] = [
                    (6, [[xu, yu, zu], [xu + 1, yu, zu + 1], [xu + 1, yu + 1, zu + 1], [xu, yu + 1, zu]]),
                    (7, [[xu, yu, zu + 1], [xu + 1, yu, zu], [xu + 1, yu + 1, zu], [xu, yu + 1, zu + 1]]),
                ];
                let target = match info.layer {
                    RenderLayer::Translucent => &mut out.translucent,
                    RenderLayer::Opaque => &mut out.opaque,
                    _ => &mut out.cutout,
                };
                for (face, c) in panes {
                    let v = |p: [u32; 3]| PackedVertex::new(p, face, 3, tex, tint as u32, color);
                    target.extend_from_slice(&[v(c[0]), v(c[1]), v(c[2]), v(c[3])]);
                    target.extend_from_slice(&[v(c[1]), v(c[0]), v(c[3]), v(c[2])]); // back side
                }
            }
        }
    }
}

/// Returns the face key if the given face of the block at `p` is visible.
#[allow(clippy::too_many_arguments)]
#[inline]
fn visible_face(
    n: &Neighborhood,
    table: &MeshTable,
    colors: &[[u32; 3]],
    p: [i32; 3],
    face: usize,
    na: usize,
    ua: usize,
    va: usize,
    step: i32,
) -> Option<FaceKey> {
    let id = n.get(p[0], p[1], p[2]);
    if id == BlockId::AIR {
        return None;
    }
    let info = table.get(id);
    if info.layer == RenderLayer::Invisible || info.shape != Shape::Cube {
        return None;
    }
    let mut q = p;
    q[na] += step;
    let nid = n.get(q[0], q[1], q[2]);
    let ninfo = table.get(nid);
    if ninfo.opaque || (nid == id && info.cull_same) {
        return None;
    }
    // Ambient occlusion for the four corners, sampled around the cell in front.
    let solid = |du: i32, dv: i32| {
        let mut c = q;
        c[ua] += du;
        c[va] += dv;
        table.get(n.get(c[0], c[1], c[2])).opaque as u8
    };
    let corner_ao = |su: i32, sv: i32| -> u8 {
        let (s1, s2, c) = (solid(su, 0), solid(0, sv), solid(su, sv));
        if s1 == 1 && s2 == 1 { 0 } else { 3 - (s1 + s2 + c) }
    };
    let ao = corner_ao(-1, -1) | (corner_ao(1, -1) << 2) | (corner_ao(1, 1) << 4) | (corner_ao(-1, 1) << 6);
    let layer = match info.layer {
        RenderLayer::Cutout => 1,
        RenderLayer::Translucent => 2,
        _ => 0,
    };
    let tint = info.tints[face];
    Some(FaceKey {
        texture: info.textures[face],
        tint,
        color: color_for(colors, p[0], p[2], tint),
        ao,
        layer,
    })
}

#[allow(clippy::too_many_arguments)]
fn emit_quad(
    out: &mut SectionMesh,
    face: usize,
    (na, ua, va, positive): (usize, usize, usize, bool),
    d: u32,
    u: u32,
    v: u32,
    w: u32,
    h: u32,
    key: FaceKey,
) {
    let plane = d + positive as u32;
    let corner = |cu: u32, cv: u32| {
        let mut p = [0u32; 3];
        p[na] = plane;
        p[ua] = cu;
        p[va] = cv;
        p
    };
    let ao = |i: u32| ((key.ao >> (i * 2)) & 3) as u32;
    let verts = [
        PackedVertex::new(corner(u, v), face as u32, ao(0), key.texture, key.tint as u32, key.color),
        PackedVertex::new(corner(u + w, v), face as u32, ao(1), key.texture, key.tint as u32, key.color),
        PackedVertex::new(corner(u + w, v + h), face as u32, ao(2), key.texture, key.tint as u32, key.color),
        PackedVertex::new(corner(u, v + h), face as u32, ao(3), key.texture, key.tint as u32, key.color),
    ];
    let target = match key.layer {
        1 => &mut out.cutout,
        2 => &mut out.translucent,
        _ => &mut out.opaque,
    };
    // The quad is drawn as triangles (0,1,2)+(0,2,3), split along the 0–2
    // diagonal. If that diagonal is the darker one, AO would interpolate
    // into a visible streak, so start at vertex 1 to split along 1–3 instead.
    if ao(0) + ao(2) < ao(1) + ao(3) {
        target.extend_from_slice(&[verts[1], verts[2], verts[3], verts[0]]);
    } else {
        target.extend_from_slice(&verts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ld_world::BlockRegistry;

    const BLOCKS: &str = r#"
        [[block]]
        name = "stone"
        layer = "opaque"
        textures = { all = "stone" }
        [[block]]
        name = "grass_block"
        layer = "opaque"
        textures = { top = "grass_block_top", bottom = "dirt", side = "grass_block_side" }
        tint = { top = "grass" }
        [[block]]
        name = "oak_leaves"
        layer = "cutout"
        textures = { all = "oak_leaves" }
        [[block]]
        name = "glass"
        layer = "cutout"
        cull_same = true
        textures = { all = "glass" }
        [[block]]
        name = "short_grass"
        layer = "cutout"
        shape = "cross"
        textures = { all = "short_grass" }
        tint = { all = "grass" }
        [[block]]
        name = "water"
        layer = "translucent"
        cull_same = true
        textures = { all = "water_still" }
        tint = { all = "water" }
    "#;

    fn setup() -> (BlockRegistry, MeshTable) {
        let r = BlockRegistry::from_toml(BLOCKS).unwrap();
        let names = r.texture_names();
        let t = MeshTable::new(&r, None, |n| names.iter().position(|x| x == n).unwrap_or(0) as u16);
        (r, t)
    }

    fn quads(m: &SectionMesh) -> usize {
        m.quad_count()
    }

    #[test]
    fn single_block_has_six_faces_full_brightness() {
        let (r, t) = setup();
        let stone = r.id("stone").unwrap();
        let n = Neighborhood::from_fn(|x, y, z| if (x, y, z) == (5, 5, 5) { stone } else { BlockId::AIR });
        let m = mesh_section(&n, &t);
        assert_eq!(quads(&m), 6);
        assert!(m.opaque.iter().all(|v| v.unpack().2 == 3), "nothing nearby → no AO");
    }

    #[test]
    fn flat_layer_merges_into_one_quad_per_side() {
        let (r, t) = setup();
        let g = r.id("grass_block").unwrap();
        // A 16×16 grass layer at y=0 with stone below (outside the section too).
        let n = Neighborhood::from_fn(|_, y, _| match y {
            0 => g,
            -1 => r.id("stone").unwrap(),
            _ => BlockId::AIR,
        });
        let m = mesh_section(&n, &t);
        // Top: 1 merged quad. Bottom: hidden by stone. Sides: the border
        // neighbours are grass too (from_fn fills the border), so hidden.
        assert_eq!(quads(&m), 1);
        let (_, face, _, tex, tint) = m.opaque[0].unpack();
        assert_eq!(face, ld_world::Face::Up as u32);
        assert_eq!(tint, 1, "grass top is tinted");
        assert_eq!(tex as usize, r.texture_names().iter().position(|n| n == "grass_block_top").unwrap());
    }

    #[test]
    fn faces_against_neighbor_section_are_culled() {
        let (r, t) = setup();
        let s = r.id("stone").unwrap();
        // Block at the +X edge; neighbour section has stone at x=16.
        let n =
            Neighborhood::from_fn(
                |x, y, z| if (y, z) == (5, 5) && (x == 15 || x == 16) { s } else { BlockId::AIR },
            );
        assert_eq!(quads(&mesh_section(&n, &t)), 5, "east face hidden by neighbour");
    }

    #[test]
    fn leaves_show_inner_faces_glass_does_not() {
        let (r, t) = setup();
        // Two blocks side by side. Coplanar outer faces merge (6 quads).
        // Leaves also keep the 2 faces between them (trees look full); glass hides them.
        for (name, expected) in [("oak_leaves", 8), ("glass", 6)] {
            let b = r.id(name).unwrap();
            let n =
                Neighborhood::from_fn(
                    |x, y, z| if y == 5 && z == 5 && (x == 5 || x == 6) { b } else { BlockId::AIR },
                );
            let m = mesh_section(&n, &t);
            assert_eq!(m.opaque.len(), 0);
            assert_eq!(m.cutout.len() / 4, expected, "{name}");
        }
    }

    #[test]
    fn ambient_occlusion_darkens_inner_corner() {
        let (r, t) = setup();
        let s = r.id("stone").unwrap();
        // Floor at y=0 plus a wall block at (5,1,5): the floor's top face next to the
        // wall must get darker corners.
        let n =
            Neighborhood::from_fn(|x, y, z| if y == 0 || (x, y, z) == (5, 1, 5) { s } else { BlockId::AIR });
        let m = mesh_section(&n, &t);
        let darkest = m.opaque.iter().filter(|v| v.unpack().1 == 2).map(|v| v.unpack().2).min().unwrap();
        assert!(darkest < 3, "floor next to wall has AO");
    }

    #[test]
    fn world_bottom_is_not_meshed() {
        let (r, t) = setup();
        let s = r.id("stone").unwrap();
        let col = {
            let mut c = ld_world::ChunkColumn::new(ld_core::coords::ChunkPos::new(0, 0));
            for x in 0..16 {
                for z in 0..16 {
                    c.set(x, -64, z, s);
                }
            }
            c
        };
        let cols = [[Some(&col); 3]; 3];
        let m = mesh_section(&Neighborhood::from_columns(cols, 0), &t);
        assert_eq!(quads(&m), 1, "only the top of the bedrock layer");
    }

    #[test]
    fn plants_are_two_double_sided_panes_and_dont_hide_neighbours() {
        let (r, t) = setup();
        let (g, s) = (r.id("short_grass").unwrap(), r.id("stone").unwrap());
        let n = Neighborhood::from_fn(|x, y, z| match (x, y, z) {
            (5, 5, 5) => g,
            (6, 5, 5) => s,
            _ => BlockId::AIR,
        });
        let m = mesh_section(&n, &t);
        assert_eq!(m.cutout.len() / 4, 4, "2 panes × 2 sides");
        assert_eq!(m.opaque.len() / 4, 6, "stone keeps its west face next to the plant");
        let faces: std::collections::HashSet<u32> = m.cutout.iter().map(|v| v.unpack().1).collect();
        assert_eq!(faces, [6, 7].into());
        assert!(m.cutout.iter().all(|v| v.c != crate::WHITE), "grass plant is biome-tinted");
    }

    #[test]
    fn water_goes_to_translucent_and_culls_itself() {
        let (r, t) = setup();
        let w = r.id("water").unwrap();
        let n = Neighborhood::from_fn(|_, y, _| if y < 8 { w } else { BlockId::AIR });
        let m = mesh_section(&n, &t);
        assert!(m.opaque.is_empty() && m.cutout.is_empty());
        assert_eq!(m.translucent.len() / 4, 1, "only the merged surface");
    }

    #[test]
    fn worst_case_checkerboard_fits_vertex_format() {
        let (r, t) = setup();
        let s = r.id("stone").unwrap();
        let n =
            Neighborhood::from_fn(|x, y, z| if (x + y + z).rem_euclid(2) == 0 { s } else { BlockId::AIR });
        let m = mesh_section(&n, &t);
        // 2048 blocks inside × 6 faces, nothing merges.
        assert_eq!(quads(&m), 2048 * 6);
    }
}
