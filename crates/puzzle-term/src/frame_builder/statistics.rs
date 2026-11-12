//! Optional frame counters wrap the normal builder; hidden HUDs take no clocks.
//!
//! The publication seals whether anyone observes FrameRendered. Both ordinary
//! builds and diagnostic builds run identical row, overlay and upload logic.
//! Only an observed build records boundaries, counts and transfer payload sizes.
//! Acquiring or skipping a drawable never emits a successful presentation fact.
use super::{BuildPhase, FrameBuilder, context::TextContext};
use crate::{frame_snapshot::FrameSnapshot, messages::FrameStats};
use puzzle_frame::{RenderFrame, Upload};
use web_time::Instant;

/// Statistics follow successful builds and are consumed only by actual presenters.
impl FrameBuilder {
    /// Builds with optional coarse timings, retaining the existing diagnostic observer.
    pub fn build_snapshot_observed(
        &mut self,
        context: &mut TextContext,
        snapshot: &FrameSnapshot,
        frame: &mut RenderFrame,
        mut observe: impl FnMut(BuildPhase),
    ) -> Result<(), String> {
        // A new build replaces any unpresented observation; it must never be reported later.
        self.statistics = None;
        if !snapshot.observe_frame {
            // No clocks or counter scans are performed on the ordinary frame path.
            return self.build_snapshot_inner(context, snapshot, frame, observe);
        }
        // One fixed payload holds all counters, with no event allocation on the builder.
        let mut stats = FrameStats::default();
        // This clock starts after the opt-in branch, before any frame-construction phase.
        let mut previous = Instant::now();
        self.build_snapshot_inner(context, snapshot, frame, |phase| {
            // Durations are adjacent, so summing phases gives the total CPU build wall time.
            let now = Instant::now();
            stats.cpu_ms[phase as usize] = now.duration_since(previous).as_secs_f64() * 1000.0;
            previous = now;
            observe(phase);
        })?;
        // Counts come from the actual completed draw arrays, including cached row instances.
        stats.instances = [frame.rects.len(), frame.slugs.len(), frame.rasters.len()];
        // Capacity rather than current occupancy exposes the configured memory footprint.
        stats.glyph_bytes =
            context.backend.atlas().allocated_bytes() + context.raster.allocated_bytes();
        // Include pending transfers that survived skipped frames, since they submit now.
        stats.upload_bytes = frame.uploads.iter().map(upload_bytes).sum();
        // Retain by value until a presenter confirms successful submission.
        self.statistics = Some(stats);
        Ok(())
    }
    /// A host calls this after submission; unpresented data produces no FrameRendered fact.
    pub fn take_frame_statistics(&mut self) -> Option<FrameStats> {
        self.statistics.take()
    }
}
/// Transfer bytes count texel payloads, including GPU-only coverage copies.
fn upload_bytes(upload: &Upload) -> usize {
    match upload {
        Upload::Curves { texels, .. } => texels.len() * 16,
        Upload::Bands { texels, .. } => texels.len() * 4,
        Upload::Paint { colors, .. } => colors.len() * 16,
        Upload::Mask { pixels, .. }
        | Upload::Color { pixels, .. }
        | Upload::RasterPage { pixels, .. } => pixels.len(),
        Upload::GpuMask { rect, .. } => rect.width as usize * rect.height as usize,
        // Lifecycle markers and device-local page retention carry no upload payload.
        Upload::RasterReset | Upload::RasterClear { .. } | Upload::RasterCapture { .. } => 0,
    }
}
