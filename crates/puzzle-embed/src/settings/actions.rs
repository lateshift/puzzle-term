//! Applies typed host shortcuts to transient settings, never to VT bytes.
//!
//! The session thread emits typed host actions after keybinding policy. File
//! access and profile selection run on the host; settings return as messages.
//! Zoom is relative to the configured size; reset therefore restores the
//! user's profile or CLI size, including the run-only 15 px fallback.
use super::DesktopSettings;
use puzzle_term::messages::Value;
use puzzle_term::{
    keybind::Action,
    settings::{Layer, ThemeMode},
};

/// Settings actions preserve the original user document and layer precedence.
impl DesktopSettings {
    /// Mutates a host setting, returning false for actions owned by the core.
    pub fn binding_action(&mut self, action: &Action, dark: bool) -> Result<bool, String> {
        // Core actions never arrive here: this queue carries only host work.
        // A settings change must survive the next file reload by living in
        // the session layer, rather than modifying the persisted user layer.
        match action {
            Action::Zoom(steps) => self.zoom = self.zoom.saturating_add(*steps).clamp(-200, 200),
            Action::ZoomReset => self.zoom = 0,
            Action::SetAppearance(mode) => self.set_appearance(*mode),
            Action::ToggleAppearance => self.set_appearance(if dark {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            }),
            Action::Profile(name) => {
                // Profile activation is transactional; a typo cannot replace
                // the last-good table with defaults on the next focus reload.
                let report = self.store.activate_profile(name);
                if let Some(issue) = report.rejected.first() {
                    return Err(issue.message.clone());
                }
                self.profile = Some(name.clone());
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Writes only the session layer, leaving the stored auto pair intact.
    fn set_appearance(&mut self, mode: ThemeMode) {
        // Preserve the configured light/dark names; only pair selection changes.
        // The Session applies mode-2031 and OSC reset for actual token changes.
        let name = match mode {
            ThemeMode::Auto => "auto",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        };
        self.store
            .apply_key("theme.mode", Value::String(name.into()), Layer::Session);
    }
}
