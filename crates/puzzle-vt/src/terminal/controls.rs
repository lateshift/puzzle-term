//! C0 movements and GL designation controls from xterm ctlseqs (§6.5).
//!
//! These operations clear pending wrap because they move the cursor without
//! printing the character that would trigger DECAWM's delayed transition.
//! Tabs use the default stop every eight columns until configurable stops
//! arrive with the editing and mode task M5.3.
//! ENQ deliberately produces no answerback bytes, matching the configured
//! terminal identity. BEL is stored as a fact so host policy can decide
//! whether to flash, sound or suppress it without putting a bus here.
//! SO and SI select GL immediately; SS2/SS3 change only the next graphic.
//! Every C0 byte is interpreted after vte has handled escape syntax, so
//! ENQ cannot accidentally write printable text. Terminal events are facts
//! for Session, not immediate host callbacks: the pure VT crate has no
//! sound device or policy knowledge. The tab vector is per terminal; HTS
//! and TBC modify it, while a reset restores eight-column defaults.
//! Backspace stops at the active left margin except with reverse wrap and
//! a preceding row explicitly marked as soft-wrapped. This avoids jumping
//! backward across a hard line break, which would alter shell prompts.
use super::{Charset, Terminal};

impl Terminal {
    /// Executes a C0 control delivered by vte's DEC ANSI state machine.
    pub(crate) fn execute_c0(&mut self, byte: u8) {
        // xterm ctlseqs treats VT and FF as LF. Unknown C0 bytes are
        // consumed but do not mutate state or reveal an answerback string.
        match byte {
            // xterm ctlseqs: ENQ has no answerback configured for Puzzle.
            0x05 => {}
            // BEL is a fact; a host policy later decides audible output.
            0x07 => self.events.push(super::TerminalEvent::Bell {
                line: self.cursor_line(),
            }),
            0x08 => self.backspace(),
            0x09 => self.tab(),
            // VT and FF share LF's index semantics in xterm ctlseqs.
            0x0a..=0x0c => {
                self.line_feed();
                if self.lnm {
                    self.carriage_return();
                }
            }
            0x0d => self.carriage_return(),
            0x0e => self.gl = 1,
            0x0f => self.gl = 0,
            _ => {}
        }
    }

    /// Moves left without entering a column before the left margin.
    pub(crate) fn backspace(&mut self) {
        // DECLRMM changes where horizontal cursor travel stops; the row
        // itself is still physically wider than that region.
        let left = if self.lrmm_enabled {
            self.margins.left
        } else {
            0
        };
        self.cursor.pending_wrap = false;
        // Reverse wrap is permitted only if the previous row is the same
        // logical line. A hard newline must remain a cursor boundary.
        if self.cursor.column > left {
            self.cursor.column -= 1;
        } else if self.modes.get(true, 45) == Some(true)
            && self.cursor.row > 0
            && self
                .screen()
                .row(self.cursor.row - 1)
                .is_some_and(|row| row.flags.contains(crate::RowFlags::WRAPPED))
        {
            // Reverse-wrap crosses only a real soft continuation (xterm).
            self.cursor.row -= 1;
            self.cursor.column = self.margins.right;
        }
    }

    /// Advances to the next default stop, or the final column.
    pub(crate) fn tab(&mut self) {
        let (columns, _) = self.size();
        // HT moves within the row and cannot trigger delayed wrap.
        self.cursor.pending_wrap = false;
        let right = if self.lrmm_enabled {
            self.margins.right
        } else {
            columns - 1
        };
        // No stop beyond the right margin is visible to this control.
        // If none remains, HT lands at that margin rather than wrapping.
        self.cursor.column = (self.cursor.column + 1..=right)
            .find(|&column| self.tabs[column])
            .unwrap_or(right);
    }

    /// Returns to the left margin, cancelling a delayed wrap.
    pub(crate) fn carriage_return(&mut self) {
        // CR is not the same as CUP(1,1): it retains the current row while
        // respecting the active horizontal origin.
        self.cursor.pending_wrap = false;
        self.cursor.column = if self.lrmm_enabled {
            self.margins.left
        } else {
            0
        };
    }

    /// Walks explicit tab stops, clamping at the active horizontal edge.
    pub(crate) fn tab_many(&mut self, count: usize, forward: bool) {
        // CHT/CBT share the mutable stop vector with HT. The count is
        // bounded by width because further steps cannot move at an edge.
        let left = if self.lrmm_enabled {
            self.margins.left
        } else {
            0
        };
        let right = if self.lrmm_enabled {
            self.margins.right
        } else {
            self.size().0 - 1
        };
        for _ in 0..count.min(self.size().0) {
            // Search existing stops in movement order so TBC-cleared
            // positions are skipped without reconstructing default stops.
            self.cursor.column = if forward {
                (self.cursor.column + 1..=right)
                    .find(|&column| self.tabs[column])
                    .unwrap_or(right)
            } else {
                (left..self.cursor.column)
                    .rev()
                    .find(|&column| self.tabs[column])
                    .unwrap_or(left)
            };
        }
        // Any tab move is explicit, even when it hits the same right edge.
        self.cursor.pending_wrap = false;
    }

    /// Designates one of G0–G3 using xterm ctlseqs' SCS finals.
    pub(crate) fn designate(&mut self, set: usize, final_byte: u8) {
        // The byte after ESC (, ), * or + selects a stored G-set. It does
        // not immediately change GL until SI/SO or a locking shift.
        // Ignore unsupported SCS finals without changing the active set.
        if set >= self.charsets.len() {
            return;
        }
        self.charsets[set] = match final_byte {
            b'0' => Charset::DecSpecial,
            b'A' => Charset::Uk,
            b'B' => Charset::Ascii,
            _ => return,
        };
    }

    /// Invokes one G-set for exactly the next printable character.
    pub(crate) fn single_shift(&mut self, set: usize) {
        // SS2/SS3 replace only one future graphic invocation, not the
        // underlying GL choice used by later printable characters.
        if set < self.charsets.len() {
            self.single_shift = Some(set);
        }
    }

    /// Locks GL to a designated set until another shift arrives.
    pub(crate) fn locking_shift(&mut self, set: usize) {
        // LS2/LS3 remain active until another locking shift or SI/SO.
        if set < self.charsets.len() {
            self.gl = set;
        }
    }
}
