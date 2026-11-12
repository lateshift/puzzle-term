//! Ordered executor for GPU-neutral terminal frames (§10.2–§10.3).
//!
//! The session chooses what to draw and supplies ordered batches. This
//! module owns persistent GPU pipelines and uploads their instance arrays
//! once per frame. Scissors clip grid slots without changing glyph origins.
//! Slug curves, ordinary bitmap crops and paint spans upload before the pass;
//! a draw never waits for readback or font parsing on the GPU path (P6).
//!
//! The frame vectors are GPU-independent. `puzzle-term` can assemble them
//! headlessly in tests, while this executor handles only wgpu resources.
//! The platform crate supplies the surface or offscreen view (R9).
//! Device-loss recovery can rebuild pipelines from CPU mirrors without
//! changing any terminal cell, cursor or line identity.
//!
//! Atlas uploads are acknowledged independently of image availability. A curve or
//! band texture may grow, replacing its sampled view; the Slug bind group
//! is rebuilt before a draw reads the newly prepared glyph address.
//! Queue ordering makes uploaded atlas texels visible to the render pass.
//!
//! Instance arrays upload once per pipeline, even when several batches
//! use them in different painter slots. This is why Batch holds a range
//! rather than an owned vector. Empty ranges cause no draw call, and
//! scissors clip the grid without changing glyph coordinates.
//!
//! Batches must arrive in monotonically increasing z-slots. Each range
//! is checked against its array before a GPU draw uses it. Scissors intersect
//! the published content canvas and acquired attachment, since resize can
//! replace the drawable after publication. Invalid ranges and overflowing
//! clip edges fail before wgpu receives an out-of-range command.
//!
//! The clear color is a frame value, not permanent renderer state. Theme
//! changes can recolor the background without recreating pipelines.
//! Premultiplied alpha in all three pipelines lets cell fills, Slug ink
//! and bitmap fallback share one pass and one presentation.
//! Native raster capacity pressure uses ordered continuation passes: earlier
//! ink is submitted before a physical page receives its next incarnation.
use crate::{RasterPipeline, RectPipeline, RenderError, Renderer, SlugPipeline, SlugTextures};
use puzzle_frame::{Batch, PipelineKind, RenderFrame, Upload};
/// Immutable format-bound pipelines shared across text contexts.
mod pipelines;
pub use pipelines::FramePipelines;
/// Cold-path GPU owner replacement and configured raster allowance.
mod budget;
/// Native harness readback of the same retained frame execution path.
mod offscreen;
/// Native atlas pressure submits complete draw segments before reusing a layer.
mod streaming;

/// Retained draw state for one device and render-target format.
pub struct FrameRenderer {
    /// Attachment format also used when reading back this exact retained executor.
    format: wgpu::TextureFormat,
    /// Context policy also bounds GPU allocations after a runtime reload.
    glyph_budget_bytes: usize,
    /// Curve and band planes backing Slug's analytic shader.
    textures: SlugTextures,
    /// Vector glyph draw state with a growable instance buffer.
    slug: SlugPipeline,
    /// Background, decoration and cursor draw state.
    rect: RectPipeline,
    /// R8 and RGBA atlas state for optional bitmap producers.
    raster: RasterPipeline,
    /// Lazy image cache moves with the surface rather than shared fonts.
    pub(crate) images: Option<Box<crate::ImageCache>>,
}

impl FrameRenderer {
    /// Creates all three pipelines once, outside the frame loop.
    #[must_use]
    pub fn new(renderer: &Renderer, format: wgpu::TextureFormat) -> Self {
        Self::with_pipelines(renderer, &FramePipelines::new(renderer, format))
    }

