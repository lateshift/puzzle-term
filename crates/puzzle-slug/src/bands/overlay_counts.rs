//! Exact f32 interval membership with constant-time initial band-index guesses.
//!
//! Algebra supplies a nearby index; comparisons to the actual widened f32
//! boundaries repair rounding at edges. The resulting memberships are identical
//! to binary-search assignment, including equality and epsilon overlap.
//! Control bounds are precomputed once by the overlay count search. Differences
//! add one curve to a contiguous interval without allocating temporary lists.
use super::{BAND_EPSILON, QuadShape};

/// Counts the largest band using the same widened boundaries as final lists.
pub(super) fn peak(shape: &QuadShape, ranges: &[(f32, f32)], axis: usize, count: usize) -> usize {
    let min = shape.bbox[axis];
    let span = (shape.bbox[axis + 2] - min) / count as f32;
    // Reciprocal estimates are corrected against exact boundaries below.
    let inverse = span.recip();
    let mut starts = [0.0; 64];
    let mut ends = [0.0; 64];
    for band in 0..count {
        // Preserve operation order; the guesses alone are never membership truth.
        starts[band] = min + band as f32 * span - BAND_EPSILON;
        ends[band] = min + (band + 1) as f32 * span + BAND_EPSILON;
    }
    let mut changes = [0isize; 65];
    for &(low, high) in ranges {
        // Clamp guesses before indexing, including negative or beyond-bbox hulls.
        let mut first = ((low - min - BAND_EPSILON) * inverse)
            .floor()
            .clamp(0.0, count as f32) as usize;
        // Equality includes the preceding band because its epsilon guard still touches the hull.
        while first > 0 && ends[first - 1] >= low {
            first -= 1;
        }
        // A rounded guess may be too early; advance until the actual end reaches the hull.
        while first < count && ends[first] < low {
            first += 1;
        }
        // The exclusive upper index keeps each interval expressible as two counter updates.
        let mut after = (((high - min + BAND_EPSILON) * inverse).floor() + 1.0)
            .clamp(0.0, count as f32) as usize;
        // Correct excess inclusion from reciprocal rounding using the exact start value.
        while after > 0 && starts[after - 1] > high {
            after -= 1;
        }
        // Include boundary equality even when the estimate omitted the next band.
        while after < count && starts[after] <= high {
            after += 1;
        }
        // An empty overlap contributes no negative prefix-count interval.
        if first < after {
            changes[first] += 1;
            changes[after] -= 1;
        }
    }
    // Prefix sums reproduce exact count peaks without materializing candidate lists.
    let mut active = 0;
    let mut peak = 0;
    for change in &changes[..count] {
        active += change;
        peak = peak.max(active as usize);
    }
    peak
}
