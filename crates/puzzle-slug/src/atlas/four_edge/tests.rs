//! Exact fast/general comparisons cover tiny COLR-like outlines before packing.
//! Random rectangles, rotations, arbitrary quads and collapsed edges exercise
//! the four-curve branch. Every selected band list and texel must be identical.
//! Row-end starting cursors also verify that identical data keeps C1/B1/B3 placement.
#![allow(clippy::expect_used, reason = "generated finite outlines must pack")]
use super::SlugAtlas;
use crate::{
    bands::BandBuilder,
    shape::{DEFAULT_TOLERANCE, QuadShape},
};
use kurbo::BezPath;
use proptest::prelude::*;

/// Builds a closed contour, allowing conversion to drop exactly degenerate edges.
fn quad(points: &[(f64, f64); 4]) -> QuadShape {
    let mut path = BezPath::new();
    path.move_to(points[0]);
    for point in &points[1..] {
        path.line_to(*point);
    }
    path.close_path();
    QuadShape::from_path(&path, DEFAULT_TOLERANCE).expect("finite quad")
}

/// Requires exact selected lists and bit-identical curve/band storage for one shape.
fn compare(shape: &QuadShape, start: usize, limit: usize) {
    assert!(shape.curves.len() <= 4);
    let fast = BandBuilder.build(shape, limit);
    let general = BandBuilder.build_general(shape, limit);
    let (Ok(fast), Ok(general)) = (&fast, &general) else {
        // Collapsed bounds must be rejected identically, never packed differently.
        assert_eq!(fast.err(), general.err());
        return;
    };
    assert_eq!(fast.horizontal, general.horizontal);
    assert_eq!(fast.vertical, general.vertical);
    let mut packed = SlugAtlas::default();
    let mut reference = SlugAtlas::default();
    // Identical row-edge starts exercise padding and shared-endpoint addressing too.
    for atlas in [&mut packed, &mut reference] {
        atlas.curves.cursor = start;
        atlas.bands.cursor = start;
    }
    let a = packed.pack_transaction(shape, fast).expect("fast bands");
    let b = reference
        .pack_transaction(shape, general)
        .expect("general bands");
    assert_eq!(a, b);
    assert_eq!(packed.band_texels(), reference.band_texels());
    // Compare float bits rather than a numeric tolerance, including signed zero.
    let bits = |atlas: &SlugAtlas| {
        atlas
            .curve_texels()
            .iter()
            .map(|texel| texel.map(f32::to_bits))
            .collect::<Vec<_>>()
    };
    assert_eq!(bits(&packed), bits(&reference));
}

proptest! {
    /// Each trial includes regular, rotated, arbitrary and collapsed four-edge paths.
    #[test]
    fn four_edge_fast_and_general_pack_identically(
        x in -1.0_f64..1.0, y in -1.0_f64..1.0,
        w in 0.01_f64..2.0, h in 0.01_f64..2.0,
        angle in -std::f64::consts::PI..std::f64::consts::PI,
        arbitrary in prop::array::uniform4((-1.0_f64..1.0, -1.0_f64..1.0)),
        start in 4088_usize..4096, limit in 1_usize..257,
    ) {
        let rectangle = [(x,y), (x+w,y), (x+w,y+h), (x,y+h)];
        let (sin, cos) = angle.sin_cos();
        // Rotation removes axis-parallel exclusions without changing edge count.
        let rotated = rectangle.map(|(a,b)| (a*cos-b*sin, a*sin+b*cos));
        let mut collapsed = rotated;
        collapsed[1] = collapsed[0];
        let mut collinear = rotated;
        // A middle point on a diagonal also exercises coincident ray boundaries.
        collinear[1] = ((rotated[0].0+rotated[2].0)/2.0,
                        (rotated[0].1+rotated[2].1)/2.0);
        for points in [rectangle, rotated, arbitrary, collapsed, collinear] {
            compare(&quad(&points), start, limit);
        }
    }
}
