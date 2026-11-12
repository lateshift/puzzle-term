//! Uniform ink fitting inside fixed device-pixel cells (design §8.4, §8.7).
//!
//! Layout owns origins and advances. This calculation returns only an ink
//! transform, so both a Slug outline and a bitmap obey the same geometry.
use crate::glyph::GlyphPlacement;

/// Uniform scale and device-pixel translation relative to the grid baseline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InkFit {
    /// Multiplicative factor for the caller's em-to-device transform.
    pub scale: f32,
    /// Additional device-pixel ink translation; placement remains immutable.
    pub offset: [f32; 2],
}

/// Fits device-space y-up bounds, preserving ordinary fallback baselines.
///
/// Bounds are measured before fitting; `baseline` is the row's baseline in
/// device pixels. The returned translation is relative to placement origin.
/// Widths up to the specified 110% tolerance keep their original scale;
/// taller ink always shrinks uniformly, retaining the baseline for text.
/// Emoji instead occupy the cell rectangle with a 5% inset on each side.
/// Every dimension uses device pixels, so DPR only changes input geometry.
/// No rounded fit feeds back into source width, advance, or later origins.
pub fn fit(placement: &GlyphPlacement, bounds: [f32; 4], emoji: bool, baseline: f32) -> InkFit {
    let [left, bottom, right, top] = bounds;
    let width = right - left;
    let height = top - bottom;
    // Empty or invalid ink cannot divide by zero or contaminate GPU matrices.
    if !bounds.iter().all(|value| value.is_finite()) || width <= 0.0 || height <= 0.0 {
        return InkFit {
            scale: 1.0,
            offset: [0.0; 2],
        };
    }
    let limit = placement.ink_limit_px as f32;
    let cell_height = placement.cell_height_px as f32;
    // Emoji have a 5% inset on each side of their fixed allocation. Their
    // ink may enlarge to fill it; ordinary fallback only scales down.
    let inset = if emoji { 0.05 } else { 0.0 };
    let available = [
        limit * (1.0 - 2.0 * inset),
        cell_height * (1.0 - 2.0 * inset),
    ];
    let width_fit = if emoji || width > limit * 1.1 {
        available[0] / width
    } else {
        1.0
    };
    let height_fit = available[1] / height;
    let scale = if emoji {
        width_fit.min(height_fit)
    } else {
        width_fit.min(height_fit).min(1.0)
    };
    // Width fitting removes the source side bearing. A narrower two-cell
    // glyph is centered, and tall text still pivots around its baseline.
    let x = if emoji || placement.cell_span == 2 && width * scale < limit {
        (limit - width * scale) * 0.5 - left * scale
    } else if width > limit * 1.1 {
        -left * scale
    } else {
        0.0
    };
    let y = if emoji {
        // Emoji occupy the row rectangle rather than inheriting a foreign
        // font ascent. Its bottom sits above the row's inset from the baseline.
        (cell_height + height * scale) * 0.5 + bottom * scale - baseline
    } else {
        0.0
    };
    InkFit {
        scale,
        offset: [x, y],
    }
}

#[cfg(test)]
mod tests;
