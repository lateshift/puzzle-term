//! Equal-thickness ray bands for Slug's early-exit shader loops (§9.3).
//!
//! Horizontal rays need curves whose control hull overlaps a y strip;
//! vertical rays use x strips. Control hulls are conservative even when a
//! quadratic bends away from its middle point. An overlap of 1/1024 em
//! protects curves exactly on band borders from floating-point rounding.
//!
//! Slug casts both a horizontal and a vertical ray per pixel. A horizontal
//! band is indexed by the pixel's y coordinate and lists curves that may
//! cross that horizontal ray; a vertical band is indexed by x. The shader
//! can stop scanning a sorted list once all remaining curves sit behind its
//! half-pixel coverage window. Sorting is thus required for correctness.
//!
//! We choose counts by the largest list, because the slowest pixel controls
//! worst-case shader latency. Trying extra bands requires only temporary
//! membership counts; constructing and sorting all candidate lists would
//! spend preparation time on options we discard. The selected count then
//! gets its actual lists once. A reduction below ten percent does not repay
//! the added header texels and index arithmetic (design §9.3).
use crate::shape::QuadShape;

/// Reference README's overlap on each side of an em-space band.
pub const BAND_EPSILON: f32 = 1.0 / 1024.0;

/// One shape's horizontal and vertical lists in ray traversal order.
#[derive(Clone, Debug)]
pub struct BandSet {
    /// Horizontal-ray lists indexed by the sample's y band.
    pub horizontal: Vec<Vec<usize>>,
    /// Vertical-ray lists indexed by the sample's x band.
    pub vertical: Vec<Vec<usize>>,
}

/// Invalid band limits cannot be represented in eight-bit instance maxima.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandError {
    /// Each axis needs one through 256 equal-thickness strips.
    InvalidLimit,
    /// A zero-sized outline has no meaningful strip transform.
    EmptyBounds,
}

/// Human-readable band construction errors.
impl std::fmt::Display for BandError {
    /// Identifies the invalid request before packing starts.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::InvalidLimit => "band limit must be within 1..=256",
                Self::EmptyBounds => "shape needs nonzero x and y extent",
            }
        )
    }
}

/// Standard error contract for band construction.
impl std::error::Error for BandError {}

/// Builds lists and chooses counts without changing the input outline.
#[derive(Default)]
pub struct BandBuilder;

/// Stateless building; cached counts live with prepared atlas handles.
impl BandBuilder {
    /// Rejects an overlay partition before searching or allocating final lists.
    /// This tests the partition policy's maximum count; rejected partitions
    /// subdivide instead of spending preparation time on discarded band choices.
    /// A negative answer permits conservative splitting even if another count could fit.
    /// This avoids constructing full candidate lists for pieces that will be discarded.
    pub(crate) fn overlay_fits(&self, shape: &QuadShape) -> bool {
        (0..2).all(|axis| interval_peak(shape, axis, 16) <= 64)
    }

    /// Selects each axis count, then sorts lists for shader early exits.
    pub fn build(&self, shape: &QuadShape, max_bands: usize) -> Result<BandSet, BandError> {
        build_by(shape, max_bands, peak_count)
    }

    /// Reuses control ranges across the larger overlay candidate-count search.
    /// Exact boundary corrections preserve epsilon and the ten-percent policy.
    pub fn build_overlay(&self, shape: &QuadShape, max_bands: usize) -> Result<BandSet, BandError> {
        if !(1..=64).contains(&max_bands) {
            return Err(BandError::InvalidLimit);
        }
        let ranges = [0, 1].map(|axis| {
            shape
                .curves
                .iter()
                .filter(|c| !c.flat_on(axis))
                .map(|c| c.control_range(axis))
                .collect::<Vec<_>>()
        });
        build_by(shape, max_bands, |shape, axis, count| {
            overlay_counts::peak(shape, &ranges[axis], axis, count)
        })
    }

    /// Bypasses tiny-contour acceleration for exact list/texel regression comparisons.
    #[cfg(test)]
    pub(crate) fn build_general(
        &self,
        shape: &QuadShape,
        max_bands: usize,
    ) -> Result<BandSet, BandError> {
        build_by(shape, max_bands, interval_peak)
    }
}

/// Shares validation and count selection between statically dispatched peak counters.
fn build_by<P>(shape: &QuadShape, max_bands: usize, peak: P) -> Result<BandSet, BandError>
where
    P: Fn(&QuadShape, usize, usize) -> usize + Copy,
{
    if !(1..=256).contains(&max_bands) {
        return Err(BandError::InvalidLimit);
    }
    // A collapsed axis has no finite shader strip transform, on either path.
    if shape.bbox[2] <= shape.bbox[0] || shape.bbox[3] <= shape.bbox[1] {
        return Err(BandError::EmptyBounds);
    }
    // Both counters use the same ten-percent selection policy and final list builder.
    // Generic dispatch keeps the production geometry path free of function pointers.
    let h = choose_count(shape, 1, max_bands, peak);
    let v = choose_count(shape, 0, max_bands, peak);
    Ok(BandSet {
        horizontal: lists(shape, 1, h),
        vertical: lists(shape, 0, v),
    })
}

