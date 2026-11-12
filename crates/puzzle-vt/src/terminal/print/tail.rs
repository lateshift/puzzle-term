//! A cell identity witness for incremental grapheme suffix reuse.
//! Parser control functions may move to arbitrary existing text. Line identity,
//! row version and screen epoch validate the suffix without instrumenting every
//! cursor/edit function or putting Unicode state into the sixteen-byte cell.
use super::boundary::State;
use crate::{Row, tracked::LineId};

/// Fixed-size context for the most recent successfully joined grid cell.
#[derive(Clone, Copy)]
pub(in crate::terminal) struct Tail {
    /// Every screen transition/reset distinguishes otherwise identical rows.
    pub(super) epoch: u64,
    /// Recycled scrollback storage gets a new absolute line identity.
    pub(super) line: LineId,
    /// Only this leading cell owns the context, never its wide spacer.
    pub(super) column: usize,
    /// Any edit invalidates the witness, including writes through the public grid.
    pub(super) version: u64,
    /// Incrementally advanced UAX #29 suffixes, without owned cluster text.
    pub(super) state: State,
}

impl Tail {
    /// Checks that suffix state still describes the authoritative previous cell.
    pub(super) fn matches(self, epoch: u64, row: &Row, column: usize) -> bool {
        self.epoch == epoch
            && self.line == row.id
            && self.column == column
            && self.version == row.version
    }
}