    /// Creates context-bound textures and buffers while reusing process shader state (D-30).
    #[must_use]
    pub fn with_pipelines(renderer: &Renderer, pipelines: &FramePipelines) -> Self {
        let format = pipelines.format;
        // Texture views must exist before Slug's bind group is created.
        // Their formats are fixed; the supplied format chooses only the
        // pass attachment accepted by the three render pipelines.
        let textures = SlugTextures::new(&renderer.device);
        let (curves, bands) = textures.views();
        let slug = SlugPipeline::shared(&renderer.device, format, curves, bands, &pipelines.slug);
        // The raster vertex stage borrows Slug's existing effect/paint rows.
        let raster = RasterPipeline::shared(
            &renderer.device,
            format,
            slug.paint_view(),
            Some(&pipelines.raster),
        );
        Self {
            format,
            glyph_budget_bytes: puzzle_slug::atlas::DEFAULT_BUDGET_BYTES,
            slug,
            rect: RectPipeline::shared(&renderer.device, format, Some(&pipelines.rect)),
            raster,
            textures,
            images: None,
        }
    }

    /// Uploads a frame and draws all its ordered batches to one target view.
    ///
    /// The return value counts nonempty draw calls for the §10.3 budget.
    pub fn render_view(
        &mut self,
        renderer: &Renderer,
        frame: &mut RenderFrame,
        view: &wgpu::TextureView,
    ) -> Result<u32, RenderError> {
        // The ordinary path has no observer; monomorphization removes this closure.
        self.render_view_observed(renderer, frame, view, || {})
    }

