//! GPU device and window-surface setup, including backend fallback.

use crate::backend_order;
use ld_config::Backend;
use std::fmt;

/// Why rendering could not start or continue.
#[derive(Debug)]
pub enum RenderError {
    /// No backend in the fallback list produced a usable GPU.
    NoAdapter(String),
    /// The GPU device was lost (driver crash/reset).
    DeviceLost,
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::NoAdapter(s) => write!(f, "no usable graphics adapter: {s}"),
            RenderError::DeviceLost => write!(f, "the graphics device was lost"),
        }
    }
}
impl std::error::Error for RenderError {}

/// Human-readable GPU details for the debug overlay and logs.
#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub backend: String,
    pub adapter: String,
    pub driver: String,
    pub device_type: String,
}

pub(crate) struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    pub info: GpuInfo,
    pub can_screenshot: bool,
    /// Non-sRGB twin of the surface format, used for the UI pass.
    pub ui_format: wgpu::TextureFormat,
    /// True if the frame is not sRGB, so shaders must gamma-encode output.
    pub manual_gamma: bool,
    adapter: wgpu::Adapter,
}

impl Gpu {
    pub fn new<W>(window: W, size: (u32, u32), pref: Backend, vsync: bool) -> Result<Self, crate::RenderError>
    where
        W: wgpu::WindowHandle + wgpu::rwh::HasDisplayHandle + fmt::Debug + Clone + 'static,
    {
        let mut errors = Vec::new();
        for backends in backend_order(pref) {
            // Each attempt gets its own instance limited to these backends.
            let mut desc =
                wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone()));
            desc.backends = backends;
            let instance = wgpu::Instance::new(desc);
            let surface = match instance.create_surface(window.clone()) {
                Ok(s) => s,
                Err(e) => {
                    errors.push(format!("{backends:?}: surface: {e}"));
                    continue;
                }
            };
            let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                apply_limit_buckets: false,
            })) {
                Ok(a) => a,
                Err(e) => {
                    errors.push(format!("{backends:?}: {e}"));
                    continue;
                }
            };
            // Ask only for "downlevel" limits so 2014-era GPUs and OpenGL qualify,
            // but allow textures as large as the GPU supports.
            let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
            let (device, queue) = match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("loredrop device"),
                required_limits: limits,
                ..Default::default()
            })) {
                Ok(dq) => dq,
                Err(e) => {
                    errors.push(format!("{backends:?}: device: {e}"));
                    continue;
                }
            };
            let Some(mut config) = surface.get_default_config(&adapter, size.0.max(1), size.1.max(1)) else {
                errors.push(format!("{backends:?}: surface not supported by adapter"));
                continue;
            };
            let caps = surface.get_capabilities(&adapter);
            // Prefer an sRGB format so colours are gamma-correct.
            if let Some(f) = caps.formats.iter().find(|f| f.is_srgb()) {
                config.format = *f;
            }
            let can_screenshot = caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
            if can_screenshot {
                config.usage |= wgpu::TextureUsages::COPY_SRC;
            }
            // The UI (egui) wants a non-sRGB target. Ideally we create a second
            // non-sRGB "view" of the sRGB frame. Some old/OpenGL drivers can't, so
            // there we use a non-sRGB frame and the world shader applies gamma itself.
            let view_formats_ok = adapter
                .get_downlevel_capabilities()
                .flags
                .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS);
            let ui_format = config.format.remove_srgb_suffix();
            let manual_gamma = if ui_format == config.format {
                true // the surface offers no sRGB format at all
            } else if view_formats_ok {
                config.view_formats.push(ui_format);
                false
            } else {
                config.format = ui_format;
                true
            };
            if manual_gamma {
                log::info!("graphics: using shader gamma correction (no sRGB view support)");
            }
            config.present_mode = present_mode(vsync);
            surface.configure(&device, &config);

            let ai = adapter.get_info();
            let info = GpuInfo {
                backend: format!("{:?}", ai.backend),
                adapter: ai.name.clone(),
                driver: format!("{} {}", ai.driver, ai.driver_info).trim().to_string(),
                device_type: format!("{:?}", ai.device_type),
            };
            log::info!(
                "graphics: {} on {} ({}, driver {})",
                info.backend,
                info.adapter,
                info.device_type,
                info.driver
            );
            return Ok(Self {
                device,
                queue,
                surface,
                config,
                info,
                can_screenshot,
                ui_format,
                manual_gamma,
                adapter,
            });
        }
        Err(crate::RenderError::NoAdapter(errors.join("; ")))
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn set_vsync(&mut self, vsync: bool) {
        let mode = present_mode(vsync);
        if self.config.present_mode != mode {
            self.config.present_mode = mode;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Gets the next frame's texture, reconfiguring the surface if needed.
    /// `Ok(None)` means "skip this frame".
    pub fn acquire(&mut self) -> Result<Option<wgpu::SurfaceTexture>, crate::RenderError> {
        use wgpu::CurrentSurfaceTexture as C;
        match self.surface.get_current_texture() {
            C::Success(t) => Ok(Some(t)),
            C::Suboptimal(t) => {
                // Usable this frame; reconfigure for the next one.
                self.surface.configure(&self.device, &self.config);
                Ok(Some(t))
            }
            C::Outdated | C::Lost => {
                self.surface.configure(&self.device, &self.config);
                Ok(None)
            }
            C::Timeout | C::Occluded => Ok(None),
            C::Validation => Err(crate::RenderError::DeviceLost),
        }
    }

    #[allow(dead_code)]
    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }
}

/// VSync on → wait for the display (Fifo, always supported).
/// VSync off → lowest-latency mode available, falling back to Fifo.
fn present_mode(vsync: bool) -> wgpu::PresentMode {
    if vsync { wgpu::PresentMode::AutoVsync } else { wgpu::PresentMode::AutoNoVsync }
}
