//! Hard and soft terminal reset without replacing an in-flight parser (§6.5).
//!
//! RIS clears both grids and primary history. DECSTR leaves existing text
//! alone but resets the cursor, regions, tabs and rendition state so the
//! application can recover from unknown prior terminal modes.
//! The parser itself is temporarily borrowed out of Terminal::advance.
//! Reconstructing the whole Terminal here would replace that in-flight
//! parser, lose bytes following RIS in the same feed, and allocate a new
//! 64 KiB parser buffer. Resetting owned state in place avoids all three.

use super::{ActiveScreen, Cursor, Margins, Terminal};
use crate::{
    cell::{Cell, PackedColor},
    modes::Modes,
};

impl Terminal {
    /// Restores full-screen margins after DECSTBM, DECLRMM or reset.
    pub(crate) fn reset_margins(&mut self) {
        let (columns, rows) = self.size();
        // A resize and a reset both return scroll coordinates to the full
        // viewport; an old short region must not trap later line feeds.
        self.margins = Margins::full(columns, rows);
        self.cursor = Cursor::default();
    }

    /// Resets mode and cursor state while preserving visible text (DECSTR).
    pub(crate) fn soft_reset(&mut self) {
        // DECSTR is intentionally less destructive than RIS: applications
        // use it to regain known input and graphic modes mid-session.
        self.modes.reset();
        // DECSTR and RIS both end the one-use paste-event authorization.
        self.clipboard_grant_epoch = self.clipboard_grant_epoch.wrapping_add(1);
        // DECSTR restores the user's initial width model, not a hard-coded
        // clustered state; DECRQM then reports that restored live value.
        self.modes.set(true, 2027, self.grapheme_width_unicode);
        // Reset restores the configured wheel policy after application overrides.
        self.modes.set(true, 1007, self.alternate_scroll_default);
        self.autowrap = true;
        self.insert_mode = false;
        self.lrmm_enabled = false;
        self.lnm = false;
        self.pen = Cell::erased(PackedColor::DEFAULT);
        self.cursor_style = 0;
        // ISO-2022 shifts cannot outlive a soft reset, or subsequent ASCII
        // would silently turn into box-drawing glyphs.
        self.charsets = [super::Charset::Ascii; 4];
        self.gl = 0;
        self.single_shift = None;
        self.last_printed = None;
        self.current_link = None;
        self.underline_color = None;
        // Default stops resume at 8-column intervals, independent of any
        // explicit HTS/TBC edits made before the reset.
        self.reset_margins();
        self.tabs
            .iter_mut()
            .enumerate()
            .for_each(|(col, stop)| *stop = col != 0 && col % 8 == 0);
    }

    /// Performs RIS, including both screens, scrollback and saved state.
    pub(crate) fn hard_reset(&mut self) {
        self.clear_source_watches();
        self.note_operation(
            super::operation_metrics::Operation::ResetRows,
            (self.primary.ring.visible_rows()
                + self.primary.ring.scrollback_len()
                + self.alternate.ring.visible_rows()) as u64,
        );
        // RIS selects the main screen before clearing, so future output
        // cannot keep addressing a full-screen program's alternate grid.
        self.active = ActiveScreen::Primary;
        self.tracked.set_screen(self.active);
        // A reset is a new viewport context even if the screen stays primary.
        self.screen_epoch = self.screen_epoch.wrapping_add(1);
        // Optional protocols distinguish RIS from alternate-screen changes and DECSTR.
        self.reset_epoch = self.reset_epoch.wrapping_add(1);
        self.soft_reset();
        self.primary.ring.clear_scrollback(&mut self.tracked);
        // Both screen allocations stay warm; content, sparse extras and
        // cache versions change together to prevent stale glyph output.
        for screen in [&mut self.primary, &mut self.alternate] {
            // RIS clears all references together, so the screen store can be released.
            screen.ring.multicells = None;
            for index in 0..screen.ring.visible_rows() {
                if let Some(row) = screen.row_mut(index) {
                    // Preserve bounded cluster stores as well as the cell allocation.
                    // RIS erases content but leaves each visible identity in place.
                    row.recycle(row.id, PackedColor::DEFAULT, &mut self.write_clock);
                }
            }
            screen.damage.full = true;
        }
        // A saved cursor from before RIS would reintroduce stale modes or
        // point into discarded alternate-screen content.
        self.saved = [None, None];
        // RIS also discards host-facing title, palette and OSC 8 overrides;
        // a newly started shell must not inherit them from the old app.
        self.reset_host_metadata();
        self.modes = Modes::default();
        self.modes.set(true, 2027, self.grapheme_width_unicode);
        // Reset restores the configured wheel policy after application overrides.
        self.modes.set(true, 1007, self.alternate_scroll_default);
        // RIS clears both keyboard stacks; a crashed app cannot leave a
        // later shell receiving enhanced key reports (kitty protocol).
        self.kitty_keys = crate::input::KittyModes::default();
    }

    /// Reset screen-scoped OSC state while preserving UAPI's session lifetime.
    fn reset_host_metadata(&mut self) {
        self.title.clear();
        self.icon_name.clear();
        self.title_stack.clear();
        self.cwd = None;
        self.colors.clear_overrides();
        self.links = crate::links::LinkTable::default();
        self.zones.clear(&mut self.tracked);
        self.pointer_shape = None;
        // A full reset clears this flag; DECSTR deliberately preserves it.
        self.mouse_shift_capture = None;
        // RIS drops partial clipboard transactions without touching the host clipboard.
        self.host_protocols.clipboard_write = None;
        self.host_protocols.notifications = None;
        self.replies.clear();
        // UAPI contexts survive RIS, including starts earlier in this parser turn.
        // Preserve their invalidation so the host still learns the retained path.
        self.events
            .retain(|event| matches!(event, super::TerminalEvent::ContextsChanged));
        self.clear_drop_target();
    }
}
