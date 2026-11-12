//! Reversible user omissions share the source watch's identity and lifetime.
//! Only complete interior rows collapse. Boundary rows preserve unrelated text;
//! the leading row supplies context and a visible marker. Atomic OSC66 rectangles
//! touching either preserved boundary keep every one of their fragments visible.
//! No cell, line identity or terminal cursor is edited by these controls.
//!
//! Folding belongs to a watched source interval, never a record-name lookup.
//! Replacing or releasing that interval removes its bit automatically, so a
//! later record cannot inherit a previously clicked control. Counts are owner
//! metadata; they never authorize a producer to hide output.
//!
//! The row projection omits only complete rows. Its leading context is always
//! retained for a marker; a partial trailing row can contain unrelated output.
//! Multi-cell rectangles are atomic even when their first fragment was evicted.
//! Their sparse fragment offsets identify how many boundary rows must remain.
//! This reads two row handles and block metadata, independent of interval length.
//!
//! Each owner's monotonic fold revision invalidates the derived viewport cache.
//! Release/reveal/clear increment it, and saturation disables cache reuse. There
//! is no timer, content matching or per-scalar hook. Nested omissions are merged
//! by the viewport, while each watch retains its own reversible user choice.
use super::{OscContext, SourceReadError, SourceWitness};
use puzzle_vt::{RangeWatch, Terminal};
use std::ops::Range;

impl OscContext<'_> {
    /// Toggles only this owner's live attachment; no producer command calls this.
    /// A region without a complete collapsible row is refused without mutation.
    pub fn set_source_fold(
        &mut self,
        witness: &SourceWitness,
        folded: bool,
    ) -> Result<(), SourceReadError> {
        // A copied receipt cannot change another registration's source UI state.
        if !self.endpoints.owns(&witness.owner) {
            return Err(SourceReadError::Gone);
        }
        // Opening remains possible even if the source lost its collapsible rows.
        if folded && omission(self.terminal, witness.watch).is_none() {
            return Err(SourceReadError::Invalid);
        }
        // The registration, not the caller's copy, owns the mutable presentation bit.
        let entry = self
            .endpoints
            .watches
            .iter_mut()
            .find(|entry| entry.id == witness.watch)
            .ok_or(SourceReadError::Gone)?;
        // Retirement, rather than numeric identity reuse, ends the presentation bit.
        if entry.folded != folded {
            // Maintain an empty guard alongside the bit; both change in one owner turn.
            self.endpoints.folded = if folded {
                self.endpoints.folded + 1
            } else {
                self.endpoints.folded - 1
            };
            entry.folded = folded;
            // Once saturated, future cache queries must always rebuild rather than alias.
            self.endpoints.fold_revision = self.endpoints.fold_revision.saturating_add(1);
        }
        Ok(())
    }

    /// Complete interior rows that a genuine-user fold could omit safely.
    /// The leading context and partial/atomic boundaries remain visible.
    pub fn source_foldable_rows(&self, witness: &SourceWitness) -> Option<Range<usize>> {
        // Eligibility follows the current tracked interval after scrolling/reflow.
        self.endpoints
            .owns(&witness.owner)
            .then(|| omission(self.terminal, witness.watch))
            .flatten()
    }

    /// Identity used for the separate blank marker row, when the user folded source.
    pub fn source_fold_marker(&self, witness: &SourceWitness) -> Option<puzzle_vt::LineId> {
        let range = self.source_fold_rows(witness)?;
        Some(self.terminal.screen().ring.retained(range.start)?.id)
    }

    /// Current hidden-line count; changed/evicted source never remains folded.
    pub fn source_fold_rows(&self, witness: &SourceWitness) -> Option<Range<usize>> {
        // A marker's count must describe this owner's current user choice only.
        if !self.endpoints.owns(&witness.owner)
            || !self
                .endpoints
                .watches
                .iter()
                .any(|entry| entry.id == witness.watch && entry.folded)
        {
            return None;
        }
        omission(self.terminal, witness.watch)
    }
}

