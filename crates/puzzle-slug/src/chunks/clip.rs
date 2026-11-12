//! Winding-preserving quadratic clipping against a convex rectangle.
//!
//! Each half-plane pass keeps directed curve intervals inside that plane.
//! Gaps join along the plane boundary, just as polygon clipping does. Roots
//! split a quadratic exactly; no sampled polyline can change its curvature.
//!
//! Contours keep their original orientation. An inner clockwise contour remains
//! a hole after each half-plane pass, including where its boundary is connected
//! along a cut edge. Source winding therefore survives spatial subdivision.
//! Connection edges lie outside a chunk's owned pixel region because the caller
//! clips to a guarded rectangle. They close contours without introducing seams.
//! Each pass validates the resulting quadratic shape before the next cut.
use crate::QuadShape;
use kurbo::{BezPath, ParamCurve, Point, QuadBez, Rect};
/// Direct quadratic output avoids path round trips at each clipping plane.
mod output;

/// Reconstructs closed kurbo contours directly from stored quadratic commands.
pub(super) fn quadratics(shape: &QuadShape) -> BezPath {
    // Closed contours already carry shared endpoints in atlas working precision.
    let mut path = BezPath::new();
    for contour in &shape.contours {
        let curves = &shape.curves[contour.clone()];
        if let Some(first) = curves.first() {
            // Start once per contour so separate holes are never joined together.
            path.move_to(point(first.p1));
            for curve in curves {
                // Preserve controls verbatim; another cubic approximation is unnecessary.
                path.quad_to(point(curve.p2), point(curve.p3));
            }
            path.close_path();
        }
    }
    path
}

