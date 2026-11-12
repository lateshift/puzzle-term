//! Incremental search at display boundaries, independent of input drains.
//!
//! Starting or changing a query wakes the host. At most one engine slice runs
//! per direct/external frame turn; even headless Session frames drive progress.
//! Completed queries stop requesting refreshes. Output marks the source dirty
//! for the next slice, keeping search work out of PTY and keyboard callbacks.
//!
//! Initial selection excludes matches behind host chrome before choosing a target.
//! The owner reads only copied font metrics and logical host rectangles there.
//! Deferred navigation uses those same metrics, before the snapshot seals its rows.
//! A host rectangle never enters the VT matcher or changes the text being searched.
use super::Session;
use crate::search::SearchState;
impl Session {
    /// Returns the count and current zero-based index after the latest slice.
    pub fn search_state(&self) -> SearchState {
        self.search.state()
    }
    /// Copies no text and exposes only reconciled ranges for host diagnostics.
    pub fn search_fills(&self) -> &[crate::search::SearchFill] {
        &self.search.fills
    }
    /// Advances exactly once before either platform captures its visible rows.
    pub(super) fn advance_search(&mut self, now: web_time::Instant) {
        if self.search.advance(&mut self.terminal) {
            // Establish the same baseline whether Enter arrives now or after a slice.
            let initial =
                self.search
                    .initial_navigation(&self.terminal, &self.viewport, |row, hit| {
                        super::occlusion::visible_match(
                            &self.terminal,
                            row,
                            hit,
                            self.frame_policy.as_ref(),
                            self.model_overlays.occluded_rects.as_deref().unwrap_or(&[]),
                        )
                    });
            let pending = self.search.pending_navigation(&self.terminal);
            if let Some(line) = pending.or(initial) {
                // Complete accepted early navigation through the existing reveal/
                // centre path, before this turn seals viewport and strong fills.
                self.model_overlays.query.dirty = true;
                self.scroll_to(crate::viewport::ScrollTarget::Line(line));
                self.pump(now);
            }
            self.needs_frame = true;
            self.refresh_scrollbar();
        }
    }
    /// An unfinished bounded query is animation-like work at the next refresh.
    pub(super) fn schedule_search(&mut self) {
        if self.search.working() {
            self.needs_frame = true;
            self.services.waker.request_frame();
        }
    }
}
