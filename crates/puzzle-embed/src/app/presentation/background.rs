//! Cold background selection mirrors the Session's initial validated theme pair.
//! Later colours come from RenderFrame.clear, so overrides and fades use the
//! actual rendered default without a synchronous query into Session.
//!
//! Cold attachment precedes Session ownership, so no worker can supply its first
//! colour yet. The existing registry provides the same built-ins, validation and
//! custom inheritance without creating another Session or text context.
//! This lookup is never used for steady-state frames or input callbacks.
//! Rejected definitions leave the initial dark default, matching core startup.
//! Once frames exist, their clear values also include OSC overrides and fades.
use crate::settings::DesktopSettings;
use puzzle_term::{
    settings::ThemeMode,
    theme::{Appearance, ThemeRegistry},
};

/// Resolve before the first CA transaction, including custom-theme inheritance.
pub(in crate::app) fn initial(
    settings: &DesktopSettings,
    system: Appearance,
) -> Result<[f32; 4], String> {
    let effective = settings.store.effective();
    let result = (|| {
        let mut registry = ThemeRegistry::default();
        let puzzle_term::settings::SettingsValue::Map(definitions) = &effective.themes else {
            return Err("themes must be an object".to_owned());
        };
        // Register all definitions before resolving: a child can sort before
        // its parent, and the Session accepts that same inheritance order.
        for (name, value) in definitions {
            registry.register(name, value)?;
        }
        // Forced modes override the initial OS observation, just as in Session.
        let appearance = match effective.theme_mode {
            ThemeMode::Auto => system,
            ThemeMode::Light => Appearance::Light,
            ThemeMode::Dark => Appearance::Dark,
        };
        registry.resolve(if appearance == Appearance::Light {
            &effective.theme_light
        } else {
            &effective.theme_dark
        })
    })();
    // Session also retains its initial dark default on a rejected theme config.
    let theme = result.or_else(|error| {
        tracing::warn!(event="native.initial_theme_rejected",%error);
        ThemeRegistry::default().resolve("puzzle-dark")
    })?;
    // Clear values are encoded sRGB with opaque alpha in the frame contract.
    let [r, g, b] = theme.background.map(|v| f32::from(v) / 255.0);
    Ok([r, g, b, 1.0])
}

/// Cold selection must match the pair/override resolution before the first image.
#[cfg(test)]
mod tests;
