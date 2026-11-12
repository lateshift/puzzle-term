//! Grid change summary for a future damage-driven renderer (§6.1, P4).
//!
//! A fixed bitset tracks changed rows without allocating per print. Scroll
//! movement is a separate hint because recycled row contents need redraw.
//! The bitset is indexed by visible row, while line identities belong to
//! scrollback; moving a row changes its screen coordinate but not its ID.

/// Pending visual changes since the last frame was consumed.
#[derive(Clone, Debug)]
pub struct Damage {
    /// Dirty rows packed into one bit per visible row.
    dirty: Vec<u64>,
    /// Positive movement means content moved upward by this many rows.
    pub scrolled: i32,
    /// Forces a redraw after a full-screen state change.
    pub full: bool,
}

impl Damage {
    /// Reserves the bitset once for a particular grid height.
    #[must_use]
    pub fn new(rows: usize) -> Self {
        Self {
            dirty: vec![0; rows.div_ceil(64)],
            scrolled: 0,
            full: true,
        }
    }

    /// Marks one visible row after a cell or row-level change.
    pub fn mark(&mut self, row: usize) {
        // A pending full redraw already includes every row. Large output
        // batches before the next frame should not update a redundant
        // bitset for each printable character (P4, §6.1).
        if self.full {
            return;
        }
        if let Some(word) = self.dirty.get_mut(row / 64) {
            *word |= 1_u64 << (row % 64);
        }
    }

    /// Checks a row without exposing the bitset representation.
    #[must_use]
    pub fn is_dirty(&self, row: usize) -> bool {
        self.dirty
            .get(row / 64)
            .is_some_and(|word| word & (1_u64 << (row % 64)) != 0)
    }

    /// Records one full-width upward scroll for instance reuse.
    pub fn scroll_up(&mut self) {
        self.scrolled = self.scrolled.saturating_add(1);
        self.mark(0);
    }

    /// Clears frame-local changes after the renderer consumes them.
    pub fn clear(&mut self) {
        self.dirty.fill(0);
        self.scrolled = 0;
        self.full = false;
    }
}
