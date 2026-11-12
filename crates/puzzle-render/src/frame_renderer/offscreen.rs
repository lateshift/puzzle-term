//! Offscreen evidence from the production frame executor (§10.6).
//!
//! A harness frame uses the same upload, scissor and draw path as a window
//! frame. Only its target and readback differ. This makes a PNG useful
//! evidence for the batch contract without a separate test-only shader.
//!
//! Harness executors use Rgba8Unorm for exact oracle/golden bytes. A native
//! retained executor keeps its real attachment format for diagnostic readback;
//! BGRA bytes are reordered for the PNG writer without rebuilding its atlas.
//! The platform creates the renderer, and this helper only borrows it;
//! no extra GPU instance or adapter is selected for a screenshot.
//! Width and height are checked before multiplication so malformed frame
//! sizes cannot wrap the readback allocation or row-pitch calculation.
use super::*;
use crate::RgbaImage;

impl FrameRenderer {
    /// Reads a frame through this executor's retained atlas and attachment format.
    pub fn render_to_image(
        &mut self,
        renderer: &Renderer,
        frame: &mut RenderFrame,
    ) -> Result<(RgbaImage, u32), RenderError> {
        let [width, height] = frame.target_size;
        // These bounds mirror the executor's normal target gate; the
        // offscreen allocation happens before render_view checks the frame.
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err(RenderError::Readback("invalid frame dimensions".into()));
        }
        // COPY_SRC allows the same attachment to be copied after the draw;
        // mapping a texture directly is unavailable in WebGPU.
        let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terminal frame evidence"),
            // A single layer suffices because the scenario observes one
            // complete painter-ordered frame, not intermediate z-slots.
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            // Multisampling would blur the very coverage bytes that the
            // Slug oracle and reviewed PNG references are meant to pin.
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        // The executor handles all uploads before starting its one pass.
        // This helper does not build another pass around the frame.
        let draws = self.render_view(renderer, frame, &view)?;
        // WebGPU requires 256-byte row pitch for texture-to-buffer copies.
        // map_pixels strips that padding before the PNG writer receives data.
        let row_bytes = width * 4;
        let padded = row_bytes.div_ceil(256) * 256;
        // COPY_DST receives GPU bytes; MAP_READ is requested only for the
        // harness artifact, never for the interactive presentation path.
        let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terminal frame readback"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        renderer.copy_to_buffer(&target, &buffer, padded, width, height);
        // map_pixels waits for the copy and strips row padding. Its output
        // is tightly packed RGBA8, matching the stored-deflate PNG writer.
        let mut pixels = renderer.map_pixels(&buffer, row_bytes, padded, height)?;
        // Native surface pipelines often use BGRA. Reorder channels only after
        // readback; the draw must preserve the actual executor's GPU resources.
        if matches!(
            self.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        Ok((
            RgbaImage {
                width,
                height,
                pixels,
            },
            draws,
        ))
    }
}
