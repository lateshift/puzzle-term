//! Opt-in evidence from the actual acquired drawable, not an oracle texture.
//! D-27 stores already encoded sRGB bytes. Reading the real swapchain is necessary:
//! an offscreen Unorm golden cannot detect a second hardware sRGB conversion.
//! Copy access is enabled before configuration and survives device recreation.
//! Ordinary presentation does not enable COPY_SRC or perform any GPU readback.
use crate::{RenderError, Renderer, RgbaImage};

/// Readback borrows the same device/queue that submitted the native frame.
impl Renderer {
    /// Enables diagnostic drawable copies before configuring any surface.
    /// This is an evidence facility, not a presentation policy or host requirement.
    pub fn enable_surface_readback(&mut self) {
        self.surface_readback = true;
    }

    /// Whether this renderer explicitly opted into swapchain evidence.
    pub fn surface_readback_enabled(&self) -> bool {
        self.surface_readback
    }

    /// Mapping waits for this diagnostic's submitted copy, so callers must opt in
    /// before configuration and keep it outside normal latency measurements.
    /// The returned image owns tightly packed RGBA bytes with no colour transform.
    /// Texture channel order is normalized only after the GPU bytes are mapped.
    /// Lost devices and unsupported attachments return RenderError::Readback.
    ///
    /// Copies bytes from the acquired texture that the caller will actually present.
    /// Rejects unsupported formats/usages rather than substituting an offscreen render.
    pub fn read_surface_texture(&self, texture: &wgpu::Texture) -> Result<RgbaImage, RenderError> {
        // Only eight-bit colour attachments have the byte contract tested here.
        let format = texture.format();
        if !texture.usage().contains(wgpu::TextureUsages::COPY_SRC)
            || !matches!(
                format.remove_srgb_suffix(),
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
            )
        {
            return Err(RenderError::Readback(
                "drawable lacks readable eight-bit colour".into(),
            ));
        }
        // Acquired dimensions are authoritative even if a resize message raced the
        // snapshot. Reading the prior frame's dimensions would not witness this image.
        let width = texture.width();
        let height = texture.height();
        // Texture extents already passed the device's limits at acquisition.
        let row_bytes = width * 4;
        let padded = row_bytes.div_ceil(256) * 256;
        // Textures cannot be mapped directly. Align staging rows to WebGPU's copy
        // requirement while leaving the returned image tightly packed.
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("presented drawable evidence"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        // Queue ordering preserves every submitted draw before this copy.
        self.copy_to_buffer(texture, &buffer, padded, width, height);
        // This waits for the copy on the rendering queue, then removes row padding.
        let mut pixels = self.map_pixels(&buffer, row_bytes, padded, height)?;
        if format.remove_srgb_suffix() == wgpu::TextureFormat::Bgra8Unorm {
            // CPU evidence is RGBA even when the native attachment is BGRA.
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        Ok(RgbaImage {
            width,
            height,
            pixels,
        })
    }
}
