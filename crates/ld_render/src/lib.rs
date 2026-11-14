//! # ld_render — renderer
//!
//! Draws the world with [`wgpu`]. Per the director's decision **Vulkan is the
//! primary backend**; DX12/Metal and OpenGL are tried only if Vulkan is not
//! available (see [`backend_order`]), so the game still starts on old GPUs
//! without Vulkan drivers and on macOS.
//!
//! This crate knows nothing about windows or winit: it accepts anything that
//! provides a window handle, which keeps the future mobile port simple.
//!
//! Contents: device setup with backend fallback, surface management, reverse-Z
//! depth buffer, the chunk renderer (packed vertices, texture array with
//! mipmaps, frustum culling, opaque + cutout passes), a simple coloured-mesh
//! pipeline for debug geometry, the egui overlay pass, and screenshots.

mod camera;
mod chunks;
mod gpu;
mod mesh;
mod screenshot;

pub use camera::Camera;
pub use chunks::{ChunkStats, EntityVertex};
pub use gpu::{GpuInfo, RenderError};
pub use mesh::{MeshId, Vertex};
pub use screenshot::Screenshot;

use glam::DVec3;
use ld_config::Backend;
use wgpu::util::DeviceExt;

/// The order in which graphics backends are tried for a given preference.
///
/// * `Auto`: Vulkan → native API (DX12 on Windows, Metal on macOS) → OpenGL.
/// * Forced choice: that backend only; if it fails, [`Renderer::new`] falls
///   back to `Auto` with a warning rather than refusing to start.
pub fn backend_order(pref: Backend) -> Vec<wgpu::Backends> {
    match pref {
        Backend::Auto => {
            vec![wgpu::Backends::VULKAN, wgpu::Backends::DX12 | wgpu::Backends::METAL, wgpu::Backends::GL]
        }
        Backend::Vulkan => vec![wgpu::Backends::VULKAN],
        Backend::Dx12 => vec![wgpu::Backends::DX12],
        Backend::Metal => vec![wgpu::Backends::METAL],
        Backend::OpenGl => vec![wgpu::Backends::GL],
    }
}

/// Per-frame scene parameters supplied by the game.
pub struct FrameParams {
    pub camera: Camera,
    /// Fog colour (and the clear colour when no sky is drawn), sRGB 0..1.
    /// Usually the horizon colour, darkened underground.
    pub sky_color: [f32; 3],
    /// `Some((start, end))` fog distances in blocks, or `None` for no fog.
    pub fog: Option<(f32, f32)>,
    /// sRGB tint colours for grass and foliage faces.
    pub grass_tint: [f32; 3],
    pub foliage_tint: [f32; 3],
    /// The sky to draw behind the world, or `None` to show just `sky_color`
    /// (e.g. underwater).
    pub sky: Option<SkyParams>,
    /// Brightness of sunlit surfaces (1 = day, ≈0.2 = moonlit night).
    pub daylight: f32,
    /// The player's brightness setting, 0 (moody) to 1 (bright).
    pub brightness: f32,
}

/// What the sky looks like this frame (from `ld_game::time::SkyState`).
#[derive(Debug, Clone, Copy)]
pub struct SkyParams {
    /// sRGB colours straight up and at the horizon.
    pub zenith: [f32; 3],
    pub horizon: [f32; 3],
    /// sRGB sunrise/sunset glow colour and its strength 0..1.
    pub glow: [f32; 3],
    pub glow_strength: f32,
    /// Unit vector toward the sun (the moon is drawn opposite).
    pub sun_dir: [f32; 3],
    /// Star visibility 0..1.
    pub stars: f32,
    /// Moon phase 0..7 (0 = full).
    pub moon_phase: u32,
}

/// UI to draw on top of the world (already tessellated by egui).
pub struct UiFrame {
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub textures_delta: egui::TexturesDelta,
    pub pixels_per_point: f32,
}

