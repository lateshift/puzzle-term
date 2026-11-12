//! Asynchronous host continuation and publication contracts retained for the web.
//!
//! Complete native sources own no lanes and never enter these policies. The
//! browser retains its measured allowance, fairness and event-driven wake behavior.
use super::FontSet;
use crate::{cluster_source::ClusterGlyph, glyph::GlyphPlacement};

/// Browser hosts control both policy lanes through their shared text context.
/// These observers do not extract a glyph, load a font or reserve a draw slot.
/// Readiness retains the producer result until actual publication transfers it.
/// Pending work alone cannot keep an otherwise idle terminal drawing frames.
/// Complete native sources leave both lanes empty and use direct preparation.
/// Their glyph costs therefore never participate in this optional-work budget.
/// The same methods remain available to tests of browser continuation fairness.
/// Family policy replacement happens independently in the source registry.
impl FontSet {
    /// Starts a shared time allowance after boundary publication, before any row is built.
    pub fn begin_source_frame(&mut self, allowance: std::time::Duration) {
        if let Some(preparation) = self.preparation.as_mut() {
            // Loaded and platform faces share this same absolute wall-clock turn.
            preparation.driver.begin_frame(allowance);
        }
        // Both source policies use the same wall-clock turn, rather than glyph counts.
        // The host has already charged page eviction and completion publication. Each
        // source converts the remaining duration to an absolute deadline, so work
        // in earlier rows consumes the allowance of later rows and other policies.
        if let Some(source) = self.sources.emoji.as_mut() {
            source.begin_frame(allowance);
        }
        if let Some(source) = self.sources.platform.as_mut() {
            source.begin_frame(allowance);
        }
    }

    /// Sums admitted source preparation; queued and completed worker jobs are excluded.
    pub fn source_preparation_time(&self) -> std::time::Duration {
        // Each source measures only its own admitted work. Reading these durations
        // needs no font-owner lock and cannot count concurrent worker preparation.
        self.preparation
            .as_ref()
            .map_or(std::time::Duration::ZERO, |p| p.driver.frame_time())
            + self
                .sources
                .emoji
                .as_ref()
                .map_or(std::time::Duration::ZERO, |s| s.frame_preparation_time())
            + self
                .sources
                .platform
                .as_ref()
                .map_or(std::time::Duration::ZERO, |s| s.frame_preparation_time())
    }

    /// Publishes one completion unit; the caller owns the elapsed-time budget.
    pub fn poll_source(&mut self) -> bool {
        // Fairness follows successful publication turns rather than glyph weights.
        // A slow emoji decode on the producer cannot hold up ready text outlines.
        // Only the host's elapsed-time allowance determines the number of turns.
        self.sources.next_platform = !self.sources.next_platform;
        // Skip an empty policy immediately, stopping after one real completion
        // so fixed multi-glyph batches cannot overrun the frame allowance.
        if self.sources.next_platform {
            self.sources
                .platform
                .as_mut()
                .is_some_and(|s| s.poll_changed())
                || self
                    .sources
                    .emoji
                    .as_mut()
                    .is_some_and(|s| s.poll_changed())
        } else {
            self.sources
                .emoji
                .as_mut()
                .is_some_and(|s| s.poll_changed())
                || self
                    .sources
                    .platform
                    .as_mut()
                    .is_some_and(|s| s.poll_changed())
        }
    }
    /// Only ready completions justify an immediate continuation; pending jobs do not.
    pub fn sources_ready(&mut self) -> bool {
        // A readiness peek owns its result until publication admits the unit.
        // It performs neither font preparation nor a wait for pending jobs.
        // Short-circuiting is safe: any ready policy justifies the next frame,
        // and poll_source alternates which policy gets the first publication turn.
        self.preparation.as_mut().is_some_and(|p| p.driver.ready())
            || self
                .sources
                .emoji
                .as_mut()
                .is_some_and(|source| source.ready())
            || self
                .sources
                .platform
                .as_mut()
                .is_some_and(|source| source.ready())
    }
    /// Moves queued source ink to the host cache without consulting visible rows.
    /// Each source keeps its existing bounded completion allowance; warm calls allocate nothing.
    pub fn take_source_ready(&mut self) -> Option<(GlyphPlacement, ClusterGlyph)> {
        // Ownership transfers independently of whether the original row remains visible.
        self.sources
            .emoji
            .as_mut()
            .and_then(|s| s.take_ready())
            .or_else(|| self.sources.platform.as_mut().and_then(|s| s.take_ready()))
    }

    /// Observes accepted fallback jobs for non-frame evidence tools; never blocks.
    pub fn sources_pending(&self) -> bool {
        // Loaded and platform queues both count; neither observer waits for font work.
        self.preparation
            .as_ref()
            .is_some_and(|p| p.driver.pending())
            || self.sources.emoji.as_ref().is_some_and(|s| s.pending())
            || self.sources.platform.as_ref().is_some_and(|s| s.pending())
    }

    /// Publishes completed fallback choices before row-cache generation is sampled.
    pub fn poll_sources(&mut self) -> bool {
        // Poll both owners even if the first reports a change. No platform wait is allowed.
        let emoji = self
            .sources
            .emoji
            .as_mut()
            .is_some_and(|s| s.poll_changed());
        let platform = self
            .sources
            .platform
            .as_mut()
            .is_some_and(|s| s.poll_changed());
        emoji || platform
    }
}
