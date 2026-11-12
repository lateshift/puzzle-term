//! Linear cmap inversion preserves the old sorted/deduplicated candidate relation.
//!
//! Face IDs are assigned monotonically while each face's scalar mappings are
//! appended. Stable counting placement therefore yields ordered face IDs in
//! every scalar bucket without comparing millions of (scalar, face) pairs.
//! Duplicate mappings are adjacent within their bucket and removed in place.
//! The extra cursor directory is temporary and costs one Unicode-sized u32 table.

/// Dense Unicode offsets plus ordered face IDs, with exact duplicate removal.
pub(super) fn invert(pairs: Vec<(u32, u32)>) -> (Vec<u32>, Vec<u32>) {
    // The final offset is a sentinel closing the U+10FFFF candidate slice.
    let mut offsets = vec![0u32; 0x110001];
    for &(scalar, _) in &pairs {
        // Shift the count by one so prefix sums directly name bucket starts.
        if let Some(count) = offsets.get_mut(scalar as usize + 1) {
            *count += 1;
        }
    }
    // Counts become prefix offsets. Each bucket owns a disjoint range.
    for i in 1..offsets.len() {
        offsets[i] += offsets[i - 1];
    }
    // Mutable cursors leave the published prefix directory intact during placement.
    let mut cursors = offsets.clone();
    let mut faces = vec![0; *offsets.last().unwrap_or(&0) as usize];
    for (scalar, face) in pairs {
        if let Some(cursor) = cursors
            .get_mut(scalar as usize)
            .filter(|_| scalar < 0x110000)
        {
            // Counting reserved this slot; increment only this scalar's cursor.
            faces[*cursor as usize] = face;
            *cursor += 1;
        }
    }
    // The source enumerates faces in increasing ID order. Stable placement
    // keeps each bucket ordered even when a face repeats a scalar mapping.
    deduplicate(&mut offsets, &mut faces);
    (offsets, faces)
}

/// Compacts duplicate IDs in place while replacing each offset with its new start.
fn deduplicate(offsets: &mut [u32], faces: &mut Vec<u32>) {
    let mut start = 0;
    let mut written = 0;
    for scalar in 0..offsets.len() - 1 {
        // Save the original end before writing this bucket's compacted start.
        let end = offsets[scalar + 1] as usize;
        offsets[scalar] = written as u32;
        let mut previous = None;
        for i in start..end {
            let face = faces[i];
            // The write cursor never overtakes unread input, so compaction is safe.
            if previous != Some(face) {
                faces[written] = face;
                written += 1;
                previous = Some(face);
            }
        }
        start = end;
    }
    // Truncation releases no capacity until publication, matching existing accounting.
    if let Some(sentinel) = offsets.last_mut() {
        *sentinel = written as u32;
    }
    faces.truncate(written);
}

#[cfg(test)]
/// Exact relation equivalence includes duplicates, absent scalars and the final scalar.
mod tests;