/// Geometric boundary protection reads two direct rows and sparse fragments only.
/// Source eligibility was verified at admission; this is not another cell scan.
pub(in crate::osc) fn omission(term: &Terminal, watch: RangeWatch) -> Option<Range<usize>> {
    // Sticky write invalidation makes hidden text reappear before a later action.
    let state = term.source_watch(watch)?;
    if state.changed {
        return None;
    }
    // Maintained physical order avoids scanning history or sorting numeric LineIds.
    let (first, last) = term.source_watch_order(watch)?;
    // The first row carries ordinary context and the reversible terminal marker.
    let mut start = first.checked_add(1)?;
    // A gap at width covers its entire row; an earlier gap preserves that row.
    let mut end = last + usize::from(state.end.1 == term.size().0);
    let ring = &term.screen().ring;
    // Keep every fragment of a block touching the retained leading context row.
    let leading = ring.retained(first)?;
    for reference in &leading.blocks {
        let block = ring.multicell(reference.key)?;
        // Offsets survive partially evicted rectangles; origin need not remain live.
        let here = block
            .fragments
            .iter()
            .find(|f| f.line == leading.id)?
            .offset;
        let tail = block.fragments.last()?.offset;
        // One row map must never split the remaining fragments of a heading.
        start = start.max(first + usize::from(tail - here) + 1);
    }
    // A partial trailing row can contain unrelated rectangle ink. Never cut it.
    if end == last {
        let trailing = ring.retained(last)?;
        for reference in &trailing.blocks {
            let block = ring.multicell(reference.key)?;
            let here = block
                .fragments
                .iter()
                .find(|f| f.line == trailing.id)?
                .offset;
            let head = block.fragments.first()?.offset;
            // Preserve the head too when a rectangle touches the partial ending row.
            end = end.min(last.saturating_sub(usize::from(here - head)));
        }
    }
    // Overlapping boundary protections leave no safe hidden row, so refuse folding.
    (start < end).then_some(start..end)
}

impl crate::osc::Registry {
    /// No projection is built until an owner has at least one user-folded watch.
    pub(crate) fn has_folds(&self) -> bool {
        self.handlers
            .iter()
            .any(|owner| owner.endpoints.folded != 0)
    }

    /// Derived caches compare one small generation per registered owner.
    pub(crate) fn fold_versions(&self) -> impl Iterator<Item = u64> + Clone + '_ {
        self.handlers
            .iter()
            // Protocols with no source allocation cannot contribute a user fold.
            // Retained capacity keeps retired owners represented without new growth.
            .filter(|owner| owner.endpoints.watches.capacity() != 0)
            .map(|owner| owner.endpoints.fold_revision)
    }

    /// Combine all registered owners; nested omissions merge in the generic cache.
    pub(crate) fn fold_ranges<'a>(
        &'a self,
        term: &'a Terminal,
    ) -> impl Iterator<Item = Range<usize>> + 'a {
        self.handlers
            .iter()
            .filter(|owner| owner.endpoints.folded != 0 && owner.endpoints.is_current(term))
            .flat_map(|owner| owner.endpoints.watches.iter())
            .filter(|entry| entry.folded)
            .filter_map(|entry| omission(term, entry.id))
    }

    /// Searching/navigating into source reveals every enclosing user fold first.
    pub(crate) fn reveal_line(&mut self, term: &Terminal, line: puzzle_vt::LineId) -> bool {
        // Search already supplies a real retained identity, not a producer coordinate.
        let Some(index) = term.screen().ring.index_of(line) else {
            return false;
        };
        let mut changed = false;
        for owner in self
            .handlers
            .iter_mut()
            .filter(|owner| owner.endpoints.folded != 0 && owner.endpoints.is_current(term))
        {
            // Every enclosing omission opens; unrelated nested choices remain unchanged.
            for entry in &mut owner.endpoints.watches {
                if entry.folded
                    && omission(term, entry.id).is_some_and(|range| range.contains(&index))
                {
                    entry.folded = false;
                    owner.endpoints.folded -= 1;
                    owner.endpoints.fold_revision = owner.endpoints.fold_revision.saturating_add(1);
                    changed = true;
                }
            }
        }
        changed
    }
}
