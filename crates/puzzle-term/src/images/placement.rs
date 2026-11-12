//! Shared image visibility model, independent of the placement wire format.
use super::Store;
use crate::vt::{Terminal, terminal::ActiveScreen, tracked::PositionState};

/// Visibility is evaluated only for retained image sources.
impl Store {
    /// Visibility is based on retained row position, never numeric LineId arithmetic.
    pub(crate) fn visible(&self, generation: u64, terminal: &Terminal) -> bool {
        // An image is visible if any of its placements intersects the displayed
        // row interval. Being retained in scrollback alone does not pin its bytes.
        self.placements.iter().any(|placement| {
            if placement.image != generation {
                return false;
            }
            if placement.virtual_ {
                return super::placeholders::visible(self, placement, terminal);
            }
            if placement.screen != terminal.active {
                return false;
            }
            // Gone anchors are never visible, even while reconciliation is
            // waiting for the end of the currently parsed application slice.
            let PositionState::Present { line, .. } = terminal.tracked.get(placement.anchor) else {
                return false;
            };
            // Numeric LineIds need not be contiguous after region scrolls.
            // Ring indices represent the actual current displayed ordering.
            let ring = &terminal.screen().ring;
            let (top, end) = self.visible_rows(terminal);
            // Font/DPR changes can alter natural and single-axis image heights.
            // Eviction must use the same current geometry as sealed frame visibility.
            let cell = [
                u32::from(terminal.cell_pixels.0).max(1),
                u32::from(terminal.cell_pixels.1).max(1),
            ];
            let size = super::geometry::display_size(
                [placement.source[2], placement.source[3]],
                placement.requested_cells,
                cell,
            );
            let rows =
                ((size[1] + f64::from(placement.offset[1])) / f64::from(cell[1])).ceil() as usize;
            ring.index_of(line)
                .is_some_and(|index| index < end && index.saturating_add(rows) > top)
        })
    }

    /// Semantic resets and evictions retire only the affected source ownership.
    pub fn reconcile(&mut self, terminal: &mut Terminal) {
        // RIS invalidates the complete protocol, including incomplete transfers.
        // An already submitted native job becomes harmless through ticket matching.
        if self.reset != terminal.reset_epoch {
            while !self.images.is_empty() {
                self.remove_image(self.images.len() - 1, terminal);
            }
            // Cancel both an accumulated transfer and a queued-but-unsubmitted
            // job. A submitted job may finish later but cannot match waiting.
            self.transfer = None;
            self.waiting = None;
            self.request = None;
            self.reset = terminal.reset_epoch;
        }
        // ED 2 preserves text row identities, so anchor liveness alone cannot
        // observe it. Independent screen epochs retire just that screen's images.
        let epochs = terminal.image_clear_epochs();
        self.placements.retain(|placement| {
            // Erasure applies to the placement's screen, not whichever screen
            // happens to be active when this reconciliation turn is called.
            let screen = usize::from(placement.screen == ActiveScreen::Alternate);
            // A prototype survives anchor eviction because its referencing text may be elsewhere.
            let keep = placement.virtual_
                || self.clear[screen] == epochs[screen]
                    && terminal.tracked.get(placement.anchor) != PositionState::Gone;
            // Registry slots are reusable only after their generation advances.
            // Releasing dead anchors prevents repeated clears from growing metadata.
            if !keep {
                terminal.tracked.release(placement.anchor);
            }
            keep
        });
        // Remember epochs only after the whole placement table has been tested;
        // updating early would hide the erase from later entries in this pass.
        self.clear = epochs;
    }
}
