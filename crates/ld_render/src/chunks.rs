//! GPU side of the voxel world: section meshes, block textures, culling, drawing.

use ahash::AHashMap;
use glam::{DVec3, Mat4, Vec3, Vec4};
use ld_core::coords::SectionPos;
use ld_mesh::{PackedVertex, SectionMesh};
use wgpu::util::DeviceExt;

/// One section's mesh on the GPU: opaque vertices first, then cutout vertices.
struct GpuSection {
    buffer: wgpu::Buffer,
    opaque_quads: u32,
    cutout_quads: u32,
}

/// Counters for the debug overlay.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChunkStats {
    /// Sections with a mesh on the GPU.
    pub sections_loaded: u32,
    /// Sections drawn this frame.
    pub sections_drawn: u32,
    /// Sections skipped because they were outside the view.
    pub sections_culled: u32,
    pub quads_drawn: u64,
    /// Draw calls issued for chunks this frame.
    pub draw_calls: u32,
    /// Approximate GPU memory used by section meshes.
    pub mesh_bytes: u64,
}

pub(crate) struct ChunkRenderer {
    opaque_pipeline: wgpu::RenderPipeline,
    cutout_pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    texture_bind_group: wgpu::BindGroup,
    sections: AHashMap<SectionPos, GpuSection>,
    /// Shared index buffer: quad `i` uses vertices 4i..4i+3 as (0,1,2, 0,2,3).
    quad_indices: wgpu::Buffer,
    quad_capacity: u32,
    /// Per-frame section origins (camera-relative), one 16-byte entry per draw.
    instances: wgpu::Buffer,
    instance_capacity: u64,
    mesh_bytes: u64,
    pub stats: ChunkStats,
}

const INSTANCE_STRIDE: u64 = 16;