    /// Executes the same frame, notifying a host when atlas/instance uploads finish.
    ///
    /// The observer can timestamp CPU phases without a GPU fence or readback.
    /// It must not mutate the supplied frame or submit another render operation.
    pub fn render_view_observed(
        &mut self,
        renderer: &Renderer,
        frame: &mut RenderFrame,
        view: &wgpu::TextureView,
        prepared: impl FnOnce(),
    ) -> Result<u32, RenderError> {
        // A newly prepared outline must be uploaded before its instance
        // references the band address in this frame's fragment shader.
        self.prepare(renderer, frame, view)?;
        // Queue writes have finished; the remaining phase records and submits draws.
        prepared();
        // Ordinary frames retain the single pass; capacity pressure adds ordered flushes.
        if self.raster.has_versions() {
            return self.render_streamed(renderer, frame, view);
        }
        // One encoder and one pass serve all visible terminal rows. The
        // draw count below measures state batches rather than cell count.
        let mut encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("terminal frame encoder"),
            });
        let mut draws = 0;
        {
            // The clear uses the resolved theme color for this exact
            // frame, so appearance switches need no separate clear pass.
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terminal frame pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(color(frame.clear)),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // The builder already chose painter order. Sorting here would
            // move cursor redraws under text or overlays unpredictably.
            let mut previous = None;
            for batch in &frame.batches {
                validate_batch(frame, batch, previous)?;
                previous = Some(batch.slot);
                draws += self.draw_batch(&mut pass, batch, drawable_clip(frame, view));
            }
        }
        // Earlier queue writes for instances and atlases precede this
        // submission without a CPU fence or mapped texture readback.
        renderer.queue.submit(Some(encoder.finish()));
        Ok(draws)
    }

    /// Presents one frame on a surface owned by the platform shell.
    pub fn render_surface(
        &mut self,
        renderer: &Renderer,
        frame: &mut RenderFrame,
        surface: &wgpu::Surface<'_>,
    ) -> Result<u32, RenderError> {
        // Upload ownership is independent of drawable availability. Timeout,
        // occlusion and reconfiguration may skip an image, never cached glyphs.
        self.write_frame_uploads(renderer, frame)?;
        // Lost and outdated acquisitions go back to the platform shell,
        // which owns surface configuration and window/canvas dimensions.
        let output = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output)
            | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
            wgpu::CurrentSurfaceTexture::Timeout => return Err(RenderError::Surface("timeout")),
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(0),
            wgpu::CurrentSurfaceTexture::Outdated => return Err(RenderError::Surface("outdated")),
            wgpu::CurrentSurfaceTexture::Lost => return Err(RenderError::Surface("lost")),
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(RenderError::Surface("validation"));
            }
        };
        // Presentation follows successful submission of the acquired
        // texture; the view must remain live throughout render_view.
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let draws = self.render_view(renderer, frame, &view)?;
        renderer.queue.present(output);
        Ok(draws)
    }

    /// Applies atlas changes without drawing a frame.
    ///
    /// A diagnostic renderer can follow every presented upload even when it
    /// only reads back selected idle frames. Incremental frame uploads omit
    /// earlier glyphs, so skipping this step would make those glyphs blank.
    pub fn apply_uploads(
        &mut self,
        renderer: &Renderer,
        uploads: &[Upload],
    ) -> Result<(), RenderError> {
        // Each storage owner ignores variants outside its atlas format.
        // Preserving this order supports new glyphs used in the same frame.
        for chunk in uploads.chunk_by(|a, b| {
            matches!((a, b),
            (Upload::GpuMask { ticket: a, .. }, Upload::GpuMask { ticket: b, .. }) if a == b)
        }) {
            if matches!(chunk.first(), Some(Upload::GpuMask { .. })) {
                self.raster.copy_masks(renderer, chunk)?;
                continue;
            }
            for upload in chunk {
                if self
                    .textures
                    .apply_upload(&renderer.device, &renderer.queue, upload)?
                {
                    // Bind the new views before a later upload can fail. Retrying a
                    // partial batch must never leave handles sampling the old store.
                    let (curves, bands) = self.textures.views();
                    self.slug.rebind(&renderer.device, curves, bands);
                }
                self.raster.apply_gpu_upload(renderer, upload)?;
                self.raster
                    .apply_upload(&renderer.device, &renderer.queue, upload)?;
                if let Upload::Paint { first, colors } = upload
                    && self
                        .slug
                        .upload_paint(&renderer.device, &renderer.queue, *first, colors)?
                {
                    let (curves, bands) = self.textures.views();
                    self.slug.rebind(&renderer.device, curves, bands);
                    // Both consumers must bind the grown table before effect quads draw.
                    self.raster
                        .set_paint_view(&renderer.device, self.slug.paint_view());
                }
            }
        }
        Ok(())
    }

    /// Transfers dirty texture spans and instance arrays before drawing.
    fn prepare(
        &mut self,
        renderer: &Renderer,
        frame: &mut RenderFrame,
        view: &wgpu::TextureView,
    ) -> Result<(), RenderError> {
        // Zero-size/minimized targets may still introduce glyphs. The queue can
        // own those bytes without a surface; validate image dimensions afterward.
        self.write_frame_uploads(renderer, frame)?;
        // Zero or oversized dimensions cannot form a valid WebGPU pass;
        // the platform normally skips such minimized targets earlier.
        let [width, height] = frame.target_size;
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err(RenderError::Upload("invalid frame target size".into()));
        }
        // Snapshot size is not drawable size: resize may happen after publication.
        // Project original device-pixel coordinates onto the acquired attachment,
        // preserving top-left ink rather than stretching an old frame to fit.
        let size = view.texture().size();
        let (width, height) = (size.width, size.height);
        // All pipelines use the same physical target dimensions, while
        // only Slug needs its em-to-device dilation uniform as well.
        self.slug.set_viewport(&renderer.queue, width, height);
        self.rect.set_viewport(&renderer.queue, width, height);
        self.raster.set_viewport(&renderer.queue, width, height);
        // A contiguous write per pipeline lets later z-slots reuse slices
        // without duplicating the instance bytes on the GPU.
        self.slug
            .upload(&renderer.device, &renderer.queue, &frame.slugs);
        self.rect
            .upload(&renderer.device, &renderer.queue, &frame.rects);
        self.raster
            .upload(&renderer.device, &renderer.queue, &frame.rasters);
        self.prepare_images(renderer, frame, [width, height])?;
        Ok(())
    }

    /// Transfers pending glyph/atlas/paint bytes before any presentation decision.
    /// Failed writes leave the entire batch pending; reapplying already written
    /// prefixes is safe because every span carries its complete replacement bytes.
    pub fn write_frame_uploads(
        &mut self,
        renderer: &Renderer,
        frame: &mut RenderFrame,
    ) -> Result<(), RenderError> {
        self.apply_uploads(renderer, &frame.uploads)?;
        // Queue writes own a copy. The next submission flushes them even if this
        // image has no drawable, so future row-cache hits need no replacement.
        renderer.acknowledge_masks(&frame.uploads)?;
        frame.uploads.clear();
        Ok(())
    }

    /// Applies one clip and issues one nonempty draw from a frame batch.
    fn draw_batch(&self, pass: &mut wgpu::RenderPass<'_>, batch: &Batch, target: [u32; 2]) -> u32 {
        // A frame with no selection or bitmap glyphs should not spend a
        // driver draw call for those otherwise valid empty batches.
        if batch.instances.is_empty() {
            return 0;
        }
        let mut clip = batch.scissor.unwrap_or(puzzle_frame::RectU32 {
            x: 0,
            y: 0,
            width: target[0],
            height: target[1],
        });
        // Intersect both implicit and explicit clips with the actual drawable.
        // Saturating edges handle an entirely clipped batch without unsigned wrap.
        clip.width = clip.width.min(target[0].saturating_sub(clip.x));
        clip.height = clip.height.min(target[1].saturating_sub(clip.y));
        // Zero-area clips produce no pixels, and WebGPU need not see a
        // draw for them even if the batch contains instances.
        if clip.width == 0 || clip.height == 0 {
            return 0;
        }
        // Scissor changes remain inside the same pass; the Top slot uses
        // the whole target when no explicit grid clip was supplied.
        pass.set_scissor_rect(clip.x, clip.y, clip.width, clip.height);
        match batch.pipeline {
            PipelineKind::Rect => self.rect.draw(pass, batch.instances.clone()),
            PipelineKind::Slug => self.slug.draw_range(pass, batch.instances.clone()),
            PipelineKind::Raster => self.raster.draw(pass, batch.instances.clone()),
            PipelineKind::Image(_) => {
                return self.images.as_ref().map_or(0, |images| {
                    images.draw(pass, &self.raster.pipeline, batch.instances.clone())
                });
            }
        }
        1
    }
}

