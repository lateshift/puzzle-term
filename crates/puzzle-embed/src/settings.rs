//! Native JSON adapter for the neutral settings store (§14.4).
//!
//! The terminal crate sees only `puzzle_bus::Value`. This module owns file
//! paths, serde_json and mtime checks, so neither wasm nor the VT parser
//! inherits a desktop persistence dependency. A bad reload retains the
//! last good store; the user can fix the file without restarting the shell.
use puzzle_term::settings::SettingsStore;
use std::{path::PathBuf, time::SystemTime};
/// Host shortcut mutations after the Session pump releases its borrow.
mod actions;
/// Reads and writes the platform's config.json document.
mod file;
/// Converts between serde_json and the bounded bus value tree.
pub mod json;
/// CLI, profile and user-layer loading in precedence order.
mod load;
/// Starter files, mtime reloads and native user-layer saves.
mod persist;

/// A desktop settings store and the last successful file timestamp.
#[derive(Clone)]
pub struct DesktopSettings {
    /// Validated user, profile and session layers.
    pub store: SettingsStore,
    /// Explicit or platform-default configuration file.
    path: PathBuf,
    /// Last observed modification time, including a rejected edit.
    modified: Option<SystemTime>,
    /// CLI profile remains selected after a successful file reload.
    profile: Option<String>,
    /// Transient zoom is relative to the resolved size, never saved to config.
    zoom: i32,
}

impl DesktopSettings {
    /// Applies a JSON settings patch through the native store's existing schema.
    /// Returns the same clamping/rejection report as every other settings source.
    pub fn apply_json(&mut self, text: &str) -> Result<puzzle_term::settings::ApplyReport, String> {
        let value =
            serde_json::from_str(text).map_err(|e| format!("invalid settings JSON: {e}"))?;
        let value = json::from_json(&value, 0)?;
        Ok(self
            .store
            .apply_patch(&value, puzzle_term::settings::Layer::Session))
    }
    /// Returns the harness-only 15 px fallback unless a source chose a size.
    pub fn run_font_size(&self) -> f32 {
        let base = if self.store.has_override("font.size") {
            self.store.effective().font_size
        } else {
            15.0
        };
        (base + self.zoom as f32).clamp(4.0, 200.0)
    }

    /// Returns the resolved path for diagnostics and init-config.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(test)]
/// Persistence tests use isolated temporary directories.
mod tests;
