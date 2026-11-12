//! Public selection reads and source-driven reconciliation (§6.9).
//!
//! Queries borrow cached endpoints. Mutations go through Select and the bus;
//! output/resize reconcile VT versions before a snapshot can observe the model.
//! Changed ranges wake presentation once and refresh the scrollbar's marker.
use super::Session;
use crate::{messages::Event, selection::SelectionRange};
/// Keeps public reads cheap and handles invalidation only at document mutations.
impl Session {
    /// Hosts use this live policy while coalescing local versus reported motion.
    pub fn mouse_shift_selects(&self) -> bool {
        // Hosts ask about local selection; capture resolves the opposite ownership.
        !self
            .mouse_shift_capture
            .captures(self.terminal.mouse_shift_capture)
    }
    /// Returns the current expanded inclusive cell range without copying text.
    pub fn selection_range(&self) -> Option<SelectionRange> {
        self.selection.range()
    }
    /// Extracts whole grapheme clusters according to the live trim setting.
    pub fn selection_text(&self) -> String {
        self.selection.text(&self.terminal)
    }
    /// Reconciles tracked identities after parser output, reset or resize.
    pub(super) fn reconcile_selection(&mut self) {
        self.reconcile_decorations();
        let changed = self.selection.reconcile(&mut self.terminal);
        let shift_selects = self.mouse_shift_selects();
        if let Some(at) = self.pointer.reconcile(
            &mut self.terminal,
            self.selection.range().is_some(),
            shift_selects,
        ) {
            // Source cancellation ends edge speed but still settles fractional text.
            self.viewport.finish_drag(&mut self.terminal, at);
        }
        if changed {
            // Clearing is a state edge just like an explicit user selection change.
            self.bus
                .publish(Event::SelectionChanged(self.selection.range()));
            self.needs_frame = true;
            self.services.waker.request_frame();
            self.refresh_scrollbar();
        }
    }
}

/// Stable shell identities resolve in Rust before a web caller submits selection policy.
impl Session {
    /// Finds the retained output boundary belonging to one OSC 133 command.
    /// Unknown or evicted commands return None without selecting a different output.
    pub fn zone_output_point(&self, id: u64) -> Option<crate::vt::selection::SelectPoint> {
        // Zone IDs name commands across tracked reflow, rather than list indices.
        // Resolving the Output mark leaves expansion and interception with Select.
        self.buffer()
            .zones()
            .find(|m| m.id == id && m.kind == crate::vt::zones::ZoneKind::Output)
            .map(|m| crate::vt::selection::SelectPoint {
                line: m.line,
                column: m.column,
            })
    }
}
