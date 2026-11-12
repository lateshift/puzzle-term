//! Native offscreen draw and readback for Slug oracle comparison (§10.6).
//!
//! The headless harness passes a CPU atlas and explicit instances. This
//! method allocates a target for one deterministic scene, synchronizes its
//! two packed texture planes and draws all instances in one indexed pass.
//! It then uses the same padded GPU readback as the M0 clear screenshot.
//!
//! Production frames will retain pipeline and texture objects across
//! calls. The isolated scene method keeps test setup small and ensures a
//! comparison cannot accidentally depend on a previous frame's resources.
//!
//! A black RGBA8 target makes oracle coverage directly observable in each
//! RGB channel when the test instance is opaque white. The draw uses
//! premultiplied alpha, so a fractional white edge over black becomes its
//! coverage byte. The CPU oracle produces the same grayscale convention.
//! Other colors and compositing are exercised separately from this strict
//! numerical comparison. Keeping one color avoids conflating a shader
//! coverage error with an sRGB blend conversion in the test harness.
//! The D-47 mask producer also uses these draw/readback helpers on the same device.
//! It retains its pipeline separately and groups several cold glyphs into one pass.
//! Coverage quantization therefore shares this already-verified white-on-black path.
//! Reading the sheet back establishes restoration mirrors before page reuse.
use crate::{RenderError, Renderer, RgbaImage, SlugPipeline, SlugTextures};
use puzzle_frame::slug::SlugInstance;
use puzzle_slug::{ShapeKey, SlugAtlas, atlas::FillRule, synthetic::SyntheticShape};

/// Draws one shape batch into a new RGBA8 target and returns actual GPU bytes.
impl Renderer {
    /// Draws a built-in rectangle through the real GPU path for harness smoke.
    pub fn render_synthetic_slug_to_image(&self) -> Result<RgbaImage, RenderError> {
        let mut atlas = SlugAtlas::default();
        let handle = atlas
            .prepare(
                ShapeKey(1),
                &SyntheticShape::Rectangle.path(),
                FillRule::NonZero,
                0,
            )
            .map_err(|error| RenderError::Upload(error.to_string()))?;
        // Em y is up while image y is down. The origin places the shape
        // inside a 64×64 target with room for its antialiased fringe.
        let instance = SlugInstance {
            m: [48.0, 0.0, 0.0, -48.0],
            t: [8.0, 56.0],
            bbox: handle.bbox,
            glyph_loc: puzzle_slug::encoding::encode_glyph_loc(handle.glyph_loc),
            band_info: handle.band_info,
            color: 0xffffffff,
            paint: 0,
        };
        // Read the pixels before asking the CPU mirror for expected values:
        // upload drains dirty ranges but leaves the packed texels intact.
        let gpu = self.render_slug_to_image(&mut atlas, &[instance], 64, 64)?;
        // The probe's axis-aligned transform is exactly invertible by the
        // oracle rasterizer, so it isolates the WGSL coverage and uploads.
        let cpu = puzzle_slug::reference::rasterize(
            &atlas,
            handle,
            64,
            64,
            puzzle_slug::reference::RasterTransform {
                scale: [48.0, -48.0],
                translate: [8.0, 56.0],
            },
        )
        .map_err(|error| RenderError::Upload(error.to_string()))?;
        // Each channel must stay within one quantization step. A first
        // mismatch index locates the precise device pixel for debugging.
        for (index, (actual, expected)) in gpu.pixels.iter().zip(&cpu.pixels).enumerate() {
            if actual.abs_diff(*expected) > 1 {
                return Err(RenderError::Upload(format!(
                    "oracle mismatch index={index} GPU={actual} CPU={expected}"
                )));
            }
        }
        Ok(gpu)
    }

