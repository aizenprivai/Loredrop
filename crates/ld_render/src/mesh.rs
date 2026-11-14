//! Simple GPU meshes (Phase 2 test-scene format).
//!
//! Phase 3 replaces this 16-byte vertex with an 8-byte packed chunk vertex.

/// A coloured vertex. `color.a` is a brightness multiplier (face shading).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub color: [u8; 4],
}

impl Vertex {
    pub(crate) const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4],
    };
}

/// Handle to an uploaded mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshId(u32);

pub(crate) struct GpuMesh {
    pub origin: glam::DVec3,
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    pub uniform: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

/// Slot storage; removed slots are reused.
#[derive(Default)]
pub(crate) struct MeshStore {
    slots: Vec<Option<GpuMesh>>,
}

impl MeshStore {
    pub fn insert(&mut self, m: GpuMesh) -> MeshId {
        if let Some(i) = self.slots.iter().position(Option::is_none) {
            self.slots[i] = Some(m);
            MeshId(i as u32)
        } else {
            self.slots.push(Some(m));
            MeshId(self.slots.len() as u32 - 1)
        }
    }

    pub fn remove(&mut self, id: MeshId) {
        if let Some(s) = self.slots.get_mut(id.0 as usize) {
            *s = None;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &GpuMesh> {
        self.slots.iter().flatten()
    }
}
