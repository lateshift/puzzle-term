//! Named D-27 rows and post-coverage compositing choices.
//!
//! Every B/C branch remains visible. No row is ranked: the user and
//! reviewer choose after seeing the same text under both color schemes.
/// Keeps color math separate from the row catalogue.
mod blend;
#[cfg(test)]
mod tests;
pub(super) use blend::composite;
use puzzle_text::probe::ProbeHintMode;

/// The coverage-to-color operation applied after the unchanged oracle.
#[derive(Clone, Copy)]
pub(super) enum Blend {
    /// The gamma-space byte blend used by the current specimen.
    Gamma,
    /// Decode, mix in linear light and encode with exact sRGB transfer.
    Linear,
    /// Linear light after the user-specified luminance correction to alpha.
    Corrected,
}

/// One named comparison row for every requested device size.
#[derive(Clone, Copy)]
pub(super) struct Variant {
    /// Stable label printed by the same oracle at 14 px.
    pub(super) name: &'static str,
    /// None retains the M2 unhinted outline path.
    pub(super) hint: Option<ProbeHintMode>,
    /// Nudges size by at most 3% so cap height reaches a whole device pixel.
    pub(super) snap_cap: bool,
    /// Sets the reference W bit, which takes sqrt of coverage.
    pub(super) weight: bool,
    /// Selects the final coverage-to-color computation only.
    pub(super) blend: Blend,
}

/// Constructs a named row while keeping the catalogue readable as a matrix.
const fn row(
    name: &'static str,
    hint: Option<ProbeHintMode>,
    snap_cap: bool,
    weight: bool,
    blend: Blend,
) -> Variant {
    Variant {
        name,
        hint,
        snap_cap,
        weight,
        blend,
    }
}

/// The font's own TrueType bytecode engine, shared across its comparison rows.
const NATIVE: Option<ProbeHintMode> = Some(ProbeHintMode::Interpreter);
/// skrifa's light autohinter, shared across its comparison rows.
const LIGHT: Option<ProbeHintMode> = Some(ProbeHintMode::AutoLight);

/// All requested variants, with native and light branches kept separate.
pub(super) const VARIANTS: [Variant; 11] = [
    // A is the unchanged M2 specimen, establishing the original softness.
    row("A", None, false, false, Blend::Gamma),
    // B and C differ only in skrifa's bytecode versus light autohinter.
    row("B", NATIVE, false, false, Blend::Gamma),
    row("C", LIGHT, false, false, Blend::Gamma),
    // D tests cap alignment independently for both hinted outline engines.
    row("D-B", NATIVE, true, false, Blend::Gamma),
    row("D-C", LIGHT, true, false, Blend::Gamma),
    // E adds the reference W flag but retains ordinary sRGB byte blending.
    row("E-B", NATIVE, true, true, Blend::Gamma),
    row("E-C", LIGHT, true, true, Blend::Gamma),
    // F1 isolates linear-light compositing from any coverage adjustment.
    row("F1-B", NATIVE, true, false, Blend::Linear),
    row("F1-C", LIGHT, true, false, Blend::Linear),
    // F2 adds the user-specified luminance correction to the F1 operation.
    row("F2-B", NATIVE, true, false, Blend::Corrected),
    row("F2-C", LIGHT, true, false, Blend::Corrected),
];

/// A bounded cap-height decision for one device size.
pub(super) struct Snap {
    /// Effective ppem, unchanged if exact cap alignment needs more than 3%.
    pub(super) size: f32,
    /// Percentage needed for exact snapping when the limit prevents it.
    pub(super) refused_percent: Option<f32>,
}

/// Applies exact §8.2 cap snapping only when the ±3% allowance permits it.
pub(super) fn snapped_size(size: u32, cap_height_em: f32) -> Snap {
    let original = size as f32;
    // The font's cap height is a ratio of design units to units per em.
    // A malformed metric cannot define an exact pixel-aligned cap.
    if !cap_height_em.is_finite() || cap_height_em <= 0.0 {
        return Snap {
            size: original,
            refused_percent: None,
        };
    }
    let proposed = (original * cap_height_em).round() / cap_height_em;
    let adjustment = 100.0 * (proposed - original).abs() / original;
    if adjustment <= 3.0 {
        Snap {
            size: proposed,
            refused_percent: None,
        }
    } else {
        // A partial nudge would change the text size without aligning the
        // cap, so preserve the original ppem and mark the review label.
        Snap {
            size: original,
            refused_percent: Some(adjustment),
        }
    }
}
