//! Random shapes verify ray membership, exclusions and early-exit ordering.
// Test failures include geometry from proptest's smallest counterexample.
#![allow(
    clippy::expect_used,
    reason = "generated finite shapes should build bands"
)]
use super::{BAND_EPSILON, BandBuilder, peak_count};
use crate::shape::{DEFAULT_TOLERANCE, QuadCurve, QuadShape};
use kurbo::BezPath;
use proptest::prelude::*;

/// One generated contour with alternating control heights and widths.
fn shape_from_points(points: &[(f64, f64)]) -> QuadShape {
    let mut path = BezPath::new();
    path.move_to((0.0, 0.0));
    for pair in points.chunks_exact(2) {
        path.quad_to(pair[0], pair[1]);
    }
    path.close_path();
    QuadShape::from_path(&path, DEFAULT_TOLERANCE).expect("finite shape")
}

/// Checks all bands independently of the builder's candidate-count policy.
fn assert_membership(shape: &QuadShape, lists: &[Vec<usize>], axis: usize) {
    let min = shape.bbox[axis];
    let span = (shape.bbox[axis + 2] - min) / lists.len() as f32;
    for (band, members) in lists.iter().enumerate() {
        let low = min + band as f32 * span - BAND_EPSILON;
        let high = min + (band + 1) as f32 * span + BAND_EPSILON;
        for (index, curve) in shape.curves.iter().enumerate() {
            let (a, b) = curve.control_range(axis);
            let expected = !curve.flat_on(axis) && b >= low && a <= high;
            assert_eq!(members.contains(&index), expected);
        }
        // Preserve the exact stable order too: tied control maxima must retain
        // contour order, and acceleration must not alter packed reference texels.
        let mut expected: Vec<_> = shape
            .curves
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let (a, b) = c.control_range(axis);
                (!c.flat_on(axis) && b >= low && a <= high).then_some(i)
            })
            .collect();
        expected.sort_by(|a, b| {
            shape.curves[*b]
                .control_range(1 - axis)
                .1
                .total_cmp(&shape.curves[*a].control_range(1 - axis).1)
        });
        assert_eq!(members, &expected);
        // Other-axis maxima must be nonincreasing, or Slug's early break
        // can skip a later curve that still reaches this pixel.
        let other = 1 - axis;
        for pair in members.windows(2) {
            assert!(
                shape.curves[pair[0]].control_range(other).1
                    >= shape.curves[pair[1]].control_range(other).1
            );
        }
    }
}

proptest! {
    /// Random quadratic contours preserve every conservative overlap.
    #[test]
    fn random_bands_cover_and_sort_curves(
        points in prop::collection::vec(
            (-1.0_f64..1.0, -1.0_f64..1.0), 4..32,
        ).prop_filter("paired controls", |items| items.len() % 2 == 0),
    ) {
        let shape = shape_from_points(&points);
        let bands = BandBuilder.build(&shape, 16).expect("nonempty bounds");
        // The corrected-index search must select and pack the identical lists.
        let accelerated = BandBuilder.build_overlay(&shape, 16).expect("overlay lists");
        prop_assert_eq!(&accelerated.horizontal, &bands.horizontal);
        prop_assert_eq!(&accelerated.vertical, &bands.vertical);
        assert_membership(&shape, &bands.horizontal, 1);
        assert_membership(&shape, &bands.vertical, 0);
        // Candidate peak acceleration must agree with independently enumerated
        // memberships for arbitrary f32 geometry, not just the selected count.
        for axis in 0..2 {
            for count in 1..=16 {
                let min=shape.bbox[axis];
                let span=(shape.bbox[axis+2]-min)/count as f32;
                let peak=(0..count).map(|b| shape.curves.iter().filter(|c| {
                    let (low,high)=c.control_range(axis);
                    !c.flat_on(axis) && high>=min+b as f32*span-BAND_EPSILON && low<=min+(b+1) as f32*span+BAND_EPSILON
                }).count()).max().expect("nonzero band count");
                prop_assert_eq!(peak_count(&shape, axis, count), peak);
            }
        }
        prop_assert!((1..=16).contains(&bands.horizontal.len()));
        prop_assert!((1..=16).contains(&bands.vertical.len()));
    }
}

/// Axis-parallel lines are excluded only from their parallel ray lists.
#[test]
fn rectangle_excludes_flat_edges() {
    let path = crate::synthetic::rectangle(0.0, 0.0, 1.0, 1.0);
    let shape = QuadShape::from_path(&path, DEFAULT_TOLERANCE).expect("rectangle");
    let bands = BandBuilder.build(&shape, 16).expect("bands");
    assert!(
        bands
            .horizontal
            .iter()
            .all(|list| !list.contains(&0) && !list.contains(&2))
    );
    assert!(
        bands
            .vertical
            .iter()
            .all(|list| !list.contains(&1) && !list.contains(&3))
    );
}

/// Candidate selection must retain exact inclusive f32 overlap counts after acceleration.
#[test]
fn interval_peak_matches_inclusive_membership() {
    for points in [
        [[0.0, 0.0], [0.5, 1.0], [1.0, 0.0]],
        [[0.0, 0.0], [BAND_EPSILON, 0.5], [1.0, 1.0]],
        [[0.25, 0.25], [0.25, 0.25], [0.25, 0.25]],
    ] {
        let curve = QuadCurve {
            p1: points[0],
            p2: points[1],
            p3: points[2],
        };
        let shape = QuadShape {
            curves: vec![curve; 37],
            contours: std::iter::once(0..37).collect(),
            bbox: [0.0, 0.0, 1.0, 1.0],
        };
        for axis in 0..2 {
            for count in 1..=256 {
                let span = 1.0 / count as f32;
                let expected = (0..count)
                    .map(|band| {
                        let start = band as f32 * span - BAND_EPSILON;
                        let end = (band + 1) as f32 * span + BAND_EPSILON;
                        shape
                            .curves
                            .iter()
                            .filter(|c| {
                                let (low, high) = c.control_range(axis);
                                !c.flat_on(axis) && high >= start && low <= end
                            })
                            .count()
                    })
                    .max()
                    .expect("there is at least one candidate band");
                assert_eq!(peak_count(&shape, axis, count), expected);
            }
        }
    }
}
