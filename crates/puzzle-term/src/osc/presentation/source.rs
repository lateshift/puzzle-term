//! Clip terminal-captured source gaps to one frozen publication window.
//! Only retained row handles are read; cell text, glyph caches and source storage
//! are untouched. Half-open gaps remain exact at either viewport boundary.
use crate::osc::{OscContext, SourceSpan, SourceWindow, SourceWitness};

impl OscContext<'_> {
    /// Clips a checked source to a publication using two direct row-handle reads.
    /// No cells are inspected and no history scan or content matching occurs.
    pub fn source_span(&self, witness: &SourceWitness, window: SourceWindow) -> Option<SourceSpan> {
        // Querying through this context enforces the registration's private owner.
        let state = self.query_source_witness(witness)?;
        // A sticky inside rewrite remains invalid even if its final text is identical.
        if state.changed {
            return None;
        }
        // Maintained order is independent of nonmonotonic LineIds after margins.
        let (first, end) = self.source_witness_order(witness)?;
        // A zero-column end gap excludes that entire row. checked_sub handles
        // an impossible empty interval at document origin without underflow.
        let last = if state.end.1 == 0 {
            end.checked_sub(1)?
        } else {
            end
        };
        // Intersect physical row order before asking for either endpoint handle.
        let start = first.max(window.top);
        // Host extents may be extreme; saturating addition cannot wrap the clip.
        let stop = last
            .saturating_add(1)
            .min(window.top.saturating_add(window.rows));
        // Offscreen/empty intervals contribute no renderer metadata or hit region.
        if start >= stop {
            return None;
        }
        // Both reads below are direct retained-slot access, never index_of scans.
        let ring = &self.terminal.screen().ring;
        // Interior/clipped rows cover the grid width while exact boundaries
        // keep their original terminal-captured columns.
        let columns = self.terminal.size().0;
        // Only plain identity/geometry values cross to the immutable view state.
        Some(SourceSpan {
            start: (
                ring.retained(start)?.id,
                if start == first { state.start.1 } else { 0 },
            ),
            end: (
                ring.retained(stop - 1)?.id,
                if stop - 1 == end {
                    state.end.1
                } else {
                    columns
                },
            ),
            // Preserve half-open physical order for stable sorting and clipping.
            rows: (start, stop),
        })
    }
}