impl ChunkRenderer {
    pub fn new(
        device: &wgpu::Device,
        camera_layout: &wgpu::BindGroupLayout,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("block textures layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/chunk.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("chunk pipeline layout"),
            bind_group_layouts: &[Some(camera_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<PackedVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Uint32x2],
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: INSTANCE_STRIDE,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![1 => Float32x4],
            }),
        ];
        let pipeline = |label, entry| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth_format,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Greater), // reverse-Z
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let opaque_pipeline = pipeline("chunk opaque", "fs_opaque");
        let cutout_pipeline = pipeline("chunk cutout", "fs_cutout");

        // Until real textures arrive, bind a tiny placeholder (2 layers: the
        // OpenGL backend treats 1-layer arrays as plain 2D textures).
        let placeholder = vec![vec![255u8; 4]; 2];
        let texture_bind_group =
            Self::make_texture_bind_group(device, None, &texture_layout, 1, &placeholder, 1);
        let quad_capacity = 16_384;
        let quad_indices = Self::make_quad_indices(device, quad_capacity);
        let instance_capacity = 4096;
        let instances = Self::make_instances(device, instance_capacity);
        Self {
            opaque_pipeline,
            cutout_pipeline,
            texture_layout,
            texture_bind_group,
            sections: AHashMap::new(),
            quad_indices,
            quad_capacity,
            instances,
            instance_capacity,
            mesh_bytes: 0,
            stats: ChunkStats::default(),
        }
    }

    fn make_quad_indices(device: &wgpu::Device, quads: u32) -> wgpu::Buffer {
        let idx: Vec<u32> = (0..quads)
            .flat_map(|q| {
                let b = q * 4;
                [b, b + 1, b + 2, b, b + 2, b + 3]
            })
            .collect();
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad indices"),
            contents: bytemuck::cast_slice(&idx),
            usage: wgpu::BufferUsages::INDEX,
        })
    }

    fn make_instances(device: &wgpu::Device, count: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("section origins"),
            size: count * INSTANCE_STRIDE,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Builds the texture array + mipmaps and its bind group.
    fn make_texture_bind_group(
        device: &wgpu::Device,
        queue: Option<&wgpu::Queue>,
        layout: &wgpu::BindGroupLayout,
        size: u32,
        layers: &[Vec<u8>],
        mip_levels: u32,
    ) -> wgpu::BindGroup {
        let mips = generate_mips(size, layers, mip_levels);
        let layer_count = layers.len().max(2) as u32;
        let desc = wgpu::TextureDescriptor {
            label: Some("block textures"),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: layer_count },
            mip_level_count: mips.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };
        // Data layout expected by create_texture_with_data: layer-major, then mip.
        let mut data = Vec::new();
        for layer in 0..layer_count as usize {
            for level in &mips {
                data.extend_from_slice(&level[layer.min(layers.len() - 1)]);
            }
        }
        let texture = match queue {
            Some(q) => {
                device.create_texture_with_data(q, &desc, wgpu::util::TextureDataOrder::LayerMajor, &data)
            }
            None => device.create_texture(&desc), // placeholder content doesn't matter
        };
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("block sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            // Nearest keeps crisp pixel art up close; blending between mip levels
            // removes shimmering on distant blocks.
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("block textures"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        })
    }

    pub fn set_textures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: u32,
        layers: &[Vec<u8>],
        mip_levels: u32,
    ) {
        self.texture_bind_group = Self::make_texture_bind_group(
            device,
            Some(queue),
            &self.texture_layout,
            size,
            layers,
            mip_levels,
        );
    }

    pub fn upload(&mut self, device: &wgpu::Device, pos: SectionPos, mesh: &SectionMesh) {
        self.remove(pos);
        let quads = (mesh.opaque.len() + mesh.cutout.len()) / 4;
        if quads == 0 {
            return;
        }
        if quads as u32 > self.quad_capacity {
            self.quad_capacity = (quads as u32).next_power_of_two();
            self.quad_indices = Self::make_quad_indices(device, self.quad_capacity);
        }
        let mut verts = Vec::with_capacity(quads * 4);
        verts.extend_from_slice(&mesh.opaque);
        verts.extend_from_slice(&mesh.cutout);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("section mesh"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.mesh_bytes += buffer.size();
        self.sections.insert(
            pos,
            GpuSection {
                buffer,
                opaque_quads: (mesh.opaque.len() / 4) as u32,
                cutout_quads: (mesh.cutout.len() / 4) as u32,
            },
        );
    }

    pub fn remove(&mut self, pos: SectionPos) {
        if let Some(s) = self.sections.remove(&pos) {
            self.mesh_bytes -= s.buffer.size();
        }
    }

    /// Frustum-culls, sorts and writes per-section origins for this frame.
    /// Returns the visible sections in front-to-back order.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_proj: Mat4,
        camera: DVec3,
    ) -> Vec<SectionPos> {
        let frustum = Frustum::from_matrix(view_proj);
        let mut visible: Vec<(f64, SectionPos, Vec3)> = Vec::new();
        let mut culled = 0;
        for &pos in self.sections.keys() {
            let origin = DVec3::new(pos.x as f64, pos.y as f64, pos.z as f64) * 16.0;
            let rel = (origin - camera).as_vec3();
            if frustum.intersects_box(rel, rel + Vec3::splat(16.0)) {
                let dist = (origin + DVec3::splat(8.0) - camera).length_squared();
                visible.push((dist, pos, rel));
            } else {
                culled += 1;
            }
        }
        // Front-to-back: nearby terrain fills the depth buffer first, so the
        // GPU can skip shading hidden pixels further away.
        visible.sort_by(|a, b| a.0.total_cmp(&b.0));

        if visible.len() as u64 > self.instance_capacity {
            self.instance_capacity = (visible.len() as u64).next_power_of_two();
            self.instances = Self::make_instances(device, self.instance_capacity);
        }
        let data: Vec<[f32; 4]> = visible.iter().map(|v| [v.2.x, v.2.y, v.2.z, 0.0]).collect();
        if !data.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&data));
        }
        self.stats = ChunkStats {
            sections_loaded: self.sections.len() as u32,
            sections_drawn: visible.len() as u32,
            sections_culled: culled,
            quads_drawn: 0,
            draw_calls: 0,
            mesh_bytes: self.mesh_bytes,
        };
        visible.into_iter().map(|v| v.1).collect()
    }

    /// Records draw calls for the visible sections (from [`prepare`](Self::prepare)).
    pub fn draw<'a>(
        &'a mut self,
        pass: &mut wgpu::RenderPass<'a>,
        camera_group: &'a wgpu::BindGroup,
        visible: &[SectionPos],
    ) {
        pass.set_bind_group(0, camera_group, &[]);
        pass.set_bind_group(1, &self.texture_bind_group, &[]);
        pass.set_index_buffer(self.quad_indices.slice(..), wgpu::IndexFormat::Uint32);
        let mut quads = 0u64;
        let mut calls = 0u32;
        for (pipeline, cutout) in [(&self.opaque_pipeline, false), (&self.cutout_pipeline, true)] {
            pass.set_pipeline(pipeline);
            for (i, pos) in visible.iter().enumerate() {
                let Some(s) = self.sections.get(pos) else { continue };
                let (count, first) =
                    if cutout { (s.cutout_quads, s.opaque_quads) } else { (s.opaque_quads, 0) };
                if count == 0 {
                    continue;
                }
                let start = first as u64 * 4 * std::mem::size_of::<PackedVertex>() as u64;
                pass.set_vertex_buffer(0, s.buffer.slice(start..));
                let inst = i as u64 * INSTANCE_STRIDE;
                pass.set_vertex_buffer(1, self.instances.slice(inst..inst + INSTANCE_STRIDE));
                pass.draw_indexed(0..count * 6, 0, 0..1);
                quads += count as u64;
                calls += 1;
            }
        }
        self.stats.quads_drawn = quads;
        self.stats.draw_calls = calls;
    }
}

