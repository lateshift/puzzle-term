//! Session-facing buffer operations implemented by the pure VT owner (§6.13).
//!
//! Creating a view only borrows the terminal. A marker registers one stable
//! identity with the shared tracker, while `seal_frame` turns pending row
//! damage into one bounded journal entry. The future Session chooses when
//! a frame ends; this crate does not own a clock or renderer (§4.1).
use super::{ActiveScreen, Terminal};
use crate::tracked::{LineId, TrackedPos};

impl Terminal {
    /// Borrows the complete active buffer without copying any rows (§6.13).
    #[must_use]
    pub fn buffer(&self) -> crate::view::BufferView<'_> {
        crate::view::BufferView::new(self)
    }

    /// Registers a host marker only if its line and column are retained.
    ///
    /// M6's Session exposes this publicly; the VT tracker remaps the
    /// handle when its logical text scrolls or reflows.
    pub fn add_marker(&mut self, line: LineId, column: usize) -> Option<TrackedPos> {
        // Checking both dimensions prevents an API coordinate from
        // becoming a marker for a nonexistent cell or evicted line.
        if column >= self.size().0 || self.buffer().line(line).is_none() {
            return None;
        }
        Some(self.tracked.track(line, column))
    }

    /// Releases a host marker after its overlay or API handle is closed.
    pub fn remove_marker(&mut self, marker: TrackedPos) {
        self.tracked.release(marker);
    }

    /// Captures a trusted owner-only source marker, excluded from publications.
    pub fn add_private_marker(
        &mut self,
        line: LineId,
        column: usize,
    ) -> Option<crate::PrivateTrackedPos> {
        // Validate retention before allocating the lazy owner-only arena. A
        // missing row cannot create a source receipt for unrelated future text.
        if column >= self.size().0 || self.buffer().line(line).is_none() {
            return None;
        }
        Some(self.tracked.track_private(line, column))
    }

    /// Retires one source-owner handle without affecting public geometry markers.
    pub fn remove_private_marker(&mut self, marker: crate::PrivateTrackedPos) {
        self.tracked.release_private(marker);
    }

    /// Reports actual owner-only tracked capacity for optional state admission.
    pub fn private_marker_storage_bytes(&self) -> usize {
        self.tracked.private_storage_bytes()
    }

    /// Seals current damage into one observer entry at the frame boundary.
    ///
    /// M6's Session will call this after consuming render damage. The
    /// journal stores line identities and movement only, never pixels.
    pub fn seal_frame(&mut self) {
        // Each screen owns separate damage. Sealing only the active one
        // prevents an alternate editor from clearing primary changes.
        match self.active {
            ActiveScreen::Primary => {
                self.change_log.record(self.active, &self.primary);
                self.primary.damage.clear();
            }
            ActiveScreen::Alternate => {
                self.change_log.record(self.active, &self.alternate);
                self.alternate.damage.clear();
            }
        }
    }
}
