//! Offscreen clear and tightly packed RGBA readback (design §10.6).
//!
//! GPU rows are padded to 256 bytes during transfer; the returned image removes
//! that padding so the harness can write ordinary PNG scanlines.
//!
//! The clear texture has two usages: render attachment for the clear pass and
//! copy source for transfer into a CPU-visible buffer. Its format is RGBA8
//! unorm so the scenario can compare exact eight-bit channels. No shader or
//! staging image library is needed for this first milestone.
//!
//! WebGPU requires the destination row stride to be a multiple of 256 bytes.
//! Most scenario widths do not satisfy that constraint, so the transfer buffer
//! includes padding at the end of each row. The CPU image removes those bytes
//! before the PNG writer sees them. That keeps image consumers unaware of GPU
//! alignment rules and makes `expect-pixel` use ordinary x/y coordinates.
//!
//! Mapping is asynchronous in wgpu. The native harness waits with a callback
//! and a single device poll; the browser does not call this synchronous path.
use crate::{RenderError, Renderer};
use std::sync::mpsc;
/// A CPU-owned image with four eight-bit channels per pixel.
///
/// Each row has exactly `width * 4` bytes, including alpha. The first pixel
/// is at the upper left, matching PNG's scanline order (D-14).
pub struct RgbaImage {
    /// Pixel width in device pixels.
    pub width: u32,
    /// Pixel height in device pixels.
    pub height: u32,
    /// Tightly packed RGBA pixels, top row first.
    pub pixels: Vec<u8>,
}
/// Adds the native offscreen readback path to the shared renderer.
impl Renderer {
    /// Creates a reusable RGBA attachment for headless frame submissions.
    ///
    /// The flood harness keeps this texture for many frames instead of
    /// mapping every frame back to the CPU, which would add a GPU stall.
    #[must_use]
    pub fn offscreen_texture(&self, width: u32, height: u32) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen frame attachment"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            // COPY_SRC allows a final evidence screenshot after the run.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    /// Clears an offscreen texture and reads it back for deterministic scenarios.
    ///
    /// Width and height are device pixels, not terminal cells. The caller has
    /// already converted grid dimensions using the M0 placeholder cell size.
    pub fn render_to_image(&self, width: u32, height: u32) -> Result<RgbaImage, RenderError> {
        // The harness rejects zero sizes before reaching this GPU path.
        let row_bytes = width
            .checked_mul(4)
            .ok_or_else(|| RenderError::Readback("row too wide".into()))?;
        // Alignment applies to the transfer stride, not the actual image width.
        let padded = row_bytes.div_ceil(256) * 256;
        // Buffer sizes are u64 in wgpu so large transfers cannot wrap a u32.
        let size = u64::from(padded) * u64::from(height);
        let texture = self.offscreen_texture(width, height);
        self.clear_view(&texture.create_view(&wgpu::TextureViewDescriptor::default()));
        // A separate mappable buffer is required because textures are GPU-owned.
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("offscreen readback"),
            size,
            // MAP_READ permits CPU access only after the GPU transfer completes.
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        self.copy_to_buffer(&texture, &buffer, padded, width, height);
        // Remove row padding before handing pixels to the scenario assertion.
        let pixels = self.map_pixels(&buffer, row_bytes, padded, height)?;
        Ok(RgbaImage {
            width,
            height,
            pixels,
        })
    }
    /// Transfers the texture into a GPU buffer with 256-byte-aligned rows.
    ///
    /// This only records and submits the copy; mapping is a separate step so
    /// the GPU can finish the clear and copy before CPU bytes are accessed.
    pub(crate) fn copy_to_buffer(
        &self,
        texture: &wgpu::Texture,
        buffer: &wgpu::Buffer,
        padded: u32,
        width: u32,
        height: u32,
    ) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("readback encoder"),
            });
        // The origin is the upper-left pixel and the aspect includes RGBA color.
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    // This stride is padded, even though returned rows are not.
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        // Queue ordering places the copy after the submitted clear pass.
        self.queue.submit(Some(encoder.finish()));
    }
    /// Waits for the transfer and removes padding from each mapped row.
    ///
    /// The callback reports mapping failure through a channel; no byte slice
    /// is read until device polling has completed that callback.
    pub(crate) fn map_pixels(
        &self,
        buffer: &wgpu::Buffer,
        row_bytes: u32,
        padded: u32,
        height: u32,
    ) -> Result<Vec<u8>, RenderError> {
        let (sender, receiver) = mpsc::channel();
        // Ignore a send failure only when the waiting receiver has gone away.
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            let _ = sender.send(result);
        });
        // Native wgpu invokes the mapping callback while the device is polled.
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|error| RenderError::Readback(error.to_string()))?;
        receiver
            .recv()
            // Both a lost callback and a failed mapping become harness errors.
            .map_err(|error| RenderError::Readback(error.to_string()))?
            .map_err(|error| RenderError::Readback(error.to_string()))?;
        let mapped = buffer
            .get_mapped_range(..)
            .map_err(|error| RenderError::Readback(error.to_string()))?;
        // Reserve the unpadded size exactly once for every image row.
        let mut pixels = Vec::with_capacity(row_bytes as usize * height as usize);
        for row in mapped.chunks_exact(padded as usize) {
            // Discard bytes from `row_bytes` through the padded stride.
            pixels.extend_from_slice(&row[..row_bytes as usize]);
        }
        // Unmap only after the mapped view is dropped, as wgpu requires.
        drop(mapped);
        buffer.unmap();
        Ok(pixels)
    }
}