/// Counters for the debug overlay.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderStats {
    pub draw_calls: u32,
    pub triangles: u64,
    pub chunks: ChunkStats,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
    tints: [[f32; 4]; 2],
    // --- Phase 8: sky and daylight (see sky.wgsl for the meaning) ---
    inv_view_proj: [[f32; 4]; 4],
    sky_zenith: [f32; 4],
    sky_horizon: [f32; 4],
    glow: [f32; 4],
    sun: [f32; 4],
    misc: [f32; 4],
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Owns the GPU device, the window surface and all rendering resources.
pub struct Renderer {
    gpu: gpu::Gpu,
    depth_view: wgpu::TextureView,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    mesh_layout: wgpu::BindGroupLayout,
    world_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    /// Whether to draw the sky this frame (set in `render`).
    draw_sky: bool,
    meshes: mesh::MeshStore,
    chunks: chunks::ChunkRenderer,
    outline_pipeline: wgpu::RenderPipeline,
    outline_buffer: wgpu::Buffer,
    /// World-space box to outline this frame (min, max), if any.
    outline: Option<(DVec3, DVec3)>,
    egui: egui_wgpu::Renderer,
    stats: RenderStats,
}

impl Renderer {
    /// Creates the renderer for a window.
    ///
    /// `window` is typically an `Arc<winit::window::Window>`; it is cloned
    /// internally so the GPU surface can keep the window alive.
    pub fn new<W>(window: W, size: (u32, u32), backend: Backend, vsync: bool) -> Result<Self, RenderError>
    where
        W: wgpu::WindowHandle + wgpu::rwh::HasDisplayHandle + std::fmt::Debug + Clone + 'static,
    {
        let gpu = match gpu::Gpu::new(window.clone(), size, backend, vsync) {
            Ok(g) => g,
            Err(e) if backend != Backend::Auto => {
                log::warn!("forced backend {:?} failed ({e}); falling back to auto", backend);
                gpu::Gpu::new(window, size, Backend::Auto, vsync)?
            }
            Err(e) => return Err(e),
        };
        let device = &gpu.device;

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera uniform"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_layout = |label| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            })
        };
        let camera_layout = uniform_layout("camera layout");
        let mesh_layout = uniform_layout("mesh layout");
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera bind group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: camera_buffer.as_entire_binding() }],
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/world.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world pipeline layout"),
            bind_group_layouts: &[Some(&camera_layout), Some(&mesh_layout)],
            immediate_size: 0,
        });
        let world_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(Vertex::LAYOUT)],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                // Back faces of blocks are never visible, so skip them (halves pixel work).
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            // Reverse-Z: near = 1.0, far = 0.0. Spreads depth precision evenly,
            // which avoids flickering (z-fighting) at long render distances.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: gpu.config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let chunks = chunks::ChunkRenderer::new(device, &camera_layout, gpu.config.format, DEPTH_FORMAT);

        // Sky: a full-screen triangle drawn first, behind everything.
        let sky_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/sky.wgsl"));
        let sky_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky layout"),
            bind_group_layouts: &[Some(&camera_layout)],
            immediate_size: 0,
        });
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky pipeline"),
            layout: Some(&sky_layout),
            vertex: wgpu::VertexState {
                module: &sky_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            // Never writes depth, so the world drawn afterwards always covers it.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &sky_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: gpu.config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // Selection outline: 12 edges = 24 line vertices, rewritten each frame.
        let outline_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/outline.wgsl"));
        let outline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("outline layout"),
            bind_group_layouts: &[Some(&camera_layout)],
            immediate_size: 0,
        });
        let outline_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("outline pipeline"),
            layout: Some(&outline_layout),
            vertex: wgpu::VertexState {
                module: &outline_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 12,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &outline_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: gpu.config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let outline_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("outline vertices"),
            size: 24 * 12,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let egui = egui_wgpu::Renderer::new(device, gpu.ui_format, egui_wgpu::RendererOptions::default());
        let depth_view = create_depth(device, gpu.config.width, gpu.config.height);

        Ok(Self {
            gpu,
            depth_view,
            camera_buffer,
            camera_bind_group,
            mesh_layout,
            world_pipeline,
            sky_pipeline,
            draw_sky: false,
            meshes: mesh::MeshStore::default(),
            chunks,
            outline_pipeline,
            outline_buffer,
            outline: None,
            egui,
            stats: RenderStats::default(),
        })
    }

    /// Information about the GPU and backend in use (for the debug overlay).
    pub fn info(&self) -> &GpuInfo {
        &self.gpu.info
    }

    /// Statistics of the most recent frame.
    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    /// Largest texture the GPU supports (egui needs to know).
    pub fn max_texture_side(&self) -> usize {
        self.gpu.device.limits().max_texture_dimension_2d as usize
    }

    /// Call when the window is resized. Zero sizes (minimised) are ignored.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.gpu.resize(width, height);
        self.depth_view = create_depth(&self.gpu.device, width, height);
    }

    /// Turns VSync on/off (takes effect on the next frame).
    pub fn set_vsync(&mut self, vsync: bool) {
        self.gpu.set_vsync(vsync);
    }

    /// Replaces the block texture array. `layers` are square RGBA8 images of
    /// `size`×`size`; `mip_levels` extra half-size levels are generated (0 = none).
    pub fn set_block_textures(&mut self, size: u32, layers: &[Vec<u8>], mip_levels: u32) {
        self.chunks.set_textures(&self.gpu.device, &self.gpu.queue, size, layers, mip_levels);
    }

    /// Uploads (or replaces) the mesh of one section. An empty mesh removes it.
    pub fn upload_section(&mut self, pos: ld_core::coords::SectionPos, mesh: &ld_mesh::SectionMesh) {
        self.chunks.upload(&self.gpu.device, pos, mesh);
    }

    /// Removes a section's mesh from the GPU.
    pub fn remove_section(&mut self, pos: ld_core::coords::SectionPos) {
        self.chunks.remove(pos);
    }

    /// Sets this frame's entity triangles (dropped items, breaking cracks…).
    /// Positions must be relative to the camera.
    pub fn set_entities(&mut self, vertices: &[EntityVertex]) {
        self.chunks.set_entities(&self.gpu.device, &self.gpu.queue, vertices);
    }

    /// Sets (or clears) the box outlined this frame — the targeted block.
    pub fn set_outline(&mut self, bounds: Option<(DVec3, DVec3)>) {
        self.outline = bounds;
    }

    /// Uploads a mesh whose vertices are relative to `origin` (world space).
    pub fn add_mesh(&mut self, origin: DVec3, vertices: &[Vertex], indices: &[u32]) -> MeshId {
        let d = &self.gpu.device;
        let vb = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ib = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let ub = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mesh uniform"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bg = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh bind group"),
            layout: &self.mesh_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
        });
        self.meshes.insert(mesh::GpuMesh {
            origin,
            vertex_buffer: vb,
            index_buffer: ib,
            index_count: indices.len() as u32,
            uniform: ub,
            bind_group: bg,
        })
    }

    /// Removes a mesh and frees its GPU memory.
    pub fn remove_mesh(&mut self, id: MeshId) {
        self.meshes.remove(id);
    }

    /// Draws one frame. If `screenshot` is true, also returns the frame's pixels.
    pub fn render(
        &mut self,
        frame: &FrameParams,
        ui: Option<UiFrame>,
        screenshot: bool,
    ) -> Result<Option<Screenshot>, RenderError> {
        let Some(surface_tex) = self.gpu.acquire()? else {
            return Ok(None); // surface temporarily unavailable (e.g. minimised)
        };
        let format = self.gpu.config.format;
        let ui_format = self.gpu.ui_format;
        let (w, h) = (self.gpu.config.width, self.gpu.config.height);
        let targets = FrameTargets::for_texture(&surface_tex.texture, ui_format);

        // --- Uniforms ---
        let queue = &self.gpu.queue;
        let device = &self.gpu.device;
        let view_proj = frame.camera.view_proj(w as f32 / h as f32);
        let sky_lin = frame.sky_color.map(|c| c.powf(2.2));
        let (fog_start, fog_end, fog_on) = match frame.fog {
            Some((s, e)) => (s, e, 1.0),
            None => (0.0, 0.0, 0.0),
        };
        let lin = |c: [f32; 3]| [c[0].powf(2.2), c[1].powf(2.2), c[2].powf(2.2), 1.0];
        let cam = CameraUniform {
            view_proj: view_proj.to_cols_array_2d(),
            fog_color: [sky_lin[0], sky_lin[1], sky_lin[2], 1.0],
            fog: [fog_start, fog_end, fog_on, if self.gpu.manual_gamma { 1.0 } else { 0.0 }],
            tints: [lin(frame.grass_tint), lin(frame.foliage_tint)],
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            sky_zenith: frame.sky.map_or([0.0; 4], |s| lin(s.zenith)),
            sky_horizon: frame.sky.map_or([0.0; 4], |s| lin(s.horizon)),
            glow: frame.sky.map_or([0.0; 4], |s| {
                let g = lin(s.glow);
                [g[0], g[1], g[2], s.glow_strength]
            }),
            sun: frame
                .sky
                .map_or([0.0, 1.0, 0.0, 0.0], |s| [s.sun_dir[0], s.sun_dir[1], s.sun_dir[2], s.stars]),
            misc: [
                frame.sky.map_or(0.0, |s| s.moon_phase as f32),
                frame.daylight,
                if frame.sky.is_some() { 1.0 } else { 0.0 },
                frame.brightness.clamp(0.0, 1.0),
            ],
        };
        self.draw_sky = frame.sky.is_some();
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&cam));
        for m in self.meshes.iter() {
            // f64 subtraction first, then f32: see world.wgsl header.
            let off = (m.origin - frame.camera.position).as_vec3();
            queue.write_buffer(&m.uniform, 0, bytemuck::cast_slice(&[off.x, off.y, off.z, 0.0]));
        }
        // The clear colour is written as-is: linear for sRGB frames, sRGB otherwise.
        let clear = if self.gpu.manual_gamma { frame.sky_color } else { sky_lin };
        let visible = self.chunks.prepare(device, queue, view_proj, frame.camera.position);
        if let Some((min, max)) = self.outline {
            // Grow the box a hair so the lines don't flicker against block faces.
            let (a, b) = (
                (min - DVec3::splat(0.002) - frame.camera.position).as_vec3(),
                (max + DVec3::splat(0.002) - frame.camera.position).as_vec3(),
            );
            let c = |x: bool, y: bool, z: bool| {
                [if x { b.x } else { a.x }, if y { b.y } else { a.y }, if z { b.z } else { a.z }]
            };
            let mut lines = Vec::with_capacity(24);
            for &(p, q) in &[
                ((0, 0, 0), (1, 0, 0)),
                ((0, 1, 0), (1, 1, 0)),
                ((0, 0, 1), (1, 0, 1)),
                ((0, 1, 1), (1, 1, 1)),
                ((0, 0, 0), (0, 1, 0)),
                ((1, 0, 0), (1, 1, 0)),
                ((0, 0, 1), (0, 1, 1)),
                ((1, 0, 1), (1, 1, 1)),
                ((0, 0, 0), (0, 0, 1)),
                ((1, 0, 0), (1, 0, 1)),
                ((0, 1, 0), (0, 1, 1)),
                ((1, 1, 0), (1, 1, 1)),
            ] {
                lines.push(c(p.0 == 1, p.1 == 1, p.2 == 1));
                lines.push(c(q.0 == 1, q.1 == 1, q.2 == 1));
            }
            queue.write_buffer(&self.outline_buffer, 0, bytemuck::cast_slice(&lines));
        }

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });

        // --- UI resources (uploaded once, drawn into every target) ---
        let mut free_after = Vec::new();
        let mut ui_draw = None;
        if let Some(ui) = ui {
            let screen =
                egui_wgpu::ScreenDescriptor { size_in_pixels: [w, h], pixels_per_point: ui.pixels_per_point };
            for (id, deltas) in &ui.textures_delta.set {
                for delta in deltas {
                    self.egui.update_texture(device, queue, *id, delta);
                }
            }
            let cmd = self.egui.update_buffers(device, queue, &mut encoder, &ui.primitives, &screen);
            queue.submit(cmd);
            free_after = ui.textures_delta.free.iter().copied().collect();
            // egui requires deltas to be explicitly marked as handled.
            let mut delta = ui.textures_delta;
            delta.clear();
            ui_draw = Some((ui.primitives, screen));
        }

        let stats = self.encode_frame(&mut encoder, &targets, clear, ui_draw.as_ref(), &visible);

        // --- Optional screenshot ---
        let pending = if !screenshot {
            None
        } else if self.gpu.can_screenshot {
            screenshot::copy_to_buffer(&self.gpu.device, &mut encoder, &surface_tex.texture, format)
        } else {
            // This surface can't be read back (common on OpenGL), so draw the
            // same frame a second time into an offscreen image we *can* read.
            let extra_view = [ui_format];
            let view_formats: &[wgpu::TextureFormat] = if ui_format != format { &extra_view } else { &[] };
            let tex = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("screenshot target"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats,
            });
            let offscreen = FrameTargets::for_texture(&tex, ui_format);
            self.encode_frame(&mut encoder, &offscreen, clear, ui_draw.as_ref(), &visible);
            screenshot::copy_to_buffer(&self.gpu.device, &mut encoder, &tex, format)
        };

        self.gpu.queue.submit(Some(encoder.finish()));
        let shot = pending.and_then(|p| p.read(&self.gpu.device));
        self.gpu.queue.present(surface_tex);

        for id in &free_after {
            self.egui.free_texture(id);
        }
        self.stats = stats;
        Ok(shot)
    }

    /// Records the world pass and UI pass into `targets`.
    fn encode_frame(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &FrameTargets,
        clear: [f32; 3],
        ui: Option<&(Vec<egui::ClippedPrimitive>, egui_wgpu::ScreenDescriptor)>,
        visible: &[ld_core::coords::SectionPos],
    ) -> RenderStats {
        let mut stats = RenderStats::default();
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.world,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear[0] as f64,
                            g: clear[1] as f64,
                            b: clear[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if self.draw_sky {
                pass.set_pipeline(&self.sky_pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.draw(0..3, 0..1);
                stats.draw_calls += 1;
            }
            self.chunks.draw(&mut pass, &self.camera_bind_group, visible);
            if self.outline.is_some() {
                pass.set_pipeline(&self.outline_pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, self.outline_buffer.slice(..));
                pass.draw(0..24, 0..1);
            }
            pass.set_pipeline(&self.world_pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            for m in self.meshes.iter() {
                pass.set_bind_group(1, &m.bind_group, &[]);
                pass.set_vertex_buffer(0, m.vertex_buffer.slice(..));
                pass.set_index_buffer(m.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..m.index_count, 0, 0..1);
                stats.draw_calls += 1;
                stats.triangles += m.index_count as u64 / 3;
            }
        }
        if let Some((primitives, screen)) = ui {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.ui,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut pass = pass.forget_lifetime();
            self.egui.render(&mut pass, primitives, screen);
        }
        stats.chunks = self.chunks.stats;
        stats.draw_calls += stats.chunks.draw_calls;
        stats.triangles += stats.chunks.quads_drawn * 2;
        stats
    }
}

/// The two views of one frame image: the world draws through the normal
/// (usually sRGB) view; egui draws through a non-sRGB view of the same pixels.
struct FrameTargets {
    world: wgpu::TextureView,
    ui: wgpu::TextureView,
}

impl FrameTargets {
    fn for_texture(tex: &wgpu::Texture, ui_format: wgpu::TextureFormat) -> Self {
        Self {
            world: tex.create_view(&Default::default()),
            ui: tex
                .create_view(&wgpu::TextureViewDescriptor { format: Some(ui_format), ..Default::default() }),
        }
    }
}

fn create_depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

#[cfg(test)]
mod shader_tests {
    /// Parses and validates every WGSL shader on the CPU, so shader mistakes
    /// fail `cargo test` instead of crashing the game at startup.
    #[test]
    fn shaders_parse_and_validate() {
        for (name, src) in [
            ("world.wgsl", include_str!("shaders/world.wgsl")),
            ("chunk.wgsl", include_str!("shaders/chunk.wgsl")),
            ("outline.wgsl", include_str!("shaders/outline.wgsl")),
            ("entity.wgsl", include_str!("shaders/entity.wgsl")),
            ("sky.wgsl", include_str!("shaders/sky.wgsl")),
        ] {
            let module = naga::front::wgsl::parse_str(src)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
    }
}
