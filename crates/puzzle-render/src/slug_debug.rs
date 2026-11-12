//! Native-only capture of Slug's interpolated fragment inputs (§9.6).
//!
//! The production shader still receives rasterizer-interpolated em
//! coordinates. This debug render uses its identical vertex stage but writes
//! those coordinates and fwidth to an Rgba32Float target. The CPU oracle
//! then evaluates exactly what the GPU coverage math consumed, separating
//! shader fidelity from expected f32 interpolation differences.
//!
//! A fresh texture store gets a full CPU mirror upload. The ordinary draw
//! may already have drained dirty spans, so relying on incremental spans
//! here would produce a blank diagnostic after a successful normal frame.
//!
//! The four float channels are not a color. Red and green preserve the
//! sample's em coordinates, while blue and alpha preserve `fwidth` along
//! each em axis. The attachment uses a float format to avoid rounding a
//! coordinate to an 8-bit color before the oracle evaluates a curve root.
//! This matters most at large zoom, where a tiny em-space interpolation
//! difference moves an edge by a visible fraction of a device pixel.
//!
//! The diagnostic output is intentionally a separate pipeline rather than
//! a production shader branch. A branch could change optimization or
//! derivatives in the coverage pass and invalidate the fidelity test.
//! Both pipelines consume the same 56-byte instance and texture layouts.
//! Overlapping specimen glyphs require a third target for coverage before
//! blending; the last captured coordinate alone cannot explain their sum.
use crate::{RenderError, Renderer, SlugPipeline, SlugTextures};
use puzzle_frame::slug::SlugInstance;
use puzzle_slug::SlugAtlas;

/// One fragment's em coordinate and derivative, or zeros outside the quad.
pub struct SlugDebugPixel {
    /// Actual rasterizer-interpolated em coordinate in the horizontal axis.
    pub x: f32,
    /// Actual rasterizer-interpolated em coordinate in the vertical axis.
    pub y: f32,
    /// `fwidth` in ems per device pixel along the horizontal ray.
    pub ems_x: f32,
    /// `fwidth` in ems per device pixel along the vertical ray.
    pub ems_y: f32,
    /// One-based draw-instance index; zero denotes untouched clear pixels.
    pub instance_index: u32,
    /// This instance's analytic coverage before other glyphs blend over it.
    pub coverage: f32,
}

