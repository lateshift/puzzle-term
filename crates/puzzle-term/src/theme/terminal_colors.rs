//! Installs every terminal colour default from a resolved theme (M18.1).
//! Shared by Session and headless harness so replies and rendered colours agree.
//! design §14.7 deliberately clears all overrides on any effective theme change.
//! A fade has two colour views: queries immediately expose the target theme,
//! while a sealed render snapshot carries sampled defaults. Both use this
//! installer, but only replacing the effective theme clears application values.
//! Pointer and Tektronix roles have no host-independent drawing; their defaults
//! still need predictable query answers. Special roles have no distinct theme
//! tokens today, so their fallback is foreground rather than an invented style.
use super::Theme;
use puzzle_vt::color_table::{ColorSlot, ColorTable, ColorValue, DynamicColor};

/// Maps semantic theme tokens into the VT's protocol-independent addresses.
impl Theme {
    /// Replaces defaults and clears overrides, including stored-only xterm roles.
    pub fn install_terminal_colors(&self, table: &mut ColorTable) {
        // One clear covers palette, special and every dynamic/kitty-only role.
        table.clear_overrides();
        self.write_terminal_defaults(table);
    }

    /// Projects a fade sample without clearing application overrides during animation.
    pub(crate) fn write_terminal_defaults(&self, table: &mut ColorTable) {
        use DynamicColor::*;
        for index in 0..=255_u8 {
            // The authored ANSI palette precedes xterm's cube and grey defaults.
            let rgb = if index < 16 {
                self.palette[usize::from(index)]
            } else {
                puzzle_vt::color_table::palette(index)
            };
            table.set_default(ColorSlot::Palette(index), ColorValue::Rgb(rgb));
        }
        // Cursor text and bell flash are real theme tokens even without xterm numbers.
        for (role, rgb) in [
            (Foreground, self.foreground),
            (Background, self.background),
            (Cursor, self.cursor),
            (CursorText, self.cursor_text),
            (PointerForeground, self.foreground),
            (PointerBackground, self.background),
            (TektronixForeground, self.foreground),
            (TektronixBackground, self.background),
            (TektronixCursor, self.cursor),
            (SelectionBackground, self.selection_background),
            (VisualBell, self.ui.bell_flash),
        ] {
            table.set_default(ColorSlot::Dynamic(role), ColorValue::Rgb(rgb));
        }
        // None is meaningful: selected SGR ink remains unchanged until overridden.
        table.set_default(
            ColorSlot::Dynamic(SelectionForeground),
            self.selection_foreground
                .map_or(ColorValue::Dynamic, ColorValue::Rgb),
        );
        for index in 0..5 {
            // There is no separate theme bold/italic/underline/blink/reverse token.
            // Like the reference terminal these roles are stored/reported, without inventing styles.
            if let Some(role) = puzzle_vt::color_table::SpecialColor::from_index(index) {
                table.set_default(ColorSlot::Special(role), ColorValue::Rgb(self.foreground));
            }
        }
    }
}
