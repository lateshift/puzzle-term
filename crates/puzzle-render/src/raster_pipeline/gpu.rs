//! GPU-only grid crops are copied in the neutral upload stream's original order.
//!
//! No GPU page is mapped or mirrored. Capacity captures use copy-only buffers,
//! preserving exact earlier ink before the same sampled layer is cleared/reused.
//! The existing streamed executor selects those buffers by the instance epoch.
//! Source tickets retain R8 batch textures across skips and partial failures.
//! Actual copies validate both source and destination extents before submission.
//! Captures retain a particular layer incarnation, rather than a handle to its
//! mutable contents. Recycled layers can therefore serve later glyphs without
//! changing the coverage consumed by earlier draw batches in the same frame.
//! A crop batch shares one staging texture and one command encoder; atlas growth
//! must finish before encoding because growth replaces the texture array handle.
//! Validation failures leave the caller's uploads intact for diagnosis or retry.
//! Queue submission itself does not wait for the GPU; subsequent commands observe
//! its ordering through the same queue. A capacity snapshot is therefore ready
//! before the streamed executor copies it back for an earlier incarnation's draw.
//! CPU waits belong only to explicit diagnostic readback, outside this transfer.
use super::RasterPipeline;
use crate::{RenderError, Renderer};
use puzzle_frame::Upload;

/// Device-owned raster arrays execute the core's ordered resource operations.
impl RasterPipeline {
    /// Handles reproducible masks without converting GPU coverage back into CPU bytes.
    pub(crate) fn apply_gpu_upload(
        &mut self,
        renderer: &Renderer,
        upload: &Upload,
    ) -> Result<(), RenderError> {
        match upload {
            Upload::RasterCapture { color, page, epoch } => {
                // A capture may be the first operation naming a newly allocated layer.
                self.ensure_page(&renderer.device, &renderer.queue, *page, *color)?;
                // The epoch lets the streamed draw restore this exact incarnation later.
                self.versions.capture(
                    &renderer.device,
                    &renderer.queue,
                    if *color { &self.color } else { &self.mask }, // Independent pixel formats.
                    *page,
                    *epoch,
                    *color,
                );
            }
            Upload::RasterClear { page } => {
                // Reproducible grid coverage occupies only the single-channel mask array.
                self.ensure_page(&renderer.device, &renderer.queue, *page, false)?;
                self.clear_mask(renderer, *page);
            }
            _ => {} // CPU-backed uploads and vector texels are handled by their own owners.
        }
        Ok(())
    }
    /// Copies one contiguous cold batch with one submission, never one submit per glyph.
    pub(crate) fn copy_masks(
        &mut self,
        renderer: &Renderer,
        uploads: &[Upload],
    ) -> Result<(), RenderError> {
        let Some(Upload::GpuMask { ticket, .. }) = uploads.first() else {
            // An empty group has no source texture or GPU work to submit.
            return Ok(());
        };
        // Safe texture handles retain their backing storage after ticket acknowledgement.
        let image = renderer.mask_ticket(*ticket)?;
        // Find the highest destination layer once, before replacing a growing array.
        let page = uploads
            .iter()
            .filter_map(|u| match u {
                Upload::GpuMask { page, .. } => Some(*page), // Every crop shares the ticket.
                _ => None,
            })
            .max()
            .unwrap_or(0); // Nonempty valid groups always name at least layer zero.
        // Grow before encoding any copy, so all destinations name the final array.
        self.ensure_page(&renderer.device, &renderer.queue, page, false)?;
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        // Copy commands preserve the upload order, including crops sharing a destination.
        for upload in uploads {
            let Upload::GpuMask {
                source, page, rect, ..
            } = upload
            else {
                // The grouping caller normally supplies only crops; ignore other variants.
                continue;
            };
            // Checked extents reject malformed source/destination crops before GPU validation.
            validate_crop(image.size(), *source, *rect)?;
            encode_crop(&mut encoder, &image, &self.mask, *source, *page, *rect);
        }
        // The next capture or clear observes all crops in this batch, without any CPU fence.
        renderer.queue.submit(Some(encoder.finish()));
        Ok(())
    }
    /// Clears one recycled layer on the GPU, including every abandoned crop and gutter.
    fn clear_mask(&self, renderer: &Renderer, page: u32) {
        let view = self.mask.create_view(&wgpu::TextureViewDescriptor {
            // Restrict the render attachment to the victim, preserving all survivor layers.
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: page,
            array_layer_count: Some(1), // Clearing an array-wide view would erase survivors.
            ..Default::default()
        });
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        {
            // A clear attachment needs no shader or CPU-filled page-sized upload.
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("recycled GPU mask layer clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    // Store the clear because the next crop copies do not fill every texel.
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), // Zero coverage.
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None, // Ordinary 2D array layer, never a 3D texture slice.
                })],
                depth_stencil_attachment: None, // Glyph masks have no depth ordering.
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None, // Only the selected victim layer participates.
            });
        }
        // Queue order guarantees every earlier capture saw the old ink before this clear.
        renderer.queue.submit(Some(encoder.finish()));
    }
}

/// Both texture extents are checked before an untrusted frame can record a copy.
fn validate_crop(
    size: wgpu::Extent3d,
    source: [u32; 2],
    rect: puzzle_frame::RectU32,
) -> Result<(), RenderError> {
    // Reject empty copies; WebGPU requires both extent dimensions to be nonzero.
    if rect.width == 0
        || rect.height == 0
        // Checked addition avoids overflow disguising an out-of-bounds scratch crop.
        || source[0]
            .checked_add(rect.width)
            .is_none_or(|n| n > size.width)
        // The occupied scratch height may be shorter than the fixed atlas height.
        || source[1]
            .checked_add(rect.height)
            .is_none_or(|n| n > size.height)
        // Destination pages have the fixed neutral atlas extent, regardless of growth.
        || rect
            .x
            .checked_add(rect.width)
            .is_none_or(|n| n > super::PAGE)
        // A shelf crop must fit vertically inside the same destination page as its width.
        || rect
            .y
            .checked_add(rect.height)
            .is_none_or(|n| n > super::PAGE)
    {
        // Report a contract error before wgpu can raise a device validation failure.
        return Err(RenderError::Upload("GPU mask crop exceeds bounds".into()));
    }
    // Successful validation leaves texture handles untouched until the enclosing batch submits.
    Ok(())
}

/// Records one validated physical-texel crop between the staging sheet and sampled array.
fn encode_crop(
    encoder: &mut wgpu::CommandEncoder,
    image: &wgpu::Texture,
    mask: &wgpu::Texture,
    source: [u32; 2],
    page: u32,
    rect: puzzle_frame::RectU32,
) {
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            // Scratch sheet coordinates are physical texels, with no DPR transform.
            origin: wgpu::Origin3d {
                x: source[0],
                y: source[1],
                z: 0, // The temporary source is one 2D sheet, not an array.
            },
            ..image.as_image_copy()
        },
        wgpu::TexelCopyTextureInfo {
            // Each glyph's final shelf rectangle selects one sampled atlas layer.
            origin: wgpu::Origin3d {
                x: rect.x,
                y: rect.y,
                z: page, // Per-format page index, independent of CPU page ordering.
            },
            ..mask.as_image_copy()
        },
        wgpu::Extent3d {
            // The tight crop includes its transparent border for later sampling.
            width: rect.width,
            height: rect.height,
            depth_or_array_layers: 1, // A glyph cannot straddle array layers.
        },
    );
}