    /// Renders a packed Slug batch for native CPU-oracle comparison.
    pub fn render_slug_to_image(
        &self,
        atlas: &mut SlugAtlas,
        instances: &[SlugInstance],
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, RenderError> {
        // Ordinary instances use their packed color and never read the
        // reserved paint row; the alternate method supplies a table texel.
        self.render_slug_inner(atlas, instances, width, height, None)
    }

    /// Renders prebuilt Slug texels without opening or parsing a font file.
    pub fn render_packed_slug_to_image(
        &self,
        curves: &[[f32; 4]],
        bands: &[[u16; 2]],
        instances: &[SlugInstance],
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, RenderError> {
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err(RenderError::Readback(
                "invalid pack target dimensions".into(),
            ));
        }
        // The pack loader already validated texel bounds and finite data.
        // This path only uploads its CPU mirrors and draws existing handles.
        // It never opens a TTF or asks skrifa to prepare an outline,
        // making the pack-only first-frame contract testable.
        let mut textures = SlugTextures::new(&self.device);
        textures.sync_pack(&self.device, &self.queue, curves, bands)?;
        self.draw_slug_with_textures(&textures, instances, width, height, None)
    }

    /// Exercises a nonzero paint-table index without changing Slug coverage.
    #[cfg(feature = "oracle-debug")]
    pub fn render_slug_paint_to_image(
        &self,
        atlas: &mut SlugAtlas,
        instances: &[SlugInstance],
        width: u32,
        height: u32,
        rgba: [f32; 4],
    ) -> Result<RgbaImage, RenderError> {
        // Both paint paths share target format and readback so a different
        // green result can only come from the nonzero paint-table lookup.
        self.render_slug_inner(atlas, instances, width, height, Some(rgba))
    }

    /// Shares target, upload and readback logic with the solid-paint probe.
    fn render_slug_inner(
        &self,
        atlas: &mut SlugAtlas,
        instances: &[SlugInstance],
        width: u32,
        height: u32,
        paint: Option<[f32; 4]>,
    ) -> Result<RgbaImage, RenderError> {
        // This method is an isolated test target. Production frame rendering
        // retains textures and pipelines instead of rebuilding per image.
        // The native harness owns the device; this shared renderer receives
        // only packed Slug data and never creates a window or GPU instance.
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err(RenderError::Readback(
                "invalid Slug target dimensions".into(),
            ));
        }
        // Synchronize curve and band mirrors before binding the views. A
        // newly prepared glyph may refer to rows not present on the GPU yet.
        let mut textures = SlugTextures::new(&self.device);
        textures.sync_full(&self.device, &self.queue, atlas)?;
        self.draw_slug_with_textures(&textures, instances, width, height, paint)
    }

    /// Shares the instanced pass and readback for runtime and pack texels.
    fn draw_slug_with_textures(
        &self,
        textures: &SlugTextures,
        instances: &[SlugInstance],
        width: u32,
        height: u32,
        paint: Option<[f32; 4]>,
    ) -> Result<RgbaImage, RenderError> {
        // Runtime glyphs and prebuilt packs converge on identical GPU
        // texture views before creating the instanced Slug pipeline.
        let (curves, bands) = textures.views();
        // The instanced shader compiles for the same RGBA8 format used by
        // readback; changing format would alter edge bytes independently.
        let mut pipeline =
            SlugPipeline::new(&self.device, wgpu::TextureFormat::Rgba8Unorm, curves, bands);
        // Index one reads the first paint texel. The default zero index
        // skips the table and keeps the instance's packed solid color.
        if let Some(rgba) = paint {
            // Queue ordering places the paint write before the draw, and
            // index one addresses the table's first texel exactly.
            pipeline.set_probe_paint(&self.queue, rgba);
        }
        pipeline.set_viewport(&self.queue, width, height);
        // Upload before beginning the pass, because buffer writes and the
        // draw are ordered through the same queue submission stream.
        pipeline.upload(&self.device, &self.queue, instances);
        let target = self.new_slug_target(width, height);
        self.draw_slug_target(&target, &pipeline, instances.len() as u32);
        self.read_slug_target(&target, width, height)
    }

    /// Creates a readback-capable target in the shader's exact output format.
    fn new_slug_target(&self, width: u32, height: u32) -> wgpu::Texture {
        // COPY_SRC is required for the later texture-to-buffer transfer;
        // a render attachment by itself cannot be mapped on the CPU.
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Slug oracle GPU target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    /// Clears and submits one Slug batch without retaining prior pixels.
    pub(crate) fn draw_slug_target(
        &self,
        target: &wgpu::Texture,
        pipeline: &SlugPipeline,
        count: u32,
    ) {
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        // One pass clears and draws; retained prior pixels cannot influence
        // an oracle comparison, including when a shape leaves empty space.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Slug oracle frame"),
            });
        {
            // Black is opaque so premultiplied white fragment output maps
            // straight to the reference's grayscale coverage byte.
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Slug oracle draw"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pipeline.draw(&mut pass, count);
        }
        // Closing the pass before submit releases the encoder's borrow of
        // its attachments, allowing the target to be copied afterward.
        self.queue.submit(Some(encoder.finish()));
    }

    /// Copies a rendered RGBA8 target through WebGPU's padded buffer stride.
    pub(crate) fn read_slug_target(
        &self,
        target: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, RenderError> {
        // The transfer stride is padded independently of the tightly packed
        // RGBA rows returned to oracle comparison and the PNG writer.
        let row_bytes = width * 4;
        let padded = row_bytes.div_ceil(256) * 256;
        // Mapping a texture directly is unavailable in WebGPU. The transfer
        // buffer has a padded stride but only unpadded bytes enter the PNG.
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Slug oracle readback"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        self.copy_to_buffer(target, &buffer, padded, width, height);
        // The shared M0 readback waits for completion and strips padding,
        // so M3's oracle comparisons use the exact bytes the GPU wrote.
        let pixels = self.map_pixels(&buffer, row_bytes, padded, height)?;
        Ok(RgbaImage {
            width,
            height,
            pixels,
        })
    }
}
