//! Oracle truth checks independent of GPU implementation (design §9.6).
// Test assertions report the exact sample position and observed coverage.
#![allow(
    clippy::expect_used,
    reason = "synthetic paths must prepare for oracle tests"
)]
use super::{RasterTransform, rasterize, sample};
use crate::{
    atlas::{FillRule, ShapeKey, SlugAtlas},
    synthetic::{SyntheticShape, rectangle},
};
use proptest::prelude::*;

/// Area of one axis-aligned pixel square intersected with a rectangle.
fn analytic_rectangle(at: [f32; 2], width: f32, bounds: [f32; 4]) -> f32 {
    let half = width * 0.5;
    let overlap_x = (at[0] + half).min(bounds[2]) - (at[0] - half).max(bounds[0]);
    let overlap_y = (at[1] + half).min(bounds[3]) - (at[1] - half).max(bounds[1]);
    (overlap_x.max(0.0) / width) * (overlap_y.max(0.0) / width)
}

/// Single-edge pixels are exact; two-edge corners use Slug's ray approximation.
#[test]
fn fractional_rectangle_classifies_pixel_coverage() {
    let path = rectangle(0.23, 0.18, 0.77, 0.82);
    let mut atlas = SlugAtlas::default();
    let handle = atlas
        .prepare(ShapeKey(1), &path, FillRule::NonZero, 0)
        .expect("rectangle");
    for x in 0..11 {
        for y in 0..11 {
            let at = [x as f32 * 0.1, y as f32 * 0.1];
            let actual = sample(&atlas, handle, at, [0.1, 0.1]).expect("sample");
            let truth = analytic_rectangle(at, 0.1, [0.23, 0.18, 0.77, 0.82]);
            let x = ((at[0] + 0.05).min(0.77) - (at[0] - 0.05).max(0.23)).clamp(0.0, 0.1);
            let y = ((at[1] + 0.05).min(0.82) - (at[1] - 0.05).max(0.18)).clamp(0.0, 0.1);
            let tolerance = if x > 0.0 && x < 0.1 && y > 0.0 && y < 0.1 {
                0.25 + 1e-6
            } else {
                1e-3
            };
            if x == 0.0 || y == 0.0 {
                assert_eq!(actual, 0.0, "outside pixel at {at:?}");
            }
            if x >= 0.1 - 1e-7 && y >= 0.1 - 1e-7 {
                assert_eq!(actual, 1.0, "inside pixel at {at:?}");
            }
            assert!(
                (actual - truth).abs() <= tolerance,
                "at {at:?}: Slug={actual}, analytic={truth}"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]
    /// A single straight edge crosses the pixel with exact area coverage.
    #[test]
    fn single_edge_pixels_match_fractional_area(x in -0.49f32..0.49) {
        let mut atlas = SlugAtlas::default();
        let handle = atlas.prepare(ShapeKey(1),
            &rectangle(x.into(), -2.0, 2.0, 2.0), FillRule::NonZero, 0)
            .expect("single edge path");
        let actual = sample(&atlas, handle, [0.0, 0.0], [1.0, 1.0])
            .expect("single edge sample");
        let exact = 0.5 - x;
        prop_assert!((actual - exact).abs() <= 1e-3,
            "vertical edge x={x}: Slug={actual}, exact={exact}");
    }

    /// Random fractional corners stay bounded despite nonmonotonic weighting.
    #[test]
    fn fractional_corners_stay_within_reference_bound(
        x in -0.49f32..0.49, y in -0.49f32..0.49,
        dx in 0.0f32..0.25, dy in 0.0f32..0.25,
    ) {
        let grown_x = (x - dx).max(-0.49);
        let grown_y = (y - dy).max(-0.49);
        let mut atlas = SlugAtlas::default();
        let small = atlas.prepare(ShapeKey(1), &rectangle(x.into(), y.into(), 2.0, 2.0),
            FillRule::NonZero, 0).expect("small");
        let large = atlas.prepare(ShapeKey(2), &rectangle(grown_x.into(), grown_y.into(), 2.0, 2.0),
            FillRule::NonZero, 0).expect("large");
        let a = sample(&atlas, small, [0.0, 0.0], [1.0, 1.0]).expect("small sample");
        let b = sample(&atlas, large, [0.0, 0.0], [1.0, 1.0]).expect("large sample");
        let exact = (0.5 - x) * (0.5 - y);
        let grown_exact = (0.5 - grown_x) * (0.5 - grown_y);
        prop_assert!((a - exact).abs() <= 0.25 + 1e-6,
            "corner ({x}, {y}): Slug={a}, analytic={exact}");
        prop_assert!((0.0..=1.0).contains(&a));
        prop_assert!((b - grown_exact).abs() <= 0.25 + 1e-6,
            "grown corner ({grown_x}, {grown_y}): Slug={b}, analytic={grown_exact}");
        prop_assert!((0.0..=1.0).contains(&b));
    }
}

/// A rectangle can grow while the weighted rays lose coverage (design §9.6).
#[test]
fn nonmonotonic_corner_still_respects_area_error_bound() {
    let x = -0.451_039_02_f32;
    let y = -0.120_586_5_f32;
    let mut atlas = SlugAtlas::default();
    for (key, lower_x) in [(1, x), (2, -0.49)] {
        let handle = atlas
            .prepare(
                ShapeKey(key),
                &rectangle(lower_x.into(), y.into(), 2.0, 2.0),
                FillRule::NonZero,
                0,
            )
            .expect("dip rectangle");
        let actual = sample(&atlas, handle, [0.0, 0.0], [1.0, 1.0]).expect("dip sample");
        let exact = (0.5 - lower_x) * (0.5 - y);
        assert!((0.0..=1.0).contains(&actual));
        assert!(
            (actual - exact).abs() <= 0.25 + 1e-6,
            "corner ({lower_x}, {y}): Slug={actual}, exact={exact}"
        );
    }
}

/// The original ±0.10 counterexample remains an exact regression fixture.
#[test]
fn corner_tolerance_counterexample_matches_reference() {
    let x = -0.210_975_45_f32;
    let mut atlas = SlugAtlas::default();
    let handle = atlas
        .prepare(
            ShapeKey(1),
            &rectangle(x.into(), 0.0, 2.0, 2.0),
            FillRule::NonZero,
            0,
        )
        .expect("corner path");
    let actual = sample(&atlas, handle, [0.0, 0.0], [1.0, 1.0]).expect("corner sample");
    let exact = (0.5 - x) * 0.5;
    assert!((actual - 0.5).abs() <= 1e-6, "counterexample Slug={actual}");
    assert!((actual - exact).abs() > 0.10);
    assert!((actual - exact).abs() <= 0.25 + 1e-6);
}

/// A corner at the pixel centre reaches the two-ray 0.25 error maximum.
#[test]
fn pixel_centered_corner_has_maximum_reference_error() {
    let mut atlas = SlugAtlas::default();
    let handle = atlas
        .prepare(
            ShapeKey(1),
            &rectangle(0.0, 0.0, 2.0, 2.0),
            FillRule::NonZero,
            0,
        )
        .expect("centered corner");
    let actual = sample(&atlas, handle, [0.0, 0.0], [1.0, 1.0]).expect("center sample");
    // Signed-zero handling in the original root code excludes crossings
    // precisely at both edges, making this the 0.25-error boundary case.
    assert!(actual.abs() <= 1e-6, "centered corner Slug={actual}");
    assert!((actual - 0.25).abs() <= 0.25 + 1e-6);
}

/// A 90-degree turn preserves coverage for circles and centered squares.
#[test]
fn circle_and_square_have_ninety_degree_symmetry() {
    for kind in [SyntheticShape::Circle, SyntheticShape::Rectangle] {
        let mut atlas = SlugAtlas::default();
        let handle = atlas
            .prepare(ShapeKey(1), &kind.path(), FillRule::NonZero, 0)
            .expect("symmetric path");
        for x in 0..21 {
            for y in 0..21 {
                let at = [x as f32 * 0.05, y as f32 * 0.05];
                let rotated = [1.0 - at[1], at[0]];
                let a = sample(&atlas, handle, at, [0.025, 0.025]).expect("first");
                let b = sample(&atlas, handle, rotated, [0.025, 0.025]).expect("rotated");
                assert!((a - b).abs() <= 1e-3, "{kind:?} {at:?}: {a} vs {b}");
            }
        }
    }
}

/// Winding two fills under nonzero but is empty under even-odd.
#[test]
fn overlapping_contours_obey_both_fill_rules() {
    let path = SyntheticShape::DoubleWinding.path();
    let mut atlas = SlugAtlas::default();
    let nonzero = atlas
        .prepare(ShapeKey(1), &path, FillRule::NonZero, 0)
        .expect("nonzero");
    let evenodd = atlas
        .prepare(ShapeKey(2), &path, FillRule::EvenOdd, 0)
        .expect("even-odd");
    let center_nonzero = sample(&atlas, nonzero, [0.5, 0.5], [0.01, 0.01]).expect("center");
    let center_evenodd = sample(&atlas, evenodd, [0.5, 0.5], [0.01, 0.01]).expect("center");
    assert!((center_nonzero - 1.0).abs() < 1e-5);
    assert!(center_evenodd.abs() < 1e-5);
    let ring_nonzero = sample(&atlas, nonzero, [0.15, 0.5], [0.01, 0.01]).expect("ring");
    let ring_evenodd = sample(&atlas, evenodd, [0.15, 0.5], [0.01, 0.01]).expect("ring");
    assert!((ring_nonzero - 1.0).abs() < 1e-5);
    assert!((ring_evenodd - 1.0).abs() < 1e-5);
}

/// The oracle emits deterministic device pixels for the debugging PNG.
#[test]
fn synthetic_raster_has_visible_ink() {
    let mut atlas = SlugAtlas::default();
    let handle = atlas
        .prepare(
            ShapeKey(1),
            &SyntheticShape::TypicalGlyph.path(),
            FillRule::NonZero,
            0,
        )
        .expect("glyph");
    let image = rasterize(
        &atlas,
        handle,
        32,
        32,
        RasterTransform {
            scale: [24.0, -24.0],
            translate: [4.0, 28.0],
        },
    )
    .expect("image");
    assert_eq!(image.pixels.len(), 32 * 32 * 4);
    assert!(image.pixels.chunks_exact(4).any(|pixel| pixel[0] > 0));
    assert!(image.pixels.chunks_exact(4).any(|pixel| pixel[0] == 0));
}
