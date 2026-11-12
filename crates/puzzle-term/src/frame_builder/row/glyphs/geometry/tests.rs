//! The extrema shortcut must preserve public hit geometry for every affine kind.
use super::*;

/// Compare against all four corners, including negative scales and cross terms.
#[test]
fn axis_aligned_and_affine_ink_unions_match_four_corner_reference() {
    for m in [
        [3.0, 0.0, 0.0, -2.0],
        [-3.0, 0.0, 0.0, 2.0],
        [3.0, 0.0, 0.5, -2.0],
        [0.0, 3.0, -2.0, 0.0],
        [0.0; 4],
    ] {
        let draw = puzzle_frame::SlugInstance {
            m,
            t: [3.125, -7.25],
            bbox: [-0.2, -0.7, 0.8, 0.3],
            glyph_loc: 0,
            band_info: 0,
            color: 0,
            paint: 0,
        };
        let mut actual = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        extend_slug(&mut actual.0, &mut actual.1, &draw);
        let mut expected = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        for x in [draw.bbox[0], draw.bbox[2]] {
            for y in [draw.bbox[1], draw.bbox[3]] {
                extend(&mut expected.0, &mut expected.1, point(&draw, x, y));
            }
        }
        assert_eq!(actual, expected, "matrix {m:?}");
    }
}
