//! Neutral raster uploads retain CPU image sources and GPU-only coverage tickets.
//!
//! Captures precede page clears and replacement crops; the executor preserves this
//! order before selecting immutable page incarnations for each draw segment.
//! Pending writes survive skipped images. Only a successful GPU writer acknowledges
//! tickets. Authored bitmap samples remain restorable; vector-backed masks rebuild.
//!
//! A page can be recycled several times while one image is assembled. The upload
//! stream records every old incarnation before its following clear and replacement
//! crops. Draw stamps select those immutable versions even for a late cursor copy
//! that reuses a glyph from an earlier incarnation. Surviving shelf UVs never move.
//! A skipped image replaces geometry while retaining writes needed by future cache
//! hits. Its old draw versions can be discarded, but its coverage crops cannot.
//! CPU-authored samples carry bytes; GPU crops carry only a device-local identity
//! and a shared lifetime marker. Layout never sees a GPU texture or waits for it.
use super::super::context::TextContext;
use puzzle_frame::{RectU32, RenderFrame, Upload};
use puzzle_text::glyph::BitmapKind;

/// Refreshes page ages and transfers owned writes to the pending frame queue.
pub(in crate::frame_builder) fn collect(
    context: &mut TextContext,
    frame: &mut RenderFrame,
) -> Result<(), String> {
    // Cold batches register GPU-owned coverage before crop tickets leave the text owner.
    context.raster.flush_grid()?;
    // Actual assembled use, including cache hits, determines the LRU victim.
    for draw in &frame.rasters {
        let kind = if draw.flags & 1 == 0 {
            BitmapKind::Mask
        } else {
            BitmapKind::Color
        };
        // Page bits name the independent mask or color array, never a VT row.
        context.raster.mark_used(kind, (draw.flags >> 8) & 0xff);
        // Bitmap effect matrices/tints share the paint table's eviction clock.
        if draw.flags & puzzle_frame::raster::RASTER_EFFECT != 0 {
            context.backend.mark_color_used(draw.tint);
        }
    }
    // A native build replaces prior staging even when this frame needs no flush.
    if context.raster.is_synchronous() {
        // Prior geometry is replaced; only its mirror writes, not its draw versions, survive.
        frame
            .uploads
            .retain(|upload| !matches!(upload, Upload::RasterReset | Upload::RasterPage { .. }));
        frame.uploads.push(Upload::RasterReset);
    }
    append_dirty(context, frame);
    append_versions(context, frame);
    Ok(())
}

/// Preserves crop, capture and clear order while moving ownership into the frame.
fn append_dirty(context: &mut TextContext, frame: &mut RenderFrame) {
    // Authored images retain CPU mirrors; generated coverage transfers only GPU tickets.
    for upload in context.raster.take_dirty() {
        let rect = RectU32 {
            // Both producers use atlas texels; device glyph origins stay in the instances.
            x: upload.rect.x,
            y: upload.rect.y,
            width: upload.rect.width,
            height: upload.rect.height,
        };
        // GPU tickets and captures stay in the same queue order as authored bitmap writes.
        if let Some(epoch) = upload.capture {
            // A capture must precede every upload that mutates this layer's contents.
            frame.uploads.push(Upload::RasterCapture {
                color: upload.kind == BitmapKind::Color,
                page: upload.page,
                epoch,
            });
            continue;
        }
        if let Some((ticket, source, lease)) = upload.gpu {
            frame.uploads.push(Upload::GpuMask {
                ticket,
                lease, // Retain staging ownership across a skipped acquisition or partial retry.
                source,
                page: upload.page,
                rect,
            });
            continue;
        }
        if upload.pixels.is_empty() {
            // Pure GPU pages have no zero-filled CPU mirror; their reset is a GPU clear.
            frame
                .uploads
                .push(Upload::RasterClear { page: upload.page });
            continue;
        }
        // RenderFrame retains ordered writes across hidden or unacquired frames.
        frame.uploads.push(match upload.kind {
            // Authored masks and the CPU oracle use one-byte coverage with terminal tint.
            BitmapKind::Mask => Upload::Mask {
                page: upload.page,
                rect,
                pixels: upload.pixels,
            },
            BitmapKind::Color => Upload::Color {
                // Authored emoji retain their full RGBA source, independently of grid masks.
                page: upload.page,
                rect,
                pixels: upload.pixels,
            },
            // CPU validation rejects deferred subpixel data before insertion.
            BitmapKind::Subpixel => continue,
        });
    }
}

/// Final snapshots let unchanged future frames sample the current shelf incarnation.
fn append_versions(context: &mut TextContext, frame: &mut RenderFrame) {
    // Page images are copied between draw segments, never all overlaid before ink.
    for version in context.raster.take_versions() {
        // Stamps keep earlier cursor/selection copies attached to the ink they prepared.
        // The executor uses copies between submissions rather than allocating atlas layers.
        // Final per-layer images restore cache residency for the following frame.
        if version.pixels.is_empty() {
            // Generated or mixed page ink must be captured on the device, never read back.
            frame.uploads.push(Upload::RasterCapture {
                color: version.kind == BitmapKind::Color,
                page: version.page,
                epoch: version.epoch,
            });
            continue;
        }
        frame.uploads.push(Upload::RasterPage {
            color: version.kind == BitmapKind::Color,
            page: version.page,
            epoch: version.epoch,
            pixels: version.pixels,
        });
    }
}
