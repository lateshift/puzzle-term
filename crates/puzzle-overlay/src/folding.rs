//! A reversible row projection for user-folded source, independent of VT storage.
//!
//! Only complete interior rows are omitted. The caller retains the first row
//! for context and any partial boundary rows for unrelated text. Marked maps
//! replace each merged omission with a blank UI row. Source
//! cells, identities, selection and search remain untouched. Mapping visits
//! fold intervals, never every hidden row, so a 20,000-line fold costs the same
//! as a twenty-line fold. Empty projections retain no mandatory allocation.
use std::ops::Range;

/// Display row indices map to physical retained rows through disjoint omissions.
#[derive(Default)]
pub struct RowProjection {
    /// Sorted, merged half-open physical row intervals; no marker row is included.
    hidden: Vec<Range<usize>>,
    /// Rebuild scratch swaps with the active vector, retaining warmed capacity.
    scratch: Vec<Range<usize>>,
    /// Source row count bounds every input range and mapping result.
    total: usize,
    /// Sum of disjoint interval lengths is at most total, so it cannot overflow.
    omitted: usize,
    /// A marked projection replaces each omission by one blank display row.
    marked: bool,
}

impl RowProjection {
    /// Replaces omissions from current validated attachments. Invalid/empty input
    /// is ignored and endpoints are clamped; producer coordinates never enter here.
    /// Returns true only when the display mapping actually changes.
    pub fn rebuild(
        &mut self,
        total: usize,
        ranges: impl IntoIterator<Item = Range<usize>>,
    ) -> bool {
        // The candidate stays separate: comparing it must never mutate the active map.
        // Keeping both allocations makes a fold/unfold/resize sequence warm after growth.
        self.scratch.clear();
        for range in ranges {
            let start = range.start.min(total);
            let end = range.end.min(total);
            // Clamp first, so malformed public input cannot create a reversed suffix.
            // Zero-length ranges omit nothing and never consume a stored entry.
            if start < end {
                self.scratch.push(start..end);
            }
        }
        // Sorting metadata needs no row/cell scan or temporary heap allocation.
        self.scratch.sort_unstable_by_key(|range| range.start);
        // Compact in place: the processed prefix is disjoint, sorted and bounded.
        // Each remaining candidate is consumed once, regardless of hidden line count.
        let mut used = 0;
        for next in 0..self.scratch.len() {
            let range = self.scratch[next].clone();
            if used != 0 && range.start <= self.scratch[used - 1].end {
                // Nested folds and touching omissions form one display interval.
                self.scratch[used - 1].end = self.scratch[used - 1].end.max(range.end);
            } else {
                self.scratch[used] = range;
                used += 1;
            }
        }
        // Stale tail entries are metadata only; truncate preserves their allocation.
        self.scratch.truncate(used);
        let changed = self.marked || self.total != total || self.hidden != self.scratch;
        self.marked = false;
        if changed {
            std::mem::swap(&mut self.hidden, &mut self.scratch);
            self.total = total;
            // Disjoint intervals lie inside total, proving this sum cannot overflow.
            self.omitted = self.hidden.iter().map(|range| range.len()).sum();
        }
        changed
    }

    /// Merge whole omissions first, then retain one blank marker row per union.
    /// Nested folds share that row and cannot manufacture extra display height.
    pub fn rebuild_marked(
        &mut self,
        total: usize,
        ranges: impl IntoIterator<Item = Range<usize>>,
    ) -> bool {
        // Preserve mode separately: ordinary rebuild compares original source
        // intervals, while this adapter accounts for one display marker per union.
        let marked = self.marked;
        self.marked = false;
        let changed = self.rebuild(total, ranges) || !marked;
        self.marked = true;
        // The merged interval's first identity is the synthetic row. Accounting
        // and mapping both subtract exactly one less than its source length.
        self.omitted = self.hidden.iter().map(|range| range.len() - 1).sum();
        changed
    }

    /// Marker identities refer to blank display rows, never a writable VT row.
    pub fn is_marker(&self, source: usize) -> bool {
        self.marked && self.hidden.iter().any(|range| range.start == source)
    }

    /// Identity projections keep the ordinary contiguous capture path available.
    pub fn has_omissions(&self) -> bool {
        // A cached but unfolded map is the identity and needs no mapped row copy.
        self.omitted != 0 || (self.marked && !self.hidden.is_empty())
    }

    /// Number of displayed source rows; omitted rows still exist in the terminal.
    pub fn len(&self) -> usize {
        self.total - self.omitted
    }

    /// An empty retained source has no selectable display row.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Maps one display row without iterating through a fold's hidden contents.
    pub fn source_row(&self, display: usize) -> Option<usize> {
        if display >= self.len() {
            return None;
        }
        // Passing an omission advances by its entire length in one metadata step.
        // Repeating for the next interval accounts for the resulting source offset.
        let mut source = display;
        for range in &self.hidden {
            if source < range.start {
                break;
            }
            // Disjoint ranges and the display bound prove source remains < total.
            if self.marked && source == range.start {
                break;
            }
            source += range.len() - usize::from(self.marked);
        }
        Some(source)
    }

    /// Maps a physical source row; hidden rows deliberately have no display hit.
    pub fn display_row(&self, source: usize) -> Option<usize> {
        if source >= self.total {
            return None;
        }
        // Only preceding disjoint intervals contribute to the display displacement.
        let mut omitted = 0;
        for range in &self.hidden {
            if source < range.start {
                break;
            }
            if source < range.end {
                return (self.marked && source == range.start).then_some(source - omitted);
            }
            omitted += range.len() - usize::from(self.marked);
        }
        Some(source - omitted)
    }

    /// A hidden top anchor resolves to its preceding marker rather than exposing
    /// text behind the fold. This operation never changes the underlying anchor.
    pub fn display_anchor(&self, source: usize) -> Option<usize> {
        if source >= self.total {
            return None;
        }
        let mut omitted = 0;
        for range in &self.hidden {
            if source < range.start {
                break;
            }
            if source < range.end {
                // A public map may omit source row zero; then no marker exists.
                // The checked subtraction reports absence instead of underflowing.
                return range.start.checked_sub(omitted + usize::from(!self.marked));
            }
            omitted += range.len() - usize::from(self.marked);
        }
        Some(source - omitted)
    }

    /// Precharge both interval buffers and their old/new growth peak. The bound
    /// depends only on the maximum live folds, never their source row lengths.
    pub fn storage_bound(intervals: usize) -> usize {
        if intervals == 0 {
            return 0;
        }
        // Vec's geometric growth starts at four ranges. Four capacities cover
        // both steady owners plus a reallocating owner's old/new overlap.
        let capacity = intervals
            .max(4)
            .checked_next_power_of_two()
            .unwrap_or(usize::MAX);
        capacity.saturating_mul(4 * size_of::<Range<usize>>())
    }

    /// Actual retained vector capacity, available to the caller's bounded ledger.
    pub fn allocated_bytes(&self) -> usize {
        (self.hidden.capacity() + self.scratch.capacity()) * size_of::<Range<usize>>()
    }
}

#[cfg(test)]
/// Exhaustive short documents and warm large intervals protect the inverse maps.
mod tests;