/// Rejects invalid ranges or reversed painter order before GPU submission.
fn validate_batch(
    frame: &RenderFrame,
    batch: &Batch,
    previous: Option<puzzle_frame::ZSlot>,
) -> Result<(), RenderError> {
    // Painter order is structural. A cursor at slot four must not appear
    // beneath glyphs at slot two after a later batch is appended.
    if previous.is_some_and(|slot| batch.slot < slot) {
        return Err(RenderError::Upload(
            "frame batches are out of z-order".into(),
        ));
    }
    let len = match batch.pipeline {
        PipelineKind::Rect => frame.rects.len(),
        PipelineKind::Slug => frame.slugs.len(),
        PipelineKind::Raster | PipelineKind::Image(_) => frame.rasters.len(),
    };
    // CPU range checks give a precise error before a vertex buffer offset
    // could reach the driver as a vague validation failure.
    if batch.instances.start > batch.instances.end || batch.instances.end as usize > len {
        return Err(RenderError::Upload(
            "frame batch exceeds instance array".into(),
        ));
    }
    // Reject unrepresentable edges, but stale grid scissors may exceed a newer
    // content canvas. draw_batch intersects them with both content and drawable.
    if let Some(clip) = batch.scissor
        && (clip.x.checked_add(clip.width).is_none() || clip.y.checked_add(clip.height).is_none())
    {
        return Err(RenderError::Upload("frame scissor edge overflow".into()));
    }
    Ok(())
}

/// Converts the neutral clear tuple to wgpu's color record.
fn color([r, g, b, a]: [f32; 4]) -> wgpu::Color {
    wgpu::Color {
        r: f64::from(r),
        g: f64::from(g),
        b: f64::from(b),
        a: f64::from(a),
    }
}

#[cfg(test)]
/// Stale published geometry against a newly acquired physical target.
mod tests;

/// Physical intersection of the published content canvas and acquired attachment.
/// DPR is already baked into both extents; it must never be applied a second time.
fn drawable_clip(frame: &RenderFrame, view: &wgpu::TextureView) -> [u32; 2] {
    let size = view.texture().size();
    [
        frame.target_size[0].min(size.width),
        frame.target_size[1].min(size.height),
    ]
}