/// Builds mip levels by averaging 2×2 blocks (in linear colour space, weighted
/// by alpha so transparent pixels don't darken edges). Returns
/// `levels[level][layer] = rgba bytes`.
fn generate_mips(size: u32, layers: &[Vec<u8>], max_levels: u32) -> Vec<Vec<Vec<u8>>> {
    let possible = 32 - size.leading_zeros(); // e.g. 16 → 5 levels (16,8,4,2,1)
    let levels = (max_levels + 1).min(possible).max(1);
    let to_lin = |c: u8| (c as f32 / 255.0).powf(2.2);
    let to_srgb = |c: f32| (c.max(0.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8;
    let mut out = vec![layers.to_vec()];
    let mut s = size;
    for _ in 1..levels {
        let ns = (s / 2).max(1);
        let prev = out.last().unwrap();
        let next: Vec<Vec<u8>> = prev
            .iter()
            .map(|src| {
                let mut dst = vec![0u8; (ns * ns * 4) as usize];
                for y in 0..ns {
                    for x in 0..ns {
                        let (mut rgb, mut a) = ([0f32; 3], 0f32);
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let i = (((y * 2 + dy) * s + x * 2 + dx) * 4) as usize;
                            let w = src[i + 3] as f32 / 255.0;
                            for c in 0..3 {
                                rgb[c] += to_lin(src[i + c]) * w;
                            }
                            a += w;
                        }
                        let o = ((y * ns + x) * 4) as usize;
                        for c in 0..3 {
                            dst[o + c] = if a > 0.0 { to_srgb(rgb[c] / a) } else { 0 };
                        }
                        dst[o + 3] = (a / 4.0 * 255.0 + 0.5) as u8;
                    }
                }
                dst
            })
            .collect();
        out.push(next);
        s = ns;
    }
    out
}

/// The view volume as planes, for "is this box on screen?" tests.
struct Frustum {
    planes: [Vec4; 5],
}

impl Frustum {
    /// Extracts left/right/bottom/top/near planes from a view-projection matrix
    /// (Gribb–Hartmann method). The far plane is omitted: our projection is
    /// infinite, and render distance limits what is loaded anyway.
    fn from_matrix(m: Mat4) -> Self {
        let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));
        // Reverse-Z: visible depth is 0 ≤ z ≤ w, near plane is z ≤ w.
        Self { planes: [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r3 - r2] }
    }

    /// Conservative box test: false only if the box is entirely outside a plane.
    fn intersects_box(&self, min: Vec3, max: Vec3) -> bool {
        self.planes.iter().all(|p| {
            // The box corner furthest along the plane normal.
            let v = Vec3::new(
                if p.x >= 0.0 { max.x } else { min.x },
                if p.y >= 0.0 { max.y } else { min.y },
                if p.z >= 0.0 { max.z } else { min.z },
            );
            p.truncate().dot(v) + p.w >= 0.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Camera;

    #[test]
    fn frustum_keeps_boxes_in_front_and_culls_behind() {
        let cam = Camera { position: DVec3::ZERO, yaw_deg: 0.0, pitch_deg: 0.0, fov_deg: 70.0 };
        let f = Frustum::from_matrix(cam.view_proj(16.0 / 9.0));
        // Looking toward +Z.
        assert!(f.intersects_box(Vec3::new(-8.0, -8.0, 20.0), Vec3::new(8.0, 8.0, 36.0)), "ahead");
        assert!(!f.intersects_box(Vec3::new(-8.0, -8.0, -36.0), Vec3::new(8.0, 8.0, -20.0)), "behind");
        assert!(!f.intersects_box(Vec3::new(200.0, -8.0, 20.0), Vec3::new(216.0, 8.0, 36.0)), "far right");
        assert!(f.intersects_box(Vec3::new(-8.0, -8.0, -8.0), Vec3::new(8.0, 8.0, 8.0)), "around camera");
    }

    #[test]
    fn mips_halve_and_preserve_colour() {
        let layer = [200u8, 100, 50, 255].repeat(16 * 16);
        let mips = generate_mips(16, &[layer], 4);
        assert_eq!(mips.len(), 5);
        assert_eq!(mips[4][0].len(), 4); // 1×1
        let px = &mips[2][0][..4];
        assert!((px[0] as i32 - 200).abs() <= 1 && (px[1] as i32 - 100).abs() <= 1 && px[3] == 255);
        assert_eq!(generate_mips(16, &[vec![0; 1024]], 0).len(), 1, "mips disabled");
    }
}