/// Test-only readback methods that leave the production shader unchanged.
impl Renderer {
    /// Captures float inputs for every pixel of one Slug fixture.
    pub fn render_slug_debug_inputs(
        &self,
        atlas: &mut SlugAtlas,
        instances: &[SlugInstance],
        width: u32,
        height: u32,
    ) -> Result<Vec<SlugDebugPixel>, RenderError> {
        // Invalid targets are rejected before multiplying row byte counts
        // or asking wgpu for an allocation larger than its normal limit.
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err(RenderError::Readback(
                "invalid Slug debug dimensions".into(),
            ));
        }
        // The debug pipeline shares the same vertex module and data plane
        // formats, so only its fragment entry point and target differ.
        let mut textures = SlugTextures::new(&self.device);
        // This full upload is necessary even when a preceding production
        // draw drained the atlas's incremental dirty-span queue.
        textures.sync_full(&self.device, &self.queue, atlas)?;
        let (curves, bands) = textures.views();
        // The debug source appends only an alternative fragment entry;
        // vertex dilation and interpolation remain production-identical.
        let mut pipeline = SlugPipeline::new_debug(
            &self.device,
            wgpu::TextureFormat::Rgba32Float,
            curves,
            bands,
        );
        pipeline.set_viewport(&self.queue, width, height);
        // The exact same instances make the diagnostic and production
        // triangles cover the same target pixels on this adapter. The
        // fragment's separate coverage output belongs to its named index.
        // Paint is not sampled by this fragment. Its flat word carries a
        // one-based index to a second integer target, so the CPU can choose
        // the matching glyph handle in a multi-instance specimen line.
        let diagnostic: Vec<_> = instances
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut copy = *item;
                copy.paint = index as u32 + 1;
                copy
            })
            .collect();
        pipeline.upload(&self.device, &self.queue, &diagnostic);
        let target = self.debug_target(width, height, wgpu::TextureFormat::Rgba32Float);
        let indices = self.debug_target(width, height, wgpu::TextureFormat::R32Uint);
        let coverage = self.debug_target(width, height, wgpu::TextureFormat::R32Float);
        self.draw_debug_target(
            &pipeline,
            &target,
            &indices,
            &coverage,
            instances.len() as u32,
        );
        let bytes = self.read_debug_target(&target, width, height, 16)?;
        let ids = self.read_debug_target(&indices, width, height, 4)?;
        let values = self.read_debug_target(&coverage, width, height, 4)?;
        // Parsing occurs only after the GPU map callback has completed.
        // Callers receive typed floats, not an aligned view of raw bytes.
        Ok(parse_debug_pixels(&bytes, &ids, &values))
    }

    /// Allocates an unfilterable float attachment with later copy access.
    fn debug_target(&self, width: u32, height: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
        // A render attachment alone cannot be inspected by the CPU;
        // COPY_SRC permits a later texture-to-buffer transfer.
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Slug fragment inputs"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    /// Runs the identical Slug vertex stage with the diagnostic fragment.
    fn draw_debug_target(
        &self,
        pipeline: &SlugPipeline,
        target: &wgpu::Texture,
        indices: &wgpu::Texture,
        coverage: &wgpu::Texture,
        count: u32,
    ) {
        // There is no production clear color here. Zero has diagnostic
        // meaning: pixels outside the dilated quad have no fwidth data.
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let index_view = indices.create_view(&wgpu::TextureViewDescriptor::default());
        let coverage_view = coverage.create_view(&wgpu::TextureViewDescriptor::default());
        // Commands are submitted before readback, preserving queue order
        // between the float write and its subsequent texture copy.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Slug debug input encoder"),
            });
        {
            // A transparent zero clear marks pixels outside the quad; valid
            // fragment derivatives are positive for these scene transforms.
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Slug debug input pass"),
                color_attachments: &[
                    clear_attachment(&view),
                    clear_attachment(&index_view),
                    clear_attachment(&coverage_view),
                ],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pipeline.draw(&mut pass, count);
        }
        // Closing the pass releases the attachment borrow before submit.
        // The texture stays live until readback completes below.
        self.queue.submit(Some(encoder.finish()));
    }

    /// Copies sixteen bytes per pixel through WebGPU's padded row stride.
    fn read_debug_target(
        &self,
        target: &wgpu::Texture,
        width: u32,
        height: u32,
        bytes_per_pixel: u32,
    ) -> Result<Vec<u8>, RenderError> {
        // Four f32 channels occupy sixteen bytes. GPU copy rows must be
        // multiples of 256 bytes even though returned rows are tightly packed.
        let row_bytes = width * bytes_per_pixel;
        let padded = row_bytes.div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Slug float input readback"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        // Existing readback machinery waits for the native GPU and removes
        // padding. It does not interpret channels as unorm or sRGB colors.
        self.copy_to_buffer(target, &buffer, padded, width, height);
        self.map_pixels(&buffer, row_bytes, padded, height)
    }
}

/// Clears each typed diagnostic plane to its zero sentinel before drawing.
fn clear_attachment(view: &wgpu::TextureView) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    // All three attachments reserve zero for an untouched pixel, so a
    // missing fragment cannot inherit a coordinate or index from a pass.
    Some(wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
        },
        depth_slice: None,
    })
}

/// Parses unorm-independent RGBA32Float bytes without alignment assumptions.
fn parse_debug_pixels(bytes: &[u8], ids: &[u8], values: &[u8]) -> Vec<SlugDebugPixel> {
    // Byte decoding avoids assuming Vec<u8> has the alignment required for
    // a borrowed &[f32]. It also avoids any handwritten unsafe cast (R8).
    bytes
        .chunks_exact(16)
        .zip(ids.chunks_exact(4))
        .zip(values.chunks_exact(4))
        .map(|((pixel, id), value)| {
            // WebGPU readback stores little-endian IEEE float bytes on the
            // native test device; each channel is decoded independently.
            let channel = |offset| {
                f32::from_le_bytes([
                    pixel[offset],
                    pixel[offset + 1],
                    pixel[offset + 2],
                    pixel[offset + 3],
                ])
            };
            SlugDebugPixel {
                x: channel(0),
                y: channel(4),
                ems_x: channel(8),
                ems_y: channel(12),
                instance_index: u32::from_le_bytes([id[0], id[1], id[2], id[3]]),
                coverage: f32::from_le_bytes([value[0], value[1], value[2], value[3]]),
            }
        })
        .collect()
}
