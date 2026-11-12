//! Direct host screen actions independent of an in-flight PTY parser.
//!
//! These are user requests, not bytes inserted into child output. Keeping
//! the parser intact matters when a shortcut lands between CSI parameters.
use super::Terminal;

/// Local prompt preservation must reinstall whole rectangular records.
mod multicell;

/// Local screen actions preserve the parser's partially received sequence.
impl Terminal {
    /// Performs the same state reset as RIS without injecting ESC bytes.
    pub fn reset(&mut self) {
        self.hard_reset();
    }
    /// Clears primary scrollback without changing the visible screen.
    pub fn clear_history(&mut self) {
        self.source_cleared();
        self.primary.ring.clear_scrollback(&mut self.tracked);
    }
    /// Clears visible output and history while retaining the current prompt.
    pub fn clear_preserving_line(&mut self) {
        if self.clear_preserving_multicell() {
            return;
        }
        let column = self.cursor.column;
        // Capture cells and extras together: a prompt may contain styled
        // glyphs, clusters or OSC 8 links, not merely an ASCII string.
        let saved = self.screen().row(self.cursor.row).cloned();
        // Erasure uses BCE, while restoring the prompt keeps its original
        // colors. This is a local UI clear, not output the child has emitted.
        self.erase_display(2, false);
        self.clear_history();
        let (screen, clock) = self.screen_clock_mut();
        if let Some(saved) = saved
            && let Some(row) = screen.row_mut(0)
        {
            row.cells.clone_from(&saved.cells);
            row.extras = saved.extras;
            row.flags = saved.flags;
            row.version = clock.written::<true>(row, 0..row.cells.len() + 1);
        }
        // Move the retained prompt to the top so subsequent child output
        // follows it rather than the now-erased original cursor row.
        self.cursor.row = 0;
        self.cursor.column = column;
        self.cursor.pending_wrap = false;
        self.screen_mut().damage.full = true;
    }
}
