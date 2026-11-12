//! Geometry cases in device pixels, independent of the chosen font (§8.4).
use super::*;
use crate::glyph::{GlyphKey, HintMode};

/// Builds the same two-cell allocation at both device scales.
fn placement(dpr: u32) -> GlyphPlacement {
    GlyphPlacement {
        cell_column: 0,
        key: GlyphKey {
            face: 4,
            glyph_id: 1,
            instance: 0,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        origin_px: [40 * dpr as i32, 15 * dpr as i32],
        baseline_px: 15.0 * dpr as f32,
        size_px: 16.0 * dpr as f32,
        advance_px: 20 * dpr,
        ink_limit_px: 20 * dpr,
        cell_height_px: 20 * dpr,
        cell_span: 2,
        color: 0xffffffff,
        effect: false,
        color_glyph: false,
    }
}

/// Tall ink keeps the baseline; width and centering are uniform at both DPRs.
#[test]
fn device_fitting_table() {
    for dpr in [1, 2] {
        let p = placement(dpr);
        for (bounds, emoji, expected_scale, expected_x) in [
            ([0.0, -5.0, 10.0, 15.0], false, 1.0, 5.0),
            ([0.0, -10.0, 20.0, 30.0], false, 0.5, 5.0),
            ([-2.0, -5.0, 38.0, 15.0], false, 0.5, 1.0),
            ([0.0, 0.0, 10.0, 10.0], true, 1.8, 1.0),
        ] {
            let bounds = bounds.map(|value| value * dpr as f32);
            let fitted = fit(&p, bounds, emoji, 15.0 * dpr as f32);
            assert!((fitted.scale - expected_scale).abs() < 0.0001);
            assert!((fitted.offset[0] - expected_x * dpr as f32).abs() < 0.0001);
            if emoji {
                // The bottom and top leave exactly the 5% inset.
                let top = p.origin_px[1] as f32 + fitted.offset[1] - bounds[3] * fitted.scale;
                assert!((top - dpr as f32).abs() < 0.0001);
            } else {
                assert_eq!(fitted.offset[1], 0.0);
            }
            assert_eq!(p.origin_px, [40 * dpr as i32, 15 * dpr as i32]);
            assert_eq!(p.advance_px, 20 * dpr);
        }
    }
}
