//! D-27's per-glyph small-text hinting policy (design §8.5, §9.7).
//!
//! The chosen light auto-hinter changes only the outline delivered to
//! Slug. It never changes the glyph's advance or cell origin. Transformed
//! text stays unhinted, because pixel-grid fitting is invalid after a
//! rotation, shear or zoom. DPR 1.5 is the explicit handoff to true curves.

use crate::glyph::HintMode;

/// User-facing hinting choices reserved by the font settings schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintingSetting {
    /// Use light auto-hinting only on eligible small grid text.
    Auto,
    /// Retain original outlines at every size.
    None,
    /// Request light auto-hinting when the grid is eligible.
    Light,
    /// Request the font's own instructions when the grid is eligible.
    Native,
}

/// Validated outline choice and size-specific cache suffix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HintChoice {
    /// Resolved skrifa engine, or no hinter.
    pub mode: HintMode,
    /// Rounded device pixels per em; zero keeps an em-space shape reusable.
    pub ppem: u16,
}

/// Selects the outline while leaving placement geometry to the text layouter.
pub fn choose(
    setting: HintingSetting,
    device_size: f32,
    dpr: f32,
    transformed: bool,
) -> Result<HintChoice, String> {
    if !device_size.is_finite() || device_size <= 0.0 || !dpr.is_finite() || dpr <= 0.0 {
        return Err("font size and DPR must be positive finite numbers".into());
    }
    // Only original, axis-aligned grid text on a low-DPR display may be
    // fitted to pixels. An effect can still share the unhinted cache entry.
    if transformed || dpr >= 1.5 || setting == HintingSetting::None {
        return Ok(HintChoice {
            mode: HintMode::None,
            ppem: 0,
        });
    }
    let ppem = device_size.round();
    // Skrifa's hinter consumes an integer device ppem. Keeping the rounded
    // value in the key prevents a 12 px shape from serving a 13 px request.
    if ppem < 1.0 || ppem > f32::from(u16::MAX) {
        return Err("hinted font size is outside supported ppem range".into());
    }
    let mode = match setting {
        HintingSetting::Auto | HintingSetting::Light => HintMode::Light,
        HintingSetting::Native => HintMode::Native,
        HintingSetting::None => return Err("unreachable unhinted choice".into()),
    };
    Ok(HintChoice {
        mode,
        ppem: ppem as u16,
    })
}

#[cfg(test)]
mod tests;
