//! Effective source geometry must preserve exact neutral coordinates at unit scale.
use super::*;
use puzzle_text::glyph::{GlyphKey, HintMode};

/// Large integer origins expose the lossy float round-trip in the ordinary path.
#[test]
fn unit_raster_scale_preserves_the_placement_exactly() {
    let placement = placement();
    assert_eq!(effective(placement, 1.0), placement);
}

/// Actual effect magnification still scales the fitting crop and its baseline together.
#[test]
fn effect_raster_scale_changes_only_preparation_geometry() {
    let mut placement = placement();
    placement.origin_px = [3, -7];
    let scaled = effective(placement, 2.0);
    assert_eq!(scaled.origin_px, [6, -14]);
    assert_eq!(scaled.size_px, 30.0);
    assert_eq!(scaled.baseline_px, 24.0);
    assert_eq!(scaled.advance_px, 18);
    assert_eq!(scaled.ink_limit_px, 36);
    assert_eq!(scaled.cell_height_px, 40);
    assert_eq!(scaled.key, placement.key);
    assert_eq!(scaled.cell_column, placement.cell_column);
    assert_eq!(scaled.cell_span, placement.cell_span);
}

/// Backend-neutral placement includes values whose exact integer identity matters.
fn placement() -> GlyphPlacement {
    GlyphPlacement {
        key: GlyphKey {
            face: 0,
            glyph_id: 1,
            instance: 0,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        cell_column: 2,
        origin_px: [16_777_217, -16_777_217],
        baseline_px: 12.0,
        size_px: 15.0,
        advance_px: 9,
        ink_limit_px: 18,
        cell_height_px: 20,
        cell_span: 2,
        color: 0xffffffff,
        effect: false,
        color_glyph: false,
    }
}
