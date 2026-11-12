//! Authored overlay tokens override derived defaults after custom inheritance (§14.6).
//!
//! UI colors were dormant before M9, apart from M8's search paper. Resolving
//! them here keeps every provider on the same validated theme surface as grid
//! ink. A sparse child inherits explicitly authored parent tokens; unsupplied
//! colors still derive from the final terminal palette. The scrollbar thumb
//! stores straight alpha, unlike the existing opaque panel/marker fields.
//! Registration validates through the same parser used by resolution, so a
//! malformed color cannot be accepted and silently replaced with a default.
use super::{CustomMap, Theme, Value, rgb};

/// Applies supplied opaque semantic surfaces, then the alpha-preserving thumb.
pub(super) fn apply(theme: &mut Theme, map: &CustomMap) -> Result<(), String> {
    // These references are disjoint fields of one resolved palette owner.
    let ui = &mut theme.ui;
    // References name the actual semantic fields, without a provider-local palette.
    // Search retains its authored values even when inherited by a sparse child.
    for (key, target) in [
        ("accent", &mut ui.accent),
        ("scrollbarTrack", &mut ui.scrollbar_track),
        ("searchMatch", &mut ui.search_match),
        ("searchCurrent", &mut ui.search_current),
        ("link", &mut ui.link),
        ("zoneOk", &mut ui.zone_ok),
        ("zoneError", &mut ui.zone_error),
        ("badgeBackground", &mut ui.badge_background),
        ("badgeForeground", &mut ui.badge_foreground),
        ("tooltipBackground", &mut ui.tooltip_background),
        ("tooltipForeground", &mut ui.tooltip_foreground),
        ("pillBackground", &mut ui.pill_background),
        ("pillForeground", &mut ui.pill_foreground),
        ("bellFlash", &mut ui.bell_flash),
        ("ghostText", &mut ui.ghost_text),
        ("preeditUnderline", &mut ui.preedit_underline),
        ("gutter", &mut ui.gutter),
    ] {
        // Absence leaves a derived or explicitly inherited value untouched.
        if let Some(value) = map.get(key) {
            *target = rgb(value, key)?;
        }
    }
    if let Some(value) = map.get("scrollbarThumb") {
        // Thumb alpha is authored separately from the provider's visibility fade.
        ui.scrollbar_thumb = rgba(value)?;
    }
    Ok(())
}

/// Accepts six-digit opaque or eight-digit straight-alpha thumb colors.
fn rgba(value: &Value) -> Result<[u8; 4], String> {
    let Value::String(text) = value else {
        return Err("scrollbarThumb must be a color".into());
    };
    let digits = text
        .strip_prefix('#')
        .filter(|s| matches!(s.len(), 6 | 8) && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("scrollbarThumb must be #rrggbb or #rrggbbaa")?;
    // Six-digit values imply opaque alpha; an eighth digit pair replaces it.
    // Preserve straight RGB so the renderer premultiplies only after coverage.
    let mut out = [255; 4];
    // Length and ASCII validation precede byte slicing, including hostile UTF-8 input.
    for (channel, pair) in out.iter_mut().zip(digits.as_bytes().chunks_exact(2)) {
        let pair = std::str::from_utf8(pair).map_err(|e| e.to_string())?;
        *channel = u8::from_str_radix(pair, 16).map_err(|e| e.to_string())?;
    }
    Ok(out)
}