/// Applies four convex half-planes, retaining boundary connections between exits.
pub(super) fn rectangle(shape: &QuadShape, rect: Rect) -> Result<Option<QuadShape>, String> {
    // Successive convex half-planes preserve the intersection of all four bounds.
    // The source owner is unchanged so sibling chunks can cut the same original contours.
    let mut source = shape.clone();
    // Each intermediate owns its contours, so no path borrow outlives a conversion.
    for (axis, edge, above) in [
        (0, rect.x0, true),
        (0, rect.x1, false),
        (1, rect.y0, true),
        (1, rect.y1, false),
    ] {
        // Existing guarded bounds often satisfy three of the child's four planes.
        // Skipping them avoids reconverting every unaffected curve at each depth.
        let bound = source.bbox[axis + if above { 0 } else { 2 }];
        if (above && f64::from(bound) >= edge) || (!above && f64::from(bound) <= edge) {
            continue;
        }
        // Cut only the currently surviving intervals, preserving original directed winding.
        let next = half_plane(&source, axis, edge, above);
        // An empty intermediate intersection stays empty under later planes.
        source = match next {
            Ok(next) => next,
            Err(crate::ShapeError::Empty) => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
    }
    // Tight final bounds avoid wasting bands on empty corners of a guarded cut.
    match QuadShape::from_curves(source.curves, source.contours) {
        Ok(shape) => Ok(Some(shape)),
        Err(crate::ShapeError::Empty) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// Keeps original contour orientation, including holes and disconnected lobes.
fn half_plane(
    shape: &QuadShape,
    axis: usize,
    edge: f64,
    above: bool,
) -> Result<QuadShape, crate::ShapeError> {
    let mut out = output::Output::default();
    for contour in &shape.contours {
        for curve in &shape.curves[contour.clone()] {
            // Convex control bounds classify most curves without root solving.
            let (low, high) = curve.control_range(axis);
            if (above && f64::from(high) < edge) || (!above && f64::from(low) > edge) {
                continue;
            }
            if (above && f64::from(low) >= edge) || (!above && f64::from(high) <= edge) {
                // Preserve controls verbatim when the entire curve lies inside.
                out.push(*curve);
                continue;
            }
            // Mixed hulls need exact roots rather than classifying only their endpoints.
            let quad = QuadBez::new(point(curve.p1), point(curve.p2), point(curve.p3));
            let (cuts, count) = cuts(quad, axis, edge, above);
            for pair in cuts[..=count].windows(2) {
                // Between consecutive roots the side cannot change, so one midpoint classifies it.
                let p = quad.eval((pair[0] + pair[1]) * 0.5);
                let value = if axis == 0 { p.x } else { p.y };
                if (above && value < edge) || (!above && value > edge) {
                    continue;
                }
                // Subsegment retains the curve itself rather than flattening into sampled lines.
                let mut part = quad.subsegment(pair[0]..pair[1]);
                // Crossings are pinned to the plane, preserving parallel connections.
                if pair[0] > 0.0 {
                    snap(&mut part.p0, axis, edge);
                }
                if pair[1] < 1.0 {
                    snap(&mut part.p2, axis, edge);
                }
                // Convert once to the existing curve-texture precision after clipping.
                let pixel = |p: Point| [p.x as f32, p.y as f32];
                out.push(crate::shape::QuadCurve {
                    p1: pixel(part.p0),
                    p2: pixel(part.p1),
                    p3: pixel(part.p2),
                });
            }
        }
        // Each hole closes separately, keeping the source contour's winding.
        out.close();
    }
    finish_plane(out, shape.bbox, axis, edge, above)
}

/// Retains conservative source bounds until the final tight-extrema computation.
/// `bbox` already encloses every contour before this half-plane pass.
/// Only the accepted side replaces one endpoint; the other axis remains valid.
/// The final rectangle pass recomputes true quadratic extrema for band efficiency.
/// Intermediate bounds therefore trade no correctness for avoiding repeated solves.
fn finish_plane(
    mut_output: output::Output,
    mut bbox: [f32; 4],
    axis: usize,
    edge: f64,
    above: bool,
) -> Result<QuadShape, crate::ShapeError> {
    // Intersecting valid source bounds with a plane encloses every surviving interval.
    // No unchanged quadratic needs its extrema solved again for the next child.
    let bound = axis + if above { 0 } else { 2 };
    bbox[bound] = if above {
        bbox[bound].max(edge as f32)
    } else {
        bbox[bound].min(edge as f32)
    };
    mut_output.finish(bbox)
}

/// Converts texture precision back into kurbo's geometric working precision.
fn point(p: [f32; 2]) -> Point {
    // Promotion adds working precision for root solving without changing stored geometry.
    Point::new(f64::from(p[0]), f64::from(p[1]))
}

/// Pins a split endpoint to the cutting plane without perturbing its other axis.
fn snap(point: &mut Point, axis: usize, edge: f64) {
    if axis == 0 {
        point.x = edge;
    } else {
        point.y = edge;
    }
}

/// Splits the quadratic parameter interval only where it crosses the cutting plane.
fn cuts(quad: QuadBez, axis: usize, edge: f64, above: bool) -> ([f64; 4], usize) {
    let coord = |p: Point| if axis == 0 { p.x } else { p.y };
    let (p0, p1, p2) = (coord(quad.p0), coord(quad.p1), coord(quad.p2));
    // A convex control hull entirely on one side cannot cross the plane.
    // The caller's midpoint rejects outside curves; inside curves keep [0,1].
    let low = p0.min(p1).min(p2);
    let high = p0.max(p1).max(p2);
    if (above && low >= edge) || (!above && high <= edge) || high < edge || low > edge {
        return ([0.0, 1.0, 1.0, 1.0], 1);
    }
    // Expanding q(t)-edge yields constant, linear and quadratic coefficients.
    // Only roots strictly inside the parameter interval create subdivisions.
    let roots = kurbo::common::solve_quadratic(p0 - edge, 2.0 * (p1 - p0), p0 - 2.0 * p1 + p2);
    let mut cuts = [0.0, 1.0, 1.0, 1.0];
    // A quadratic has at most two interior roots; four slots include endpoints.
    let mut count = 1;
    for root in roots {
        if root > 0.0 && root < 1.0 {
            // Endpoint roots already have cuts; duplicate endpoint pieces add no area.
            cuts[count] = root;
            count += 1;
        }
    }
    cuts[count] = 1.0;
    cuts[..=count].sort_by(f64::total_cmp);
    // The count indexes the final endpoint, so callers include it in their windows.
    (cuts, count)
}
