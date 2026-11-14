//! # ld_mesh — chunk meshing
//!
//! Turns one 16³ section into the triangles the GPU draws. Runs on worker
//! threads and never touches the GPU, so it is easy to test and benchmark.
//!
//! ## Steps
//! 1. **Neighbourhood** ([`Neighborhood`]): copy the section plus a one-block
//!    border from the surrounding sections into a flat 18³ array. Faces on the
//!    section's edge need to know what is next to them, and ambient occlusion
//!    looks at diagonal neighbours too.
//! 2. **Face culling**: a face is only kept if the block next to it doesn't
//!    hide it. In solid terrain this removes ~95 % of faces.
//! 3. **Greedy meshing**: neighbouring visible faces with the same texture,
//!    tint and shading are merged into one larger rectangle. A flat 16×16
//!    grass field becomes 1 quad instead of 256.
//! 4. **Ambient occlusion (AO)**: each corner of a face is darkened according to
//!    how many solid blocks touch it, giving the soft contact shadows that make
//!    voxel scenes readable.
//! 5. **Smooth lighting**: each corner gets the average sky and block light
//!    of the (up to 4) open cells in front of it, from a light flood fill
//!    computed for the section on demand ([`ld_world::LightRegion`]).
//!    Sections with no visible faces never compute light.
//! 6. **Biome colours**: grass, leaves and water are tinted per column with
//!    the colour of the surrounding biomes (averaged over 3×3 columns, so
//!    biome borders blend smoothly instead of switching abruptly).
//! 7. **Packing**: every vertex is squeezed into 12 bytes ([`PackedVertex`]).
//!
//! Plants (`Shape::Cross`) are not greedy-meshed: each becomes two crossed
//! diagonal panes, drawn from both sides.
//!
//! Texture coordinates are not stored at all: the shader derives them from the
//! vertex position, which is what lets one merged quad repeat its texture
//! across its whole surface.

mod mesher;
mod neighborhood;
mod table;

pub use mesher::{SectionMesh, mesh_section};
pub use neighborhood::Neighborhood;
pub use table::{BlockMeshInfo, MeshTable};

/// One vertex in 12 bytes.
///
/// `a` bits: `x:5 | y:5 | z:5 | face:3 | ao:2`  (positions are 0..=16 inside the section;
///           faces 0–5 are cube faces, 6–7 the two diagonal panes of plants)
/// `b` bits: `texture layer:16 | tint:2 | block light:4 | sky light:4`
/// `c` bits: `red:8 | green:8 | blue:8` biome colour multiplier (white = untinted)
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PackedVertex {
    pub a: u32,
    pub b: u32,
    pub c: u32,
}

/// Untinted (white) colour.
pub const WHITE: u32 = 0x00FF_FFFF;

impl PackedVertex {
    #[inline]
    /// `light` is packed `sky << 4 | block` (see [`ld_world::light::pack`]).
    pub fn new(pos: [u32; 3], face: u32, ao: u32, texture: u16, tint: u32, color: u32, light: u8) -> Self {
        debug_assert!(pos.iter().all(|&c| c <= 16) && face < 8 && ao < 4 && tint < 4);
        let (sky, block) = (light as u32 >> 4, light as u32 & 15);
        Self {
            a: pos[0] | (pos[1] << 5) | (pos[2] << 10) | (face << 15) | (ao << 18),
            b: texture as u32 | (tint << 16) | (block << 18) | (sky << 22),
            c: color,
        }
    }

    /// Packed light (`sky << 4 | block`) — used by tests.
    pub fn light(self) -> u8 {
        (((self.b >> 22) & 15) << 4 | ((self.b >> 18) & 15)) as u8
    }

    /// Unpacks (position, face, ao, texture, tint) — used by tests.
    pub fn unpack(self) -> ([u32; 3], u32, u32, u16, u32) {
        let a = self.a;
        (
            [a & 31, (a >> 5) & 31, (a >> 10) & 31],
            (a >> 15) & 7,
            (a >> 18) & 3,
            (self.b & 0xFFFF) as u16,
            (self.b >> 16) & 3,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_round_trip() {
        let v = PackedVertex::new([16, 0, 9], 7, 2, 1234, 1, 0x00AA_BBCC, 0x9C);
        assert_eq!(v.unpack(), ([16, 0, 9], 7, 2, 1234, 1));
        assert_eq!(v.light(), 0x9C);
        assert_eq!(v.c, 0x00AA_BBCC);
        assert_eq!(std::mem::size_of::<PackedVertex>(), 12);
    }
}
