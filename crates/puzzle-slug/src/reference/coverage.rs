//! Coverage combination from SlugPixelShader.hlsl, unchanged in math.
//!
//! Slug attribution and license identifiers are at this port's module root.
//! The E flag switches fill rules at runtime, matching the reference's
//! compile-time SLUG_EVENODD path. The W flag reserves optical weighting
//! for later instances; neither alters root finding or ray contributions.

/// Combine coverages from horizontal and vertical rays using their weights.
/// Absolute values ensure either winding direction convention works.
#[must_use]
pub(super) fn calc_coverage(xcov: f32, ycov: f32, xwgt: f32, ywgt: f32, flags: u8) -> f32 {
    let mut coverage = ((xcov * xwgt + ycov * ywgt).abs() / (xwgt + ywgt).max(1.0 / 65536.0))
        .max(xcov.abs().min(ycov.abs()));
    if flags & crate::encoding::EVEN_ODD == 0 {
        // Using nonzero fill rule here.
        coverage = coverage.clamp(0.0, 1.0);
    } else {
        // Using even-odd fill rule here.
        coverage = 1.0 - (1.0 - (coverage * 0.5).fract() * 2.0).abs();
    }
    // SLUG_WEIGHT takes a square root to boost optical weight.
    if flags & crate::encoding::WEIGHT != 0 {
        coverage = coverage.sqrt();
    }
    coverage
}
