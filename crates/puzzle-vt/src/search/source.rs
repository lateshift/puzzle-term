//! Search frontier rebasing and source-driven invalidation (§6.3, §6.10).
//! History is immutable after leaving the live grid, so remember versions only
//! for the old live rows. Their IDs follow output into history and distinguish
//! rewrites from harmless movement. Evicted result handles are released, while
//! surviving matches retain their exact tracked endpoints. A rewritten logical
//! suffix is searched again; walking to its start is itself incremental.
//! A multi-row block also continues its logical band, even without soft-wrap.
//! Rewind therefore follows the contents view's combined continuation rule.
use super::Search;
use crate::Terminal;

impl Search {
    /// Reconciles output/resize once before the next slice, never per parser byte.
    pub fn reconcile(&mut self, term: &mut Terminal) {
        let context = (term.screen_epoch, term.size().0);
        if self.context.is_some_and(|old| old != context) {
            // A reset/screen switch can reuse numeric IDs; width changes reflow text.
            self.clear(term);
            self.next_row = 0;
            self.done = false;
            self.live.clear();
        }
        // Install the incarnation before recording the new visible-row witnesses.
        self.context = Some(context);
        let ring = &term.screen().ring;
        // The scan frontier names text, not a ring index that eviction can shift.
        // Missing frontier text means all preceding scanned rows were evicted.
        // A frontier beyond the retained ring cannot supply overlap bytes.
        // Restart at the retained boundary rather than pretending it was scanned.
        let frontier_gone = self
            .last_scanned
            .is_some_and(|id| ring.index_of(id).is_none());
        if frontier_gone {
            // Byte overlap belongs to the last scanned row, just like results.
            // Its eviction makes that prefix unavailable; joining it to the new
            // oldest row would invent a match across discarded document text.
            self.pending.clear();
            // Each position is tied to a byte of the discarded overlap.
            self.positions.clear();
        }
        self.next_row = self
            .last_scanned
            .and_then(|id| ring.index_of(id))
            .map_or(0, |i| i + 1);
        // Only the old live grid can have been rewritten since the previous slice.
        // Resolve those identities even after they move into immutable history.
        let changed = self
            .live
            .iter()
            .filter_map(|(id, version)| {
                let index = ring.index_of(*id)?;
                (ring.retained(index)?.version != *version).then_some(index)
            })
            .min();
        // Compact gone endpoints first; next/prev and count never include evicted hits.
        self.retain_before(term, changed);
        self.rewind_changed(term, changed);
        // The cap ends work, but later eviction can free slots and resume the tail.
        // No timer is needed once both the frontier and result cap are settled.
        self.done = self.rewind.is_none()
            && (self.next_row >= ring_len(term) || self.results.len() == super::scan::MAX_MATCHES);
        // The next output transaction compares against this reconciled baseline.
        self.capture_live(term);
    }

    /// Restarts source discovery at the earliest rewritten scanned suffix.
    fn rewind_changed(&mut self, term: &Terminal, changed: Option<usize>) {
        if let Some(index) = changed.filter(|i| *i < self.next_row) {
            // Earlier pending rewind work still owns the earliest affected suffix.
            // Rebase a suspended backward walk by identity too: more output can
            // evict history between display turns while this job is unfinished.
            let old = self.rewind.and_then(|id| term.screen().ring.index_of(id));
            self.rewind = term
                .screen()
                .ring
                .retained(old.map_or(index, |old| old.min(index)))
                .map(|r| r.id);
            self.pending.clear();
            self.positions.clear();
        }
    }

    /// Reuses the small visible-row witness vector after each source transaction.
    pub(super) fn capture_live(&mut self, term: &Terminal) {
        // Reusing this small vector avoids copying a 100,000-row version table.
        // Direct tail indexing keeps capture cost proportional to visible height.
        self.live.clear();
        let ring = &term.screen().ring;
        self.live.extend(
            (0..term.size().1)
                .filter_map(|i| ring.retained(ring.scrollback_len() + i))
                .map(|row| (row.id, row.version)),
        );
        self.context = Some((term.screen_epoch, term.size().0));
    }

    /// Releases invalid suffix hits and preserves the selected surviving handle.
    fn retain_before(&mut self, term: &mut Terminal, first_changed: Option<usize>) {
        // Keep the selected registry handle, not its shifting result-array index.
        // Removing an older hit must not switch current to an unrelated match.
        let selected = self.current.and_then(|i| self.results.get(i)).copied();
        self.results.retain(|hit| {
            // Both endpoints and their document membership must still be present.
            // A suffix rewrite invalidates cross-boundary hits as well as later hits.
            let keep = match (
                super::tracked_point(term, hit.start),
                super::tracked_point(term, hit.end),
            ) {
                (Some(start), Some(end)) => {
                    let ring = &term.screen().ring;
                    match (ring.index_of(start.line), ring.index_of(end.line)) {
                        (Some(a), Some(b)) => {
                            first_changed.is_none_or(|first| a < first && b < first)
                        }
                        _ => false,
                    }
                }
                _ => false,
            };
            // Gone slots remain owner-held until released; dropping only the Vec
            // entry would leak two registry slots on every scrollback eviction.
            if !keep {
                term.tracked.release(hit.start);
                term.tracked.release(hit.end);
            }
            keep
        });
        // A removed current match becomes absent; surviving current retains identity.
        self.current = selected.and_then(|hit| self.results.iter().position(|other| *other == hit));
    }

    /// Finds a hard boundary within the same allowance used for forward searching.
    pub(super) fn finish_rewind(&mut self, term: &mut Terminal) -> bool {
        let Some(id) = self.rewind else { return true };
        // IDs survive ordinary scrolling. A gone rewind row clamps to the oldest
        // retained prefix, which is now the only possible logical-line beginning.
        let mut index = term.screen().ring.index_of(id).unwrap_or(0);
        while index > 0 && self.visited < super::scan::SLICE_ROWS {
            // Metadata discovery consumes physical-row credit just like scanning.
            // A hostile wrapped line cannot make this setup a hidden long loop.
            self.visited += 1;
            let wrapped = term
                .screen()
                .ring
                .retained(index - 1)
                .and_then(|row| term.buffer().line(row.id))
                .is_some_and(|line| line.continues());
            if !wrapped {
                break;
            }
            index -= 1;
        }
        if index > 0 && self.visited == super::scan::SLICE_ROWS {
            // Yield with a stable identity rather than a stale document offset.
            self.rewind = term.screen().ring.retained(index).map(|r| r.id);
            return false;
        }
        // Rebuilding a whole logical line preserves nonoverlapping match semantics.
        self.retain_before(term, Some(index));
        self.next_row = index;
        self.last_scanned = index
            .checked_sub(1)
            .and_then(|i| term.screen().ring.retained(i))
            .map(|r| r.id);
        self.rewind = None;
        true
    }
}

/// Uses stored ring dimensions rather than traversing all of scrollback.
fn ring_len(term: &Terminal) -> usize {
    term.screen().ring.scrollback_len() + term.size().1
}
