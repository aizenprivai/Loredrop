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
//! 5. **Packing**: every vertex is squeezed into 8 bytes ([`PackedVertex`]).
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

/// One vertex in 8 bytes.
///
/// `a` bits: `x:5 | y:5 | z:5 | face:3 | ao:2`  (positions are 0..=16 inside the section)
/// `b` bits: `texture layer:16 | tint:2 | (light:8, reserved for Phase 8)`
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PackedVertex {
    pub a: u32,
    pub b: u32,
}

impl PackedVertex {
    #[inline]
    pub fn new(pos: [u32; 3], face: u32, ao: u32, texture: u16, tint: u32) -> Self {
        debug_assert!(pos.iter().all(|&c| c <= 16) && face < 6 && ao < 4 && tint < 4);
        Self {
            a: pos[0] | (pos[1] << 5) | (pos[2] << 10) | (face << 15) | (ao << 18),
            b: texture as u32 | (tint << 16),
        }
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
        let v = PackedVertex::new([16, 0, 9], 5, 2, 1234, 1);
        assert_eq!(v.unpack(), ([16, 0, 9], 5, 2, 1234, 1));
        assert_eq!(std::mem::size_of::<PackedVertex>(), 8);
    }
}
