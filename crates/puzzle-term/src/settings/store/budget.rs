//! Repeated invalid file reloads must not flood the log (R8).
//! The schema still reports every clamp to the caller. This latch affects only
//! the warning sink and survives the store's transactional reload clones.
use super::SettingsStore;
impl SettingsStore {
    /// Warns once per settings owner; the ApplyReport still reports every clamp.
    pub(super) fn log_glyph_clamp(&mut self, key: &str) {
        if key == "render.glyphMemoryMb" && !self.glyph_budget_warned {
            // Latch before publishing so the next reload cannot repeat this warning.
            self.glyph_budget_warned = true;
            tracing::warn!(
                event = "settings.invalid",
                key,
                minimum = crate::frame_builder::context::budget::MIN_MIB,
                "glyph memory clamped to the atlas capacity range"
            );
        }
    }
}

#[cfg(test)]
/// A thread-local observer proves repeated reloads keep one warning.
mod tests;
