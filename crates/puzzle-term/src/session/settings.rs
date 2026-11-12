//! Live application of typed settings to one Session (§14.3).
//!
//! The settings store resolves layers outside the hot parser path. A diff
//! from the desktop adapter reaches this method only after validation, so
//! the terminal sees concrete values and updates the affected subsystem.
//! The existing OSC 52 policies govern MIME clipboard reads and writes too.
use super::Session;
use crate::settings::Settings;
use std::time::Duration;
use web_time::Instant;

impl Session {
    /// Updates supported live policies without resetting terminal content.
    pub fn apply_settings(&mut self, settings: &Settings, now: Instant) {
        // The next feed observes the new budget; already parsed bytes and
        // pending transport order are unaffected by a configuration edit.
        self.parse_budget = Duration::from_millis(u64::from(settings.performance_parse_budget_ms));
        self.on_exit = if settings.session_on_exit == "hold" {
            super::OnExit::Hold
        } else {
            super::OnExit::Close
        };
        self.apply_link_settings(settings);
        // One policy governs ordinary OSC 52 and MIME OSC 5522 host access.
        self.clipboard.allow_read = settings.clipboard_osc52_read == "allow";
        self.clipboard.allow_write = settings.clipboard_osc52_write == "allow";
        // Image settings mutate only opted-in storage, never start a native worker.
        self.apply_image_settings(settings);
        self.mouse_shift_capture = settings.mouse_shift_capture;
        self.set_visual_bell(settings.bell_visual);
        // Preferences affect future word gestures and explicit copies.
        // They do not alter the current tracked range or invalidate glyph caches.
        self.pointer.line_end_snap = settings.selection_line_end_snap;
        self.selection.configure(
            &settings.selection_word_separators,
            settings.selection_trim_trailing,
            settings.selection_copy_on_select,
        );
        // The setting supplies the initial/live policy; apps may still DECSET/DECRST it.
        self.terminal
            .set_alternate_scroll_default(settings.mouse_alternate_scroll);
        self.viewport.configure(
            f64::from(settings.scrolling_multiplier),
            settings.scrolling_smooth,
            settings.scrolling_snap_to_rows,
        );
        self.viewport.on_input = settings.scrolling_on_input;
        self.viewport.on_output = settings.scrolling_on_output;
        self.abnormal_exit_ms = u64::from(settings.session_abnormal_exit_ms);
        self.terminal
            .set_grapheme_width_default(settings.unicode_grapheme_width == "unicode");
        // Shrinking history evicts oldest line identities, while growing
        // the limit leaves existing visible and historical rows intact.
        self.terminal
            .tracked
            .set_screen(crate::vt::terminal::ActiveScreen::Primary);
        self.terminal.primary.ring.set_limits(
            settings.scrollback_lines as usize,
            settings.scrollback_max_bytes as usize,
            &mut self.terminal.tracked,
        );
        self.terminal.tracked.set_screen(self.terminal.active);
        // History reduction may evict a selected endpoint. Reconcile it before
        // publishing viewport/marker state so no stale selection reaches a snapshot.
        self.reconcile_selection();
        self.reconcile_viewport(false);
        self.refresh_scrollbar();
        // The cursor setter updates its style epoch and timer without
        // mutating the application's stored DECSCUSR request (§14.2).
        self.set_cursor_settings(settings.cursor_settings(), now);
        // Theme colors are resolved outside frame assembly. A rejected
        // custom definition leaves the previous visible theme in place.
        self.apply_theme_settings(settings, now);
        if let Some(builder) = self.frame_policy.as_mut() {
            // Contrast and bold-bright are render policies, not mutations
            // of palette-tagged VT cells (§10.5).
            builder.set_color_options(
                settings.theme_bold_is_bright,
                settings.theme_minimum_contrast,
            );
        }
    }
}
