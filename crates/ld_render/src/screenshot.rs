//! Copying a rendered frame back to the CPU (F2 screenshots, automated tests).

/// An RGBA8 image (sRGB), row-major, top row first.
pub struct Screenshot {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub(crate) struct PendingCopy {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row: u32,
    bgra: bool,
}

/// Records a copy of `texture` (which must allow `COPY_SRC`) into a readable
/// buffer. Returns `None` for pixel formats we can't convert.
pub(crate) fn copy_to_buffer(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    format: wgpu::TextureFormat,
) -> Option<PendingCopy> {
    use wgpu::TextureFormat as F;
    let bgra = match format {
        F::Bgra8Unorm | F::Bgra8UnormSrgb => true,
        F::Rgba8Unorm | F::Rgba8UnormSrgb => false,
        other => {
            log::warn!("screenshot: unsupported surface format {other:?}");
            return None;
        }
    };
    let (width, height) = (texture.width(), texture.height());
    // GPU copies require each row to be a multiple of 256 bytes.
    let padded_row =
        (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot"),
        size: (padded_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    Some(PendingCopy { buffer, width, height, padded_row, bgra })
}

impl PendingCopy {
    /// Waits for the GPU and returns tightly packed RGBA pixels.
    pub(crate) fn read(self, device: &wgpu::Device) -> Option<Screenshot> {
        let slice = self.buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        if let Err(e) = device.poll(wgpu::PollType::wait_indefinitely()) {
            log::warn!("screenshot: GPU wait failed: {e}");
            return None;
        }
        let data = match slice.get_mapped_range() {
            Ok(d) => d,
            Err(e) => {
                log::warn!("screenshot: could not map buffer: {e}");
                return None;
            }
        };
        let mut rgba = Vec::with_capacity((self.width * self.height * 4) as usize);
        for row in data.chunks(self.padded_row as usize) {
            for px in row[..(self.width * 4) as usize].as_chunks::<4>().0 {
                if self.bgra {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                } else {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
        }
        Some(Screenshot { width: self.width, height: self.height, rgba })
    }
}
