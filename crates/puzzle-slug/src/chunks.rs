//! Device-bounded overlay partitions without flattening quadratic curves (§11.2).
//!
//! Chunks own disjoint pixel scissors and retain one pixel of geometry outside
//! each boundary. A pixel thus evaluates the original winding and antialiasing
//! window, never the artificial cut edge. Stroke expansion happens before this
//! pass: its round caps/joins survive in the overlapping geometry. Disjoint
//! scissors prevent translucent paints from blending twice across that overlap.
//!
//! The conversion tolerance is expressed in device pixels after the linear map.
//! Large zoom therefore cannot magnify an approximation chosen at a smaller size.
//! f32 atlas coordinates remain within a conservative range for reliable coverage.
//! Translation is supplied separately by the instance and does not affect cuts.
//!
//! A spatial split alone cannot guarantee a short band list: a small plot may
//! contain thousands of segments. Each candidate measures the actual band peak
//! before acceptance, then subdivides further when either constraint fails.
//! The final bands are retained alongside quadratics and packed without a second
//! membership pass. Restoration can reuse these CPU descriptors after eviction.
//! Band counts are checked on guarded geometry, including closure edges created by clipping.
use crate::QuadShape;
use kurbo::{Affine, BezPath, Rect};
/// Directed quadratic half-plane clipping used by recursive spatial subdivision.
mod clip;

/// One bounded outline plus the nonoverlapping rectangle it owns.
#[derive(Clone, Debug)]
pub struct Chunk {
    /// Quadratics and winding are preserved; coordinates are device pixels.
    pub path: BezPath,
    /// Exclusive pixel ownership before the instance translation is applied.
    pub owner: Rect,
    /// Validated band peak, useful for independent acceptance and diagnostics.
    pub peak: usize,
    /// Converted geometry is reused by atlas packing without a second path pass.
    pub shape: QuadShape,
    /// Validated 64-band lists are the exact lists eventually packed for this chunk.
    pub bands: crate::bands::BandSet,
}

/// Converts a logical path with a translation-free logical-to-device transform.
/// Returns errors for malformed input or density that cannot meet the band bound.
pub fn prepare(path: &BezPath, linear: Affine) -> Result<Vec<Chunk>, String> {
    // Validate coefficients before kurbo can propagate nonfinite curve coordinates.
    if !linear.as_coeffs().iter().all(|v| v.is_finite()) {
        return Err("overlay transform is nonfinite".into());
    }
    let transformed = linear * path;
    // Physical-space conversion makes the cubic tolerance independent of zoom.
    let shape = QuadShape::from_path(&transformed, 1.0 / 2048.0).map_err(|e| e.to_string())?;
    let bbox = shape.bbox.map(f64::from);
    // Beyond this bound f32 spacing would compete with fractional-pixel coverage.
    if bbox.iter().any(|v| v.abs() > 1_048_576.0) {
        return Err("overlay device coordinates exceed reliable f32 pixel precision".into());
    }
    let owner = Rect::new(
        // The outer guard protects the original contour's antialiased edge pixels.
        bbox[0].floor() - 1.0,
        bbox[1].floor() - 1.0,
        bbox[2].ceil() + 1.0,
        bbox[3].ceil() + 1.0,
    );
    // Later cuts operate on quadratics, preserving curves rather than flattening them.
    let mut result = Vec::new();
    split(shape, owner, 0, &mut result)?;
    Ok(result)
}

/// Tests both output constraints before accepting an overlapping contour partition.
fn split(shape: QuadShape, owner: Rect, depth: usize, out: &mut Vec<Chunk>) -> Result<(), String> {
    let width = f64::from(shape.bbox[2] - shape.bbox[0]);
    // Guarded contour bounds, not just pixel ownership, must obey the side limit.
    let height = f64::from(shape.bbox[3] - shape.bbox[1]);
    if width <= 256.0 && height <= 256.0 {
        // A spatially illegal parent must split regardless of its band counts.
        // Avoid searching counts for geometry that can never become a draw.
        if accept(owner, &shape, out)? {
            return Ok(());
        }
    }
    // A finite failure is preferable to an illegal GPU band or unbounded recursion.
    if depth >= 40 || out.len() >= 16_384 || (owner.width() <= 1.0 && owner.height() <= 1.0) {
        return Err("overlay density cannot fit 64 curves per band within a guarded pixel".into());
    }
    for child in children(&shape, owner) {
        // Retain full AA neighborhoods at internal seams; scissors own the pixels.
        if let Some(clipped) = clip::rectangle(&shape, child.inflate(1.0, 1.0))? {
            split(clipped, child, depth + 1, out)?;
        }
    }
    Ok(())
}

