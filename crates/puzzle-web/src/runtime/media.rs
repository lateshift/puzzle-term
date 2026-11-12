//! Browser system colors are a transient theme overlay, never persisted.
//!
//! Computed styles resolve platform colors once per forced-colors change.
//! The normal palette is restored from the terminal's own layers on disable;
//! a shared text context contains no appearance policy or CSS objects.
//!
//! Forced colors belong to the browser's accessibility policy. They must not
//! rewrite the user's selected pair or persist a synthetic palette. Disabling
//! the preference restores the same settings that were active beforehand.
//! The generated theme inherits all unspecified UI tokens from the current
//! theme, while the five CSS system tokens replace its relevant terminal ink.
//!
//! CSS system color names cannot be passed straight into the neutral resolver,
//! which accepts concrete hex colors. A probe resolves them through the browser
//! so Windows high-contrast choices become actual bytes without a guessed map.
//! The element is removed even if one computed-style request throws.
//!
//! The container exposes theme colors to page controls, but the GPU clear is
//! still sourced from the RenderFrame. Updating CSS does not draw a terminal
//! image or introduce a second presentation path outside requestAnimationFrame.
use super::*;
use puzzle_term::bus::Value;
use std::collections::BTreeMap;
use wasm_bindgen::JsCast;

/// Samples CSS system colors and applies them through the neutral resolver.
pub(super) fn forced_colors(owner: &Owner, enabled: bool) -> Result<(), JsValue> {
    // Work on a copy: accessibility overlays never become the session's persisted layers.
    let mut settings = owner.borrow().settings.clone();
    if enabled {
        // Sample only on this media change; a frame cannot trigger DOM style/layout reads.
        let colors = system_colors()?;
        let base = if owner.borrow().session.terminal().appearance_dark {
            settings.theme_dark.clone()
        } else {
            settings.theme_light.clone()
        };
        // Inheritance provides the remaining UI tokens without inventing system colors.
        let mut theme = BTreeMap::from([("extends".into(), Value::String(base))]);
        for (key, value) in colors {
            theme.insert(key.into(), Value::String(value));
        }
        // A validated settings map is expected, but corrupted input still returns an error.
        let Value::Map(themes) = &mut settings.themes else {
            return Err(value::error(
                "InvalidArgument",
                "custom themes must be a map",
            ));
        };
        // This transient name is local to the terminal, never another registry category.
        themes.insert("browser-forced".into(), Value::Map(theme));
        // Both appearance members select the same platform palette, so an
        // OS light/dark change cannot undo the user's forced-colors policy.
        settings.theme_dark = "browser-forced".into();
        settings.theme_light = "browser-forced".into();
        settings.theme_transition_ms = 0;
    }
    // Disabling the overlay applies the original pair, including cursor precedence.
    owner
        .borrow_mut()
        .session
        .apply_settings(&settings, Instant::now());
    css(owner)
}

/// A hidden ordinary element asks the browser to resolve its own palette.
fn system_colors() -> Result<Vec<(&'static str, String)>, JsValue> {
    // Browser globals may be absent in an embedding context; no unwrap enters production.
    let window = window()?;
    let document = window
        .document()
        .ok_or_else(|| value::error("InvalidArgument", "no document"))?;
    // A regular HTML element participates in system-color resolution without a canvas.
    let probe = document
        .create_element("span")?
        .dyn_into::<web_sys::HtmlElement>()?;
    // It must not change the terminal's measured container geometry while sampled.
    probe.style().set_property("position", "absolute")?;
    probe.style().set_property("visibility", "hidden")?;
    document
        .body()
        .ok_or_else(|| value::error("InvalidArgument", "no body"))?
        .append_child(&probe)?;
    // Capture failure before cleanup so a rejected property does not leave hidden DOM behind.
    let result = (|| {
        let mut colors = Vec::new();
        for (token, system) in [
            ("background", "Canvas"),
            ("foreground", "CanvasText"),
            ("selectionBackground", "Highlight"),
            ("selectionForeground", "HighlightText"),
            ("link", "LinkText"),
        ] {
            // The computed value reflects the user's current platform contrast palette.
            probe.style().set_property("color", system)?;
            let css = window
                .get_computed_style(&probe)?
                .ok_or_else(|| value::error("InvalidArgument", "no computed style"))?;
            // Normalize the browser serialization to the core's supported concrete syntax.
            colors.push((token, rgb_hex(&css.get_property_value("color")?)?));
        }
        Ok(colors)
    })();
    // Always detach the probe, including a rejected style/color conversion.
    probe.remove();
    result
}

/// Browser-computed rgb channels enter the core as its supported hex syntax.
fn rgb_hex(css: &str) -> Result<String, JsValue> {
    // These computed system colors are opaque sRGB; other formats are rejected, not guessed.
    let channels = css
        .strip_prefix("rgb(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| value::error("InvalidArgument", "system color is not RGB"))?;
    // Channel validation prevents negative, fractional or overflowing values from truncating.
    let bytes: Result<Vec<u8>, _> = channels
        .split(',')
        .map(|c| c.trim().parse::<u8>())
        .collect();
    match bytes
        .map_err(|_| value::error("InvalidArgument", "invalid system RGB"))?
        .as_slice()
    {
        [r, g, b] => Ok(format!("#{r:02x}{g:02x}{b:02x}")),
        _ => Err(value::error(
            "InvalidArgument",
            "system RGB needs three channels",
        )),
    }
}

/// Host controls inherit the active theme only after a resolved theme changes.
pub(super) fn css(owner: &Owner) -> Result<(), JsValue> {
    // Theme facts trigger this path after the core borrow; no per-frame style polling.
    // CSS reflects the resolved in-progress palette, including its current fade position.
    let r = owner.borrow();
    let (Some(parent), Some(theme)) = (&r.parent, r.session.active_theme()) else {
        return Ok(());
    };
    // Native controls and page scrollbars should match the terminal's resolved appearance.
    let style = parent.style();
    style.set_property(
        "color-scheme",
        if theme.appearance == puzzle_term::theme::Appearance::Dark {
            "dark"
        } else {
            "light"
        },
    )?;
    // Fixed three-channel arrays are trusted resolved tokens rather than external indices.
    let hex = |rgb: [u8; 3]| format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
    style.set_property("background-color", &hex(theme.background))?;
    // Embedding pages opt out without changing the terminal's actual palette or GPU clear.
    if r.settings.theme_export_css_variables {
        for (key, color) in [
            ("foreground", theme.foreground),
            ("background", theme.background),
            ("cursor", theme.cursor),
            ("accent", theme.ui.accent),
        ] {
            style.set_property(&format!("--puzzle-{key}"), &hex(color))?;
        }
        // Stable ANSI names let surrounding status chrome follow the same semantic palette.
        for (index, color) in theme.palette.iter().enumerate() {
            style.set_property(&format!("--puzzle-ansi-{index}"), &hex(*color))?;
        }
    }
    Ok(())
}
