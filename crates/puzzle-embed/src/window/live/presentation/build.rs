//! Row building and optional phase observations operate on a sealed owner publication.
//! The permanent render-side builder retains its row caches through FIFO acquisition.
//! Build failure invalidates the snapshot without presenting partial geometry.
//! Final ink observations read retained handles and never prepare replacement glyphs.
//!
//! A total wall timer cannot distinguish exposed-row shaping from geometry or
//! overlay compilation. The instrumented path therefore records six boundaries
//! inside the same generic build operation; the ordinary empty observer optimizes
//! away. Phase arrays contain only fixed-size scalar durations, with no strings,
//! heap growth or logging during the measured interval. The completed build also
//! reports its actual row misses, so a tween exposing ten rows cannot be confused
//! with §15's one-new-row regression. Timers include scheduling delays and do not
//! claim to measure exclusive CPU execution. Acquisition is recorded separately.
use super::*;

/// Successful row assembly carries captured blink and opt-in phase evidence.
pub struct BuiltRows {
    /// Cursor phase must stay paired with the snapshot used by cold verification.
    pub blink: bool,
    /// Ordinary builds avoid timing calls when diagnostics are disabled.
    #[cfg(feature = "lock-metrics")]
    pub build_cpu: std::time::Duration,
    /// Six opt-in phase durations identify row work versus placement/provider/upload work.
    #[cfg(feature = "lock-metrics")]
    pub build_phases: [std::time::Duration; 6],
    /// Metrics, cluster/fallback, shaping/placement and glyph preparation totals.
    #[cfg(feature = "lock-metrics")]
    pub row_phases: [u64; 4],
    /// Number of exposed/damaged rows distinguishes rebuilds from cache-only animation.
    #[cfg(feature = "lock-metrics")]
    pub rebuilt_rows: usize,
    /// Prepared-key difference measures new geometry rather than parser scalars.
    #[cfg(feature = "lock-metrics")]
    pub glyphs_before: usize,
    /// Upload regions are counted before the renderer acknowledges them.
    #[cfg(feature = "lock-metrics")]
    pub upload_count: usize,
}
impl LiveWindow {
    /// Builds from copied cells without a Session reference or ownership loan.
    pub fn build_rows(&mut self) -> Option<BuiltRows> {
        let _stage = crate::resize_timing::Stage::new("render_build");
        // Capture and build phases borrow only copied cells, never the live VT.
        let (built, work) = self.assemble_rows();
        #[cfg(feature = "lock-metrics")]
        // An unsuccessful build cannot resolve a covered appearance.
        self.observe_ink(built.is_ok());
        // The verifier sees pending bytes before the real writer acknowledges them.
        self.observe_glyph_uploads();
        if let Err(error) = built {
            #[cfg(feature = "lock-metrics")]
            self.frame_metrics.gate("build_failed");
            // Failed assembly requires a complete recapture instead of reusing partial damage.
            self.snapshot.invalidate();
            self.send(SessionMessage::FrameResult(FrameResult::BuildFailed));
            tracing::warn!(event="desktop.frame_build_failed", %error);
            return None;
        }
        // Ownership and all optional phase scalars remain paired with this frame.
        Some(work)
    }

    /// Uses one build path; phase timers exist only in explicitly instrumented binaries.
    fn assemble_rows(&mut self) -> (Result<(), String>, BuiltRows) {
        // Slug preparation and all GPU work now operate on the frozen
        // snapshot while the parser continues changing Session rows.

        // Blink is captured with geometry; a timer firing during GPU work
        // must not make cold verification use a different cursor phase.
        #[cfg(feature = "lock-metrics")]
        let build_started = Instant::now();
        #[cfg(feature = "lock-metrics")]
        let glyphs_before = self.text_context.prepared_glyphs();
        // Only immutable captured VT rows enter shaping and instance assembly here.
        #[cfg(feature = "lock-metrics")]
        let mut build_phases = [std::time::Duration::ZERO; 6];
        #[cfg(feature = "lock-metrics")]
        let mut phase_started = build_started;
        #[cfg(feature = "lock-metrics")]
        self.text_context.measure_rows();
        #[cfg(feature = "lock-metrics")]
        let built = self.builder.build_snapshot_observed(
            &mut self.text_context,
            &self.snapshot,
            &mut self.frame,
            |phase| {
                // Store fixed-size scalars; formatting happens after the recorded interval.
                let now = Instant::now();
                build_phases[phase as usize] = now.duration_since(phase_started);
                phase_started = now;
            },
        );
        #[cfg(not(feature = "lock-metrics"))]
        let built =
            self.builder
                .build_snapshot(&mut self.text_context, &self.snapshot, &mut self.frame);
        #[cfg(feature = "lock-metrics")]
        let rebuilt_rows = self.builder.rebuilt_rows();
        #[cfg(feature = "lock-metrics")]
        // Exclude boundary publication and FIFO acquisition from this build phase.
        let build_cpu = build_started.elapsed();
        #[cfg(feature = "lock-metrics")]
        let upload_count = self.frame.uploads.len();
        // Return scalars without formatting, allocating or retaining any borrowed row.
        (
            built,
            BuiltRows {
                blink: self.builder.blink_phase(),
                #[cfg(feature = "lock-metrics")]
                build_cpu,
                #[cfg(feature = "lock-metrics")]
                build_phases,
                #[cfg(feature = "lock-metrics")]
                rebuilt_rows,
                #[cfg(feature = "lock-metrics")]
                row_phases: self.text_context.row_metrics().unwrap_or_default(),
                #[cfg(feature = "lock-metrics")]
                glyphs_before,
                #[cfg(feature = "lock-metrics")]
                upload_count,
            },
        )
    }
}
