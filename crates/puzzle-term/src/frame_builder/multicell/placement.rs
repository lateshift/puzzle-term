//! One affine maps an entire natural line into an OSC 66 render rectangle.
//!
//! Natural advances preserve whitespace. Ink overhangs enlarge that same box
//! before fitting, so italic and fallback ink cannot invade an adjacent block.
//! Fractional alignment moves the render area rather than centering each glyph.
//! The map remains row-relative until normal frame assembly adds padding and y.
//! A clipped root uses its original offset; exposing another fragment cannot
//! change glyph size or fit, only which row owns the same complete ink stream.
use crate::frame_snapshot::multicell::Block;
use kurbo::Affine;
use puzzle_overlay::geometry::Glyph;
use puzzle_text::metrics::CellMetrics;

/// Fits the complete ink/advance union uniformly, retaining the original row offset.
pub(super) fn map(
    metrics: CellMetrics,
    block: &Block,
    width: f32,
    height: u32,
    glyphs: &[Glyph],
) -> Affine {
    // Metadata was bounded by VT; no render-side reinterpretation changes occupancy.
    let sizing = block.value.sizing;
    // n/d is an ink multiplier only when it describes a proper fraction.
    let fraction = if sizing.numerator > 0 && sizing.numerator < sizing.denominator {
        f64::from(sizing.numerator) / f64::from(sizing.denominator)
    } else {
        1.0
    };
    // Physical occupancy comes from D-31 or explicit width, not natural font advance.
    let occupied = [
        block.value.columns as f64 * f64::from(metrics.width_px),
        f64::from(sizing.scale) * f64::from(metrics.height_px),
    ];
    // Alignment reserves the unpainted remainder around this fractional render area.
    let area = occupied.map(|value| value * fraction);
    // Include zero so leading/trailing whitespace stays part of natural layout.
    let mut min = [0.0_f64; 2];
    // A blank run has no contours, but still supplies a well-defined nonzero fit.
    let mut max = [f64::from(width.max(1.0)), f64::from(height.max(1))];
    // This walks only a newly shaped sparse block, never ordinary cells.
    for glyph in glyphs {
        // Use one union for the entire run, including italic bearings and mark overhangs.
        for axis in 0..2 {
            min[axis] = min[axis].min(f64::from(glyph.ink.origin[axis]));
            max[axis] = max[axis].max(f64::from(glyph.ink.origin[axis] + glyph.ink.size[axis]));
        }
    }
    // Overlong explicit runs and font overhangs use one protocol-permitted fit.
    let scale = (f64::from(sizing.scale) * fraction)
        .min(area[0] / (max[0] - min[0]))
        .min(area[1] / (max[1] - min[1]));
    // Kitty uses zero=start, one=end, two=center for both alignment axes.
    let alignment = |mode| match mode {
        1 => 1.0,
        2 => 0.5,
        _ => 0.0,
    };
    // Hidden original rows stay in the transform; viewport scissors remove them.
    Affine::translate((
        block.value.column as f64 * f64::from(metrics.width_px)
            + (occupied[0] - area[0]) * alignment(sizing.horizontal)
            - min[0] * scale,
        -f64::from(block.offset) * f64::from(metrics.height_px)
            + (occupied[1] - area[1]) * alignment(sizing.vertical)
            - min[1] * scale,
    )) * Affine::scale(scale)
}
