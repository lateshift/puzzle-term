//! Submit complete draw segments before replacing referenced raster page contents.
//!
//! This is the capacity-only path requested by the M16 reviewer. A frame still
//! has one target and one presentation. Its first segment clears the target;
//! subsequent passes load the ink already drawn. Queue submissions preserve
//! upload/draw order without CPU fences. Each raster segment groups consecutive
//! instances until a physical layer needs a different incarnation. Mixed pages
//! and glyphs with the same contents remain one instanced draw, never a routine
//! draw per glyph. Painter order, scissors, selection and cursor copies are
//! preserved, including a later draw that refers back to an earlier incarnation.
//!
//! The attachment is not presented between submissions: users see only the
//! completed frame. A flush preserves previous color values with LoadOp::Load;
//! clearing each continuation would erase the very ink it protects. The same
//! rule applies to masks created by vector-atlas overflow and bitmap emoji.
//! Ten stack slots track only existing sampled layers, without a residency map
//! or a per-glyph allocation. High stamp bits select CPU-owned upload images;
//! the shader masks them off and samples the existing layer index. A final copy
//! leaves later upload-free frames on the latest shelf layout. Ordinary frames
//! use the parent executor's original single pass and have no staging images.
//! An empty geometry stream still owns its clear color, which matters for the
//! background-only diagnostic image. It records a zero-draw pass rather than
//! leaving a new offscreen texture uninitialized. Retained staging is not a
//! reason to suppress a clear. Copies after that empty pass update only atlas
//! textures; they cannot alter the attachment's paper color. This also keeps
//! readback comparisons independent of whatever ink the previous frame drew.
use super::{
    Batch, FrameRenderer, PipelineKind, RenderError, RenderFrame, Renderer, color, validate_batch,
};

/// Eight mask and two color layers have independent resident-content stamps.
type Stamps = [Option<u32>; 10];

/// The executor is the only owner that may submit GPU draws (R9).
impl FrameRenderer {
    /// Executes a capacity-pressure frame using load passes and ordered submissions.
    pub(super) fn render_streamed(
        &self,
        renderer: &Renderer,
        frame: &RenderFrame,
        view: &wgpu::TextureView,
    ) -> Result<u32, RenderError> {
        let mut previous = None;
        // Validate every range before any partial frame reaches the queue.
        for batch in &frame.batches {
            validate_batch(frame, batch, previous)?;
            previous = Some(batch.slot);
        }
        let mut resident = [None; 10];
        // Unknown residency forces the first occurrence to select its captured image.
        let mut first = true;
        // This counts actual instanced draws, including capacity-required continuations.
        let mut draws = 0;
        for batch in &frame.batches {
            // Empty authored batches need neither a load pass nor a page copy.
            let mut start = batch.instances.start;
            while start < batch.instances.end {
                let (end, stamps) = segment(frame, batch, start)?;
                // Only the range changes: painter slot and scissor remain authored values.
                let piece = Batch {
                    instances: start..end,
                    ..batch.clone()
                };
                // Submit ink before a following segment may replace a referenced page.
                draws +=
                    self.draw_segment(renderer, frame, view, &piece, &stamps, &mut resident, first);
                first = false;
                // The conflicting instance starts the next range and is never discarded.
                start = end;
            }
        }
        if first {
            // Background-only readback can have no geometry, but still needs its clear.
            let empty = Batch {
                slot: puzzle_frame::ZSlot::Background,
                pipeline: PipelineKind::Rect,
                instances: 0..0,
                scissor: None,
            };
            self.draw_segment(
                renderer,
                frame,
                view,
                &empty,
                &[None; 10],
                &mut resident,
                true,
            );
        }
        // A cursor may have selected an old image last; restore final CPU mirrors
        // before another session or an unchanged row samples these physical layers.
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        self.raster.restore_versions(&mut encoder);
        renderer.queue.submit(Some(encoder.finish()));
        Ok(draws)
    }

    /// Copies required pages, then draws one contiguous painter-ordered segment.
    #[allow(
        clippy::too_many_arguments,
        reason = "one segment borrows its target, frame and ten-layer residency"
    )]
    fn draw_segment(
        &self,
        renderer: &Renderer,
        frame: &RenderFrame,
        view: &wgpu::TextureView,
        batch: &Batch,
        stamps: &Stamps,
        resident: &mut Stamps,
        first: bool,
    ) -> u32 {
        let mut encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("raster capacity draw segment"),
            });
        for (slot, stamp) in stamps.iter().enumerate() {
            // A segment can sample several layers without copying any unchanged one.
            if let Some(stamp) = stamp
                && resident[slot] != Some(*stamp)
            {
                // The preceding submission already owns every draw of its old contents.
                self.raster.copy_version(&mut encoder, *stamp);
                resident[slot] = Some(*stamp);
            }
        }
        let draws = {
            // Loading the attachment keeps earlier segments' exact premultiplied ink.
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("raster capacity continuation"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    // Store keeps all earlier pixels available to the next continuation.
                    ops: wgpu::Operations {
                        load: if first {
                            wgpu::LoadOp::Clear(color(frame.clear))
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                // This is the same three-pipeline color path; no depth ordering is added.
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.draw_batch(&mut pass, batch, super::drawable_clip(frame, view))
        };
        // Queue order, rather than a CPU wait, protects the pixels already drawn.
        renderer.queue.submit(Some(encoder.finish()));
        draws
    }
}

/// Finds the largest contiguous draw range with one incarnation per physical layer.
fn segment(frame: &RenderFrame, batch: &Batch, start: u32) -> Result<(u32, Stamps), RenderError> {
    let mut stamps = [None; 10];
    // Only raster instances carry page incarnation metadata in their high flags.
    if batch.pipeline != PipelineKind::Raster {
        // Rects and Slug draws preserve their original ranges and painter slots.
        return Ok((batch.instances.end, stamps));
    }
    let mut end = start;
    // Inspect a contiguous slice without allocating per-glyph commands or vectors.
    for index in start..batch.instances.end {
        let flags = frame.rasters[index as usize].flags;
        let page = ((flags >> 8) & 0xff) as usize;
        // Authored color and tinted coverage address separate physical texture arrays.
        let color = flags & 1 != 0;
        if page >= if color { 2 } else { 8 } {
            return Err(RenderError::Upload(
                "raster instance exceeds page budget".into(),
            ));
        }
        let slot = page + if color { 8 } else { 0 };
        // Affine/recolor metadata changes appearance, never the contents to restore.
        let stamp = flags & 0xffff_ff01;
        // A conflict closes this segment before the conflicting glyph is recorded.
        if stamps[slot].is_some_and(|resident| resident != stamp) {
            break;
        }
        stamps[slot] = Some(stamp);
        // At least the first valid instance fits, so every range makes progress.
        end = index + 1;
    }
    Ok((end, stamps))
}
