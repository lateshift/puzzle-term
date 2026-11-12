//! A visible grid backed by the same row type on both screens (§6.1).
//!
//! The primary screen receives a bounded history ring; the alternate screen
//! is constructed with zero history. Keeping their row APIs equal lets the
//! parser change screens later without reimplementing cell access.
//! Damage belongs to each screen because an inactive grid can still carry
//! changes that must be repainted when a DEC screen switch makes it visible.
//! The screen borrows its Terminal write clock for recycling; it never owns a
//! separate sequence that could alias versions on the other screen.
use crate::{
    cell::PackedColor, damage::Damage, ring::RowRing, row::Row, tracked::TrackedPositions,
};

/// Visible rows, optional history and their pending damage.
#[derive(Clone, Debug)]
pub struct Screen {
    /// Recycled rows in oldest-to-newest order.
    pub ring: RowRing,
    /// Dirty-row bitset and full-scroll hint for rendering.
    pub damage: Damage,
}

impl Screen {
    /// Creates a screen with its own scrollback policy.
    #[must_use]
    pub fn new(
        columns: usize,
        rows: usize,
        scrollback_lines: usize,
        scrollback_bytes: usize,
    ) -> Self {
        let ring = RowRing::new(columns, rows, scrollback_lines, scrollback_bytes);
        let damage = Damage::new(ring.visible_rows());
        Self { ring, damage }
    }

    /// Returns one visible row for rendering or tests.
    #[must_use]
    pub fn row(&self, index: usize) -> Option<&Row> {
        self.ring.visible(index)
    }

    /// Returns one visible row for in-place grid updates.
    pub fn row_mut(&mut self, index: usize) -> Option<&mut Row> {
        self.ring.visible_mut(index)
    }

    /// Scrolls a full-width grid upward, preserving history if allowed.
    pub fn scroll_up(
        &mut self,
        tracked: &mut TrackedPositions,
        background: PackedColor,
        clock: &mut crate::WriteClock,
    ) {
        self.ring.scroll_up(tracked, background, clock);
        self.damage.scroll_up();
    }
}