/// Constructs actual band lists only for spatially eligible candidate chunks.
fn accept(owner: Rect, shape: &QuadShape, out: &mut Vec<Chunk>) -> Result<bool, String> {
    // Every nonparallel curve enters at least one band. With sixteen candidate
    // bands, more than 16*64 eligible curves cannot satisfy the shader limit.
    // Reject this proven-impossible candidate before spending time on counts.
    for axis in 0..2 {
        if shape
            .curves
            .iter()
            .filter(|curve| !curve.flat_on(axis))
            .count()
            > 16 * 64
        {
            return Ok(false);
        }
    }
    // Count one feasible partition before the full ten-percent count search.
    // An unsuitable sixteen-band candidate splits; no discarded list is sorted.
    if !crate::bands::BandBuilder.overlay_fits(shape) {
        return Ok(false);
    }
    // Overlay geometry may use up to 64 axis bands. Sixteen suffice for this
    // partition policy; finer guarded chunks enforce the same per-band bound.
    let bands = crate::bands::BandBuilder
        .build_overlay(shape, 16)
        .map_err(|e| e.to_string())?;
    // Counts are inspected after selection, rather than estimated from curve density.
    let peak = bands
        .horizontal
        .iter()
        .chain(&bands.vertical)
        .map(Vec::len)
        .max()
        .unwrap_or(0);
    if peak > 64 {
        return Ok(false);
    }
    // Retain exact accepted lists; no conversion or count search repeats during packing.
    out.push(Chunk {
        path: clip::quadratics(shape),
        owner,
        peak,
        shape: shape.clone(),
        bands,
    });
    Ok(true)
}

/// Bisects exclusive pixel ownership along its longer integer-aligned side.
fn children(shape: &QuadShape, owner: Rect) -> [Rect; 2] {
    let axis = split_axis(shape, owner);
    // Split the longer side on an integer boundary to keep disjoint pixel ownership.
    let middle = if axis == 0 {
        ((owner.x0 + owner.x1) * 0.5).floor()
    } else {
        ((owner.y0 + owner.y1) * 0.5).floor()
    };
    let (a, b) = if axis == 0 {
        // Child rectangles meet at one shared exclusive edge, with no lost strip.
        (
            Rect::new(owner.x0, owner.y0, middle, owner.y1),
            Rect::new(middle, owner.y0, owner.x1, owner.y1),
        )
    } else {
        (
            Rect::new(owner.x0, owner.y0, owner.x1, middle),
            Rect::new(owner.x0, middle, owner.x1, owner.y1),
        )
    };
    [a, b]
}

/// Dense candidates choose the cut with fewer duplicated guarded curve memberships.
fn split_axis(shape: &QuadShape, owner: Rect) -> usize {
    let longer = usize::from(owner.height() > owner.width());
    // Oversized geometry first reduces the illegal side, independently of density.
    if shape.bbox[2] - shape.bbox[0] > 256.0 || shape.bbox[3] - shape.bbox[1] > 256.0 {
        return longer;
    }
    let mut scores = [usize::MAX; 2];
    for (axis, score) in scores.iter_mut().enumerate() {
        let (lo, hi) = if axis == 0 {
            (owner.x0, owner.x1)
        } else {
            (owner.y0, owner.y1)
        };
        if hi - lo <= 1.0 {
            continue;
        }
        let middle = ((lo + hi) * 0.5).floor();
        let mut counts = [0; 2];
        for curve in &shape.curves {
            // Control hulls conservatively count a curve in either child's AA guard.
            let (low, high) = curve.control_range(axis);
            counts[0] += usize::from(f64::from(low) <= middle + 1.0);
            counts[1] += usize::from(f64::from(high) >= middle - 1.0);
        }
        *score = counts[0].max(counts[1]);
    }
    if scores[0] == scores[1] {
        longer
    } else {
        usize::from(scores[1] < scores[0])
    }
}
