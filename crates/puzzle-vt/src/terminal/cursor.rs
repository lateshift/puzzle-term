//! Cursor movement and save state in xterm grid coordinates (§6.5).
//!
//! DEC origin mode makes CUP relative to the scrolling rectangle, whereas
//! VPA and CHA remain absolute without it. Every explicit movement cancels
//! delayed wrap; only the next printable may perform that transition.
//! The cursor is stored in physical, zero-based grid coordinates. DECOM
//! changes the interpretation of incoming one-based CUP parameters rather
//! than changing the stored coordinate system. That makes screen switches
//! and scrollback identity independent of a temporary DEC region.
//! Each screen has its own DECSC slot: a full-screen program can restore
//! its own state without overwriting the shell's saved primary position.

use super::{ActiveScreen, SavedCursor, Terminal};

impl Terminal {
    /// Sets a top-origin position, clamped to the active margins.
    pub(crate) fn move_to(&mut self, row: usize, column: usize) {
        let (columns, rows) = self.size();
        // Origin mode narrows the legal addressing rectangle. Without it,
        // the physical grid bounds all absolute and relative movement.
        let origin = self.modes.get(true, 6) == Some(true);
        let (top, bottom) = if origin {
            (self.margins.top, self.margins.bottom)
        } else {
            (0, rows - 1)
        };
        let (left, right) = if origin && self.modes.get(true, 69) == Some(true) {
            (self.margins.left, self.margins.right)
        } else {
            (0, columns - 1)
        };
        // Clamping before storage keeps the public cursor invariant true
        // even for CSI parameters that saturate vte's numeric type.
        self.cursor.row = row.clamp(top, bottom);
        self.cursor.column = column.clamp(left, right);
        self.cursor.pending_wrap = false;
    }

    /// Moves to a one-based CUP coordinate, applying DECOM's origin.
    pub(crate) fn cup(&mut self, row: usize, column: usize) {
        // CUP/HVP parameters are one-based; an omitted or zero parameter
        // denotes the first row or column of the current origin rectangle.
        let origin = self.modes.get(true, 6) == Some(true);
        let base_row = if origin { self.margins.top } else { 0 };
        let base_col = if origin && self.modes.get(true, 69) == Some(true) {
            self.margins.left
        } else {
            0
        };
        // Saturating arithmetic turns hostile large coordinates into an
        // edge position without overflowing before `move_to` clamps them.
        self.move_to(
            base_row.saturating_add(row.saturating_sub(1)),
            base_col.saturating_add(column.saturating_sub(1)),
        );
    }

    /// Moves by signed row and column deltas without crossing active edges.
    pub(crate) fn move_relative(&mut self, rows: i32, columns: i32) {
        let (width, height) = self.size();
        // Signed deltas arise from CUU/CUB as well as CUD/CUF. Subtraction
        // saturates at zero before the region-aware final clamp.
        let clamp_add = |current: usize, delta: i32| -> usize {
            if delta >= 0 {
                current.saturating_add(delta as usize)
            } else {
                current.saturating_sub(delta.unsigned_abs() as usize)
            }
        };
        let row = clamp_add(self.cursor.row, rows).min(height - 1);
        // Margin clamping happens in one place, so cursor commands cannot
        // disagree about DECOM's top or bottom boundary.
        let column = clamp_add(self.cursor.column, columns).min(width - 1);
        self.move_to(row, column);
    }

    /// Saves DECSC graphic state separately for primary and alternate grids.
    pub(crate) fn save_cursor(&mut self) {
        // DECSC also saves the pen, G-set designation and origin mode;
        // saving only the cursor coordinates breaks applications after ESC 8.
        let index = usize::from(self.active == ActiveScreen::Alternate);
        self.saved[index] = Some(SavedCursor {
            cursor: self.cursor,
            pen: self.pen,
            charsets: self.charsets,
            gl: self.gl,
            origin: self.modes.get(true, 6) == Some(true),
        });
    }

    /// Restores a saved DECSC state, or homes when none was saved.
    pub(crate) fn restore_cursor(&mut self) {
        let index = usize::from(self.active == ActiveScreen::Alternate);
        // Restore DECOM before clamping the saved coordinate, because that
        // mode determines the addressing rectangle at the destination.
        if let Some(saved) = self.saved[index] {
            self.pen = saved.pen;
            self.charsets = saved.charsets;
            self.gl = saved.gl;
            self.modes.set(true, 6, saved.origin);
            self.move_to(saved.cursor.row, saved.cursor.column);
            self.cursor.pending_wrap = saved.cursor.pending_wrap;
        } else {
            // A newly created screen has no DECSC snapshot; home is a
            // deterministic, in-bounds fallback for an isolated ESC 8.
            self.cup(1, 1);
        }
    }

    /// Switches the visible grid without copying either screen's rows.
    pub(crate) fn switch_screen(&mut self, alternate: bool, clear: bool) {
        // An alternate grid never retains scrollback. Switching the active
        // borrow leaves primary rows and their line IDs untouched.
        let next = if alternate {
            ActiveScreen::Alternate
        } else {
            ActiveScreen::Primary
        };
        if self.active == next && !clear {
            return;
        }
        // Mode 1049 clears the alternate grid on entry; mode 47 simply
        // selects it. Both require a full redraw at the rendering boundary.
        self.clear_source_watches();
        self.active = next;
        self.tracked.set_screen(next);
        // Numeric LineIds overlap across screens; invalidate host anchors.
        self.screen_epoch = self.screen_epoch.wrapping_add(1);
        self.cursor = super::Cursor::default();
        if clear {
            self.erase_display(2, false);
        }
        self.screen_mut().damage.full = true;
    }
}
