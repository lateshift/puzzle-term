//! Fallback fitting changes ink transforms, never source placements (§8.4).
use crate::glyph_ink::{append_ink, fitted_instance};
use puzzle_slug::atlas::ShapeHandle;
use puzzle_text::glyph::{GlyphKey, GlyphPlacement, HintMode};

/// Builds a backend-neutral two-cell glyph with a deliberately wide bbox.
fn fixture() -> (GlyphPlacement, ShapeHandle) {
    let placement = GlyphPlacement {
        cell_column: 0,
        key: GlyphKey {
            face: 0,
            glyph_id: 42,
            instance: 0,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        origin_px: [16, 32],
        baseline_px: 12.0,
        size_px: 12.0,
        advance_px: 16,
        ink_limit_px: 16,
        cell_height_px: 16,
        cell_span: 2,
        color: 0xffff_ffff,
        effect: false,
        color_glyph: false,
    };
    let handle = ShapeHandle {
        generation: 1,
        glyph_loc: [0, 0],
        band_info: 0,
        bbox: [0.0, 0.0, 2.0, 1.0],
    };
    (placement, handle)
}

/// A large fallback is scaled into the span while the grid advance stays put.
#[test]
fn wide_fallback_fits_without_relayout() {
    let (placement, handle) = fixture();
    let draw = fitted_instance(&placement, handle, false, true);
    assert!((draw.m[0] * 2.0 - 16.0).abs() < 0.001);
    assert_eq!(draw.t[0], 16.0);
    assert_eq!(placement.origin_px, [16, 32]);
    assert_eq!(placement.advance_px, 16);
}

/// A narrow two-cell glyph is centered; synthetic bold adds ink only.
#[test]
fn narrow_fallback_and_bold_keep_cell_positions() {
    let (placement, mut handle) = fixture();
    handle.bbox[2] = 0.5;
    let draw = fitted_instance(&placement, handle, false, true);
    assert_eq!(draw.t[0], 21.0);
    let mut layers = Vec::new();
    append_ink(&mut layers, &placement, handle, false, true, true);
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].t[0], 21.0);
    assert_eq!(layers[1].t[0], 22.0);
    assert_eq!(placement.origin_px, [16, 32]);
}
