//! Path conversion tests for closure and kurbo's cubic subdivision.
// Test assertions identify precisely which constructed outline broke.
#![allow(
    clippy::expect_used,
    reason = "shape fixtures should explain conversion failures"
)]
use super::{DEFAULT_TOLERANCE, QuadShape, ShapeError};
use kurbo::{BezPath, CubicBez, ParamCurve, Point};
use proptest::prelude::*;

/// Open contours close explicitly, and lines duplicate their endpoints.
#[test]
fn open_rectangle_closes_and_encodes_lines() {
    let mut path = BezPath::new();
    path.move_to((0.0, 0.0));
    path.line_to((1.0, 0.0));
    path.line_to((1.0, 1.0));
    path.line_to((0.0, 1.0));
    let shape = QuadShape::from_path(&path, DEFAULT_TOLERANCE).expect("rectangle");
    assert_eq!(shape.curves.len(), 4);
    assert_eq!(shape.contours.len(), 1);
    assert_eq!(shape.curves[0].p2, shape.curves[0].p3);
    assert_eq!(shape.curves[3].p3, shape.curves[0].p1);
}

/// Font-supplied nonfinite coordinates cannot enter a float texture.
#[test]
fn nonfinite_path_returns_error() {
    let mut path = BezPath::new();
    path.move_to((0.0, 0.0));
    path.line_to((f64::NAN, 1.0));
    assert_eq!(
        QuadShape::from_path(&path, DEFAULT_TOLERANCE).err(),
        Some(ShapeError::NonFinite)
    );
}

proptest! {
    /// Kurbo's cubic-to-quadratic output remains continuous after conversion.
    #[test]
    fn random_cubic_uses_kurbo_subdivision(
        a in (-1.0_f64..1.0, -1.0_f64..1.0),
        b in (-1.0_f64..1.0, -1.0_f64..1.0),
    ) {
        let cubic = CubicBez::new(Point::new(0.0, 0.0), Point::new(a.0, a.1),
            Point::new(b.0, b.1), Point::new(1.0, 1.0));
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.curve_to(a, b, (1.0, 1.0));
        path.close_path();
        let shape = QuadShape::from_path(&path, DEFAULT_TOLERANCE).expect("finite cubic");
        let reference = cubic.to_quads(DEFAULT_TOLERANCE).collect::<Vec<_>>();
        prop_assert!(shape.curves.len() >= reference.len());
        for (curve, (t0, t1, quad)) in shape.curves.iter().zip(reference) {
            // The selected midpoint checks the declared em-space error and
            // catches a wrong start point or missing subdivision segment.
            let midpoint = cubic.eval((t0 + t1) * 0.5);
            let estimate = quad.eval(0.5);
            prop_assert!((midpoint - estimate).hypot() <= DEFAULT_TOLERANCE * 2.0);
            prop_assert!((f64::from(curve.p3[0]) - quad.p2.x).abs() < 1e-6);
            prop_assert!((f64::from(curve.p3[1]) - quad.p2.y).abs() < 1e-6);
        }
    }
}
