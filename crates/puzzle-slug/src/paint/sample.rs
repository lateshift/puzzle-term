//! OpenType color-line evaluation, independent of GPU interpolation (§9.6).
//!
//! Geometry produces a normalized offset; extension and interpolation then
//! operate on premultiplied sRGB. No linear-light conversion belongs here.
//!
//! A normalized linear color line projects onto a vector. A radial color
//! line solves a family of circles, retaining only nonnegative-radius roots.
//! A sweep line converts a root-relative point into clockwise degrees and
//! maps that angle between the normalized start/end values from skrifa.
//! Extension follows the actual first and last stops: pad holds the endpoint,
//! repeat wraps one interval, and reflect reverses every other interval.
//! Duplicate offsets are deliberate hard color boundaries. Below a duplicate
//! offset the first stop at that position is selected; at/above it the last
//! one wins. A nonpositive repeated interval paints nothing, avoiding division
//! by zero while preserving OpenType's specified degenerate-line behavior.
//! Sampling here returns premultiplied RGBA; the oracle multiplies it by the
//! Slug coverage of the leaf and each enclosing clip before source-over blend.
use super::{Brush, Extend, Paint};
/// Samples a brush after applying its root-to-local affine map.
pub(super) fn evaluate(paint: &Paint, at: [f32; 2]) -> [f32; 4] {
    // Root coordinates stay stable when the brush rotates or shears inside a clip.
    let m = paint.inverse;
    let p = [
        m[0] * at[0] + m[2] * at[1] + m[4],
        m[1] * at[0] + m[3] * at[1] + m[5],
    ];
    // Each geometry variant reduces to the same normalized color-line offset.
    let offset = match paint.brush {
        Brush::Solid(color) => return color,
        Brush::Linear { p0, p1 } => {
            // Projection, not glyph width, determines the linear interpolation.
            let d = [p1[0] - p0[0], p1[1] - p0[1]];
            let norm = d[0] * d[0] + d[1] * d[1];
            // OpenType specifies an empty result for a degenerate color line.
            if norm == 0.0 {
                return [0.0; 4];
            }
            ((p[0] - p0[0]) * d[0] + (p[1] - p0[1]) * d[1]) / norm
        }
        Brush::Radial { c0, c1, radii } => {
            // No valid circle means transparent ink outside the radial cone.
            let Some(t) = radial(p, c0, c1, radii) else {
                return [0.0; 4];
            };
            t
        }
        Brush::Sweep { center, angles } => {
            // Font coordinates are y-up; COLR sweep angles increase clockwise.
            let angle = (-(p[1] - center[1]))
                .atan2(p[0] - center[0])
                .to_degrees()
                .rem_euclid(360.0);
            // Angles outside one turn remain meaningful for the color interval.
            let span = angles[1] - angles[0];
            if span == 0.0 {
                return [0.0; 4];
            }
            (angle - angles[0]) / span
        }
    };
    color_line(paint, offset)
}
/// Solves the circle family, choosing the largest root with positive radius.
fn radial(p: [f32; 2], c0: [f32; 2], c1: [f32; 2], r: [f32; 2]) -> Option<f32> {
    // Subtract the starting circle center before expanding its squared distance.
    let q = [p[0] - c0[0], p[1] - c0[1]];
    let d = [c1[0] - c0[0], c1[1] - c0[1]];
    // Radius changes linearly with offset, just like the center position.
    let dr = r[1] - r[0];
    // |q - t*d|² = (r0 + t*dr)² expands to this quadratic.
    let a = d[0] * d[0] + d[1] * d[1] - dr * dr;
    let b = -2.0 * (q[0] * d[0] + q[1] * d[1] + r[0] * dr);
    let c = q[0] * q[0] + q[1] * q[1] - r[0] * r[0];
    // A nearly linear cone equation has one finite root rather than two.
    if a.abs() < 1e-12 {
        if b.abs() < 1e-12 {
            return None;
        }
        let t = -c / b;
        return (r[0] + t * dr >= 0.0).then_some(t);
    }
    // A negative discriminant means the point intersects no circle in the family.
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    // Keep both roots until the physical nonnegative-radius condition is applied.
    let root = disc.sqrt();
    let t0 = (-b - root) / (2.0 * a);
    let t1 = (-b + root) / (2.0 * a);
    // Negative-radius portions of the cone are transparent, not reflected.
    [t0, t1]
        .into_iter()
        .filter(|t| r[0] + t * dr >= 0.0)
        .max_by(f32::total_cmp)
}
/// Applies extension and interpolates adjacent stops, preserving hard edges.
fn color_line(paint: &Paint, mut t: f32) -> [f32; 4] {
    // Empty color lines are transparent; a single stop has constant color.
    let Some(first) = paint.stops.first() else {
        return [0.0; 4];
    };
    let Some(last) = paint.stops.last() else {
        return [0.0; 4];
    };
    if paint.stops.len() == 1 {
        return first.color;
    }
    // Stop extrema, not implicit zero/one endpoints, define extension cycles.
    let span = last.offset - first.offset;
    if span <= 0.0 && paint.extend != Extend::Pad {
        return [0.0; 4];
    }
    if span > 0.0 {
        // Euclidean modulo retains the right cycle for negative offsets.
        let u = (t - first.offset) / span;
        t = first.offset
            + span
                * match paint.extend {
                    Extend::Pad => u,
                    Extend::Repeat => u.rem_euclid(1.0),
                    Extend::Reflect => 1.0 - (u.rem_euclid(2.0) - 1.0).abs(),
                };
    }
    // Padding below the first offset takes the first authored color.
    if t < first.offset {
        return first.color;
    }
    // Exact duplicate offsets select the last stop at that position.
    let mut before = first;
    for after in &paint.stops[1..] {
        // Strict comparison skips equal offsets, implementing a right-hand jump.
        if t < after.offset {
            let f = (t - before.offset) / (after.offset - before.offset);
            return std::array::from_fn(|i| {
                before.color[i] + f * (after.color[i] - before.color[i])
            });
        }
        before = after;
    }
    // Padding above the final stop holds its last authored color.
    last.color
}
