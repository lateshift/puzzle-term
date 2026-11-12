//! Constant-size, owner-scoped source witnesses for registered consumers.
//! Interior versions use the Terminal clock; precise write tracking distinguishes
//! identical boundary rewrites. Tracking survives content-preserving transforms
//! and named endpoint release. Source eligibility is still checked incrementally
//! with SourceScan: a watch never authorizes hidden, blank or split-atomic text.
//! The registration identity survives a named mark being released. A record
//! holds its own VT trackers, so its source does not depend on caller ownership.
//! Registry teardown explicitly releases these trackers; receipt copies cannot
//! create another live registration or keep discarded history pinned.
//! Admission requires both completed metadata preparation and source inspection.
//! The scan supplies already verified document slots, avoiding an unaccounted
//! index_of fallback at the final step. Actual shared capacity is billed once
//! by the owner; fixed witness records never multiply storage per physical row.
use super::{EndpointReceipt, OscContext, SourceReadError, SourceScan};
use puzzle_vt::{RangeWatch, Terminal, WatchState};

/// One source owner stores its user presentation bit beside the lifetime token.
/// The bit neither edits VT cells nor changes source validation/authorization.
pub(super) struct Watch {
    /// VT owns the tracked gaps and sticky mutation state.
    pub(super) id: RangeWatch,
    /// User folding is revoked with this exact source witness.
    pub(super) folded: bool,
}

/// One source interval, two tracked gaps and a clock, independent of row count.
/// Release explicitly through its issuing OscContext; registry teardown also
/// releases all outstanding witnesses. Copies cannot acquire source ownership.
pub struct SourceWitness {
    /// Private registration identity/epoch, not a requirement that the named mark lives.
    owner: EndpointReceipt,
    /// Terminal owns the fixed tracked range and sticky write observation.
    watch: RangeWatch,
}

impl SourceWitness {
    /// Prepaid geometry allowance for a consumer's maximum live user folds.
    /// Include it in the consumer's state cap before accepting records. This
    /// covers reusable core projection storage, even while logically unfolded.
    pub fn fold_storage_bound(intervals: usize) -> usize {
        crate::viewport::folding::storage_bound(intervals)
    }

    /// Two tracked gaps plus one clock/range record; never one entry per row.
    pub fn entries(&self) -> usize {
        3
    }

    /// Fixed record bytes. Global table capacity is charged separately, never
    /// multiplied per snapshot; this value owns no text or per-row allocation.
    pub fn charged_bytes(&self) -> usize {
        size_of::<Self>() + Terminal::source_watch_entry_bytes()
    }
}

/// Only the registration that captured these endpoints may create/query the proof.
impl OscContext<'_> {
    /// Installs a bounded source watch at this ordered parser boundary.
    /// Call only after SourceScan completes admission; registration is not proof
    /// that source text satisfies the optional protocol's visibility rules.
    pub fn capture_source_witness(
        &mut self,
        scan: &SourceScan,
        limit: usize,
    ) -> Result<SourceWitness, SourceReadError> {
        if !scan.complete || !scan.ordering_ready || !self.source_scan_current(scan) {
            return Err(SourceReadError::Changed);
        }
        let [first, last] = scan.endpoints;
        let watch = self
            .terminal
            .watch_source_prepared(
                (first.line, first.column),
                (last.line, last.column),
                (scan.indices[0], scan.indices[1]),
                limit,
            )
            .ok_or(SourceReadError::Rejected)?;
        self.endpoints.watches.push(Watch {
            id: watch,
            folded: false,
        });
        Ok(SourceWitness {
            owner: scan.receipts[0].clone(),
            watch,
        })
    }

    /// Current tracked interval/cutoff; changed or missing source is inert.
    pub fn query_source_witness(&self, witness: &SourceWitness) -> Option<WatchState> {
        self.endpoints
            .owns(&witness.owner)
            .then(|| self.terminal.source_watch(witness.watch))
            .flatten()
    }

    /// Returns maintained physical row order for an unchanged owned source.
    /// It visits watch metadata only; row identities need not be numerically sorted.
    pub fn source_witness_order(&self, witness: &SourceWitness) -> Option<(usize, usize)> {
        if !self.endpoints.owns(&witness.owner) {
            return None;
        }
        self.terminal.source_watch_order(witness.watch)
    }

    /// Relationship of two live owned intervals, without cell/row lookup work.
    /// Numeric LineIds are identities, not source-order keys after region edits.
    /// None denotes unavailable or changed source, never a disjoint live range;
    /// the consumer must retire or make that attachment inert before interaction.
    pub fn source_relation(
        &self,
        first: &SourceWitness,
        second: &SourceWitness,
    ) -> Option<puzzle_vt::RangeRelation> {
        if !self.endpoints.owns(&first.owner) || !self.endpoints.owns(&second.owner) {
            return None;
        }
        self.terminal
            .source_range_relation(first.watch, second.watch)
    }

    /// Frees this witness's private trackers without touching named marks.
    pub fn release_source_witness(&mut self, witness: SourceWitness) -> bool {
        if !self.endpoints.owns(&witness.owner) {
            return false;
        }
        let Some(index) = self
            .endpoints
            .watches
            .iter()
            .position(|entry| entry.id == witness.watch)
        else {
            return false;
        };
        let removed = self.endpoints.watches.swap_remove(index);
        self.endpoints.folded -= usize::from(removed.folded);
        if removed.folded {
            self.endpoints.fold_revision = self.endpoints.fold_revision.saturating_add(1);
        }
        self.terminal.release_source_watch(witness.watch);
        true
    }

    /// Actual retained optional table capacity, including slack in both owners.
    /// The semantic ledger additionally charges owned SourceWitness values.
    pub fn source_witness_storage_bytes(&self) -> usize {
        self.terminal.source_watch_storage_bytes()
            + self.endpoints.watches.capacity() * size_of::<Watch>()
    }
}

#[cfg(test)]
mod tests;

/// User-only projection controls use the same source owner and retirement rules.
mod folding;

/// Bounded logical text copies require this watch, including after named releases.
mod text;
pub use text::SourceTextScan;
