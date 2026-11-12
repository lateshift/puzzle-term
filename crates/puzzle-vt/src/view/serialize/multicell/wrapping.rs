//! A wide print at the final column sets WRAPPED without touching that cell.
//! Unlike a temporary ASCII X, this also works when the cell is a reservation
//! or when wrapping immediately scrolls the old row into inaccessible history.
//!
//! CUP cannot write a row's delayed-wrap history flag. The wide probe triggers
//! the same parser transition without replacing the final cell with a temporary
//! ASCII character. Its ink is erased before reconstructing the destination row.
//! Preparing wraps before reservations avoids the lower-fragment skip rule.
use super::*;

/// Wrap preparation runs before installing future rectangular reservations.
impl Writer<'_> {
    /// Marks future rows while the complete new rectangle can still fit.
    pub(super) fn prepare_wraps(&mut self, first: usize, last: usize) {
        for row in first..=last {
            if row + 1 < self.base() + self.mirror.size().1 {
                self.finish_wrap(row);
            }
        }
    }
    /// Preserves source physical wrap flags independently of logical band continuity.
    pub(super) fn finish_wrap(&mut self, row: usize) {
        let Some(index) = row.checked_sub(self.prefix) else {
            return;
        };
        if !self.lines.get(index).is_some_and(|line| line.wrapped()) || self.prepared[row] {
            return;
        }
        let y = row.saturating_sub(self.base());
        let width = self.mirror.size().0;
        // A CJK scalar has width two in both width modes. At the rightmost
        // column its fit rule wraps before any overwrite/skip operation.
        self.push(&format!("\x1b[{};{width}H\x1b[0m界", y + 1));
        let cursor = self.mirror.cursor;
        let column = if cursor.pending_wrap {
            width - 2
        } else {
            cursor.column.saturating_sub(2)
        };
        // Erase the probe at its actual destination: lower reservations can
        // redirect printing, so assuming column zero would corrupt a block.
        self.push(&format!(
            "\x1b[{};{}H\x1b[0m\x1b[2X",
            cursor.row + 1,
            column + 1
        ));
        self.prepared[row] = true;
    }
}