/// Picks the smallest useful count rather than paying for tiny improvements.
fn choose_count<P>(shape: &QuadShape, axis: usize, max_bands: usize, peak: P) -> usize
where
    P: Fn(&QuadShape, usize, usize) -> usize,
{
    // Both peak implementations start from exactly the same single-band baseline.
    let mut best_count = 1;
    let mut best_max = peak(shape, axis, 1);
    for count in 2..=max_bands {
        let candidate = peak(shape, axis, count);
        // Zero means no curve crosses this ray direction, so more bands
        // cannot reduce shader work or justify extra band texels.
        if best_max == 0 {
            break;
        }
        if candidate * 10 <= best_max * 9 {
            best_count = count;
            best_max = candidate;
        }
    }
    best_count
}

/// Counts candidate memberships without allocating or sorting temporary lists.
fn peak_count(shape: &QuadShape, axis: usize, count: usize) -> usize {
    // Small COLR clip boxes have four edges. Direct overlap checks avoid the
    // fixed 256-band scratch initialization that dominates these tiny paths.
    if shape.curves.len() <= 4 {
        return small_peak(shape, axis, count);
    }
    interval_peak(shape, axis, count)
}

/// Uses interval differences for general contours with many curve memberships.
fn interval_peak(shape: &QuadShape, axis: usize, count: usize) -> usize {
    let min = shape.bbox[axis];
    let span = (shape.bbox[axis + 2] - min) / count as f32;
    // Precompute the exact widened boundaries used by final assignment.
    // Binary search compares these same f32 values, avoiding rounding from
    // converting coordinates to approximate band indices.
    let mut starts = [0.0; 256];
    let mut ends = [0.0; 256];
    for band in 0..count {
        starts[band] = min + band as f32 * span - BAND_EPSILON;
        ends[band] = min + (band + 1) as f32 * span + BAND_EPSILON;
    }
    // A curve overlaps one contiguous interval of bands. Difference counts
    // add its membership to that whole interval in constant time, replacing
    // repeated control-hull work for every candidate band of a complex glyph.
    let mut changes = [0isize; 257];
    for curve in &shape.curves {
        if curve.flat_on(axis) {
            continue;
        }
        let (low, high) = curve.control_range(axis);
        let first = ends[..count].partition_point(|&end| end < low);
        let after = starts[..count].partition_point(|&start| start <= high);
        if first < after {
            changes[first] += 1;
            changes[after] -= 1;
        }
    }
    // Prefix sums reconstruct the exact membership counts. Exclusion, epsilon,
    // chosen-count thresholds and all sorted final lists remain unchanged.
    let mut members = 0isize;
    let mut peak = 0;
    for change in &changes[..count] {
        members += change;
        peak = peak.max(members as usize);
    }
    peak
}

/// Counts tiny contours directly with the same inclusive f32 band boundaries.
fn small_peak(shape: &QuadShape, axis: usize, count: usize) -> usize {
    let min = shape.bbox[axis];
    let span = (shape.bbox[axis + 2] - min) / count as f32;
    let mut peak = 0;
    for band in 0..count {
        // Retain the exact arithmetic and overlap epsilon of final assignment.
        let start = min + band as f32 * span - BAND_EPSILON;
        let end = min + (band + 1) as f32 * span + BAND_EPSILON;
        let mut members = 0;
        for curve in &shape.curves {
            // Parallel edges cannot change winding in this ray direction.
            let (low, high) = curve.control_range(axis);
            if !curve.flat_on(axis) && high >= start && low <= end {
                members += 1;
            }
        }
        peak = peak.max(members);
    }
    peak
}

/// Assigns and sorts all curves for one ray direction.
fn lists(shape: &QuadShape, axis: usize, count: usize) -> Vec<Vec<usize>> {
    let min = shape.bbox[axis];
    let max = shape.bbox[axis + 2];
    let span = (max - min) / count as f32;
    let mut result = vec![Vec::new(); count];
    // Sort eligible curves once for the whole axis. Assigning this order to
    // every band preserves the exact stable early-exit lists while avoiding
    // repeated sorting of the same curve in many overlapping band lists.
    let other = 1 - axis;
    let mut order: Vec<_> = shape
        .curves
        .iter()
        .enumerate()
        .filter(|(_, curve)| !curve.flat_on(axis))
        .map(|(index, curve)| (index, curve.control_range(other).1))
        .collect();
    order.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (index, _) in order {
        let curve = &shape.curves[index];
        // A line parallel to the ray cannot change that ray's winding;
        // excluding it also avoids a zero denominator in root solving.
        if curve.flat_on(axis) {
            continue;
        }
        let (low, high) = curve.control_range(axis);
        for (band, members) in result.iter_mut().enumerate() {
            let start = min + band as f32 * span - BAND_EPSILON;
            let end = min + (band + 1) as f32 * span + BAND_EPSILON;
            if high >= start && low <= end {
                members.push(index);
            }
        }
    }
    // Every band's insertion order is already the same stable descending
    // control maximum required by the reference shader's early termination.
    result
}

#[cfg(test)]
/// Randomized overlap and sorting properties, plus count edge cases.
mod tests;

/// Exact corrected index guesses avoid repeated binary searches for dense paths.
mod overlay_counts;
