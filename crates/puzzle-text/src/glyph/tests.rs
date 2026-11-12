//! Backend and hinting placement invariants (design §8.9, D-28).
//!
//! Tests use a fixed placement to show that selecting a different backend
//! changes only the drawable result; layout coordinates and advances stay
//! the authority for selection, geometry and cursor positioning.
use super::{
    BackendKind, GlyphBackendSelector, GlyphKey, GlyphPlacement, HintMode, RendererChoice,
};

/// A vector effect overrides an explicit atlas request without moving text.
#[test]
fn effect_selects_vector_without_moving_placement() {
    let mut placement = GlyphPlacement {
        cell_column: 0,
        key: GlyphKey {
            face: 0,
            glyph_id: 42,
            instance: 0,
            hint_ppem: 12,
            hint_mode: HintMode::Light,
        },
        origin_px: [16, 24],
        baseline_px: 12.0,
        size_px: 12.0,
        advance_px: 8,
        ink_limit_px: 8,
        cell_height_px: 16,
        cell_span: 1,
        color: 0xffff_ffff,
        effect: false,
        color_glyph: false,
    };
    let selector = GlyphBackendSelector {
        renderer: RendererChoice::Atlas,
    };
    assert_eq!(selector.choose(&placement), BackendKind::Bitmap);
    let origin = placement.origin_px;
    let advance = placement.advance_px;
    placement.effect = true;
    assert_eq!(selector.choose(&placement), BackendKind::Vector);
    assert_eq!(placement.origin_px, origin);
    assert_eq!(placement.advance_px, advance);
}

/// D-27 keeps the default on Slug even below the former atlas threshold.
#[test]
fn auto_uses_slug_for_small_grid_text() {
    let placement = GlyphPlacement {
        cell_column: 0,
        key: GlyphKey {
            face: 0,
            glyph_id: 3,
            instance: 0,
            hint_ppem: 10,
            hint_mode: HintMode::Light,
        },
        origin_px: [0, 0],
        baseline_px: 12.0,
        size_px: 10.0,
        advance_px: 6,
        ink_limit_px: 6,
        cell_height_px: 13,
        cell_span: 1,
        color: 0xffff_ffff,
        effect: false,
        color_glyph: false,
    };
    let selector = GlyphBackendSelector {
        renderer: RendererChoice::Auto,
    };
    assert_eq!(selector.choose(&placement), BackendKind::Vector);
}
