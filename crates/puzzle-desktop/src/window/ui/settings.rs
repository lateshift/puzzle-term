//! Host-side settings reload and actions with nonblocking renderer notification.
//!
//! Only the native host reads config files; Surface queues the resulting policy. Font
//! measurement and GPU reconfiguration happen later on the render worker.
//!
//! A successful reload publishes one validated snapshot. The last-good store
//! survives syntax errors, and focus checks mtime only when focus arrives.
//! Keybindings and Option policy take effect before the next key can encode.
//! Font zoom and family edits transfer owned settings; they cannot start a
//! glyph lookup, block on a worker result, or rebuild a GPU attachment here.
//! Session layers remain authoritative on the input owner, including host
//! actions drained after the same bus turn that recognized their binding.
use super::*;

/// Configuration policy stays beside native input; rendering receives only snapshots.
impl InputWindow {
    /// Retains last-good settings when a user edit is malformed (§14.4).
    pub(super) fn reload_settings(&mut self, force: bool) {
        // Explicit reload bypasses mtime; focus reload avoids reading an unchanged file.
        let result = if force {
            self.settings.reload().map(|()| true)
        } else {
            self.settings.reload_if_changed()
        };
        match result {
            Ok(true) => self.apply_settings(),
            Ok(false) => {}
            Err(error) => tracing::warn!(event = "settings.reload_failed", %error),
        }
    }

    /// Applies input policy now and forwards expensive font work to its exclusive owner.
    fn apply_settings(&mut self) {
        // Validated input and render preferences are applied through one capability.
        self.sent(self.session.settings(self.settings.clone()));
    }

    /// The action carries its resolved appearance from its original core turn.
    pub(super) fn binding_action(&mut self, action: puzzle_term::keybind::Action, dark: bool) {
        if action == puzzle_term::keybind::Action::ReloadSettings {
            self.reload_settings(true);
            return;
        }
        match self.settings.binding_action(&action, dark) {
            Ok(true) => self.apply_settings(),
            Ok(false) => {}
            Err(error) => tracing::warn!(event = "settings.binding_failed", %error),
        }
    }
}
