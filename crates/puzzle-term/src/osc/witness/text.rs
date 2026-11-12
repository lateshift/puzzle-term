//! Two-pass logical source export, authorized by an existing owner-local watch.
//!
//! Count before allocation: a large reply is refused whole, never truncated.
//! Both passes walk retained order directly, spending a caller credit for every
//! row entry and cell examined. A very wide row can therefore yield mid-row.
//! This cold reader adds no printing, frame, timer or absent-handler work.
//!
//! The source watch survives named mark release. A continuation rechecks its
//! identity, tracked endpoints, order and reflow before exposing any result;
//! unrelated output outside the source does not invalidate an immutable export.
//! Transforms during a copy cancel it rather than mixing two physical layouts.
use super::SourceWitness;
use crate::osc::{OscContext, ScanProgress, SourceReadError};
use puzzle_vt::{RangeWatch, WatchState};

/// Owner-local copy continuation; not authority and never a source row borrow.
pub struct SourceTextScan {
    /// An independently retained record watch must still exist on each resume.
    watch: RangeWatch,
    /// Endpoints/cutoff must still describe the source measured by the first pass.
    state: WatchState,
    /// Maintained source slots avoid a row-identity lookup on every resume.
    order: (usize, usize),
    /// Reflow can retain a watch while replacing its physical row decomposition.
    geometry: ((usize, usize), u64),
    /// User copy policy is fixed for both passes.
    trim: bool,
    /// Consumer-supplied remaining byte allowance, not a new protocol constant.
    limit: usize,
    /// The first pass owns no text buffer; the second reserves the exact length.
    pass: Pass,
    /// Small state machine visits a row in bounded classification/extraction steps.
    walk: walker::Walker,
    /// Byte count and pending trim state are identical in both passes.
    sink: sink::Sink,
    /// Copy has failed; no partial text may escape through take_text.
    failed: bool,
}

/// Counting and copying share one walker, preventing mismatched copy policies.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    /// Bounded counting rejects before allocating the full content.
    Count,
    /// A preflighted exact-capacity buffer receives the same logical units.
    Copy,
    /// Only this phase can hand immutable text to the caller.
    Complete,
}

/// The sink never materializes a per-row String or a second full-content copy.
mod sink;
/// Physical traversal and logical byte accounting have independent responsibilities.
mod walker;

impl OscContext<'_> {
    /// Begins a bounded logical copy from an attached, already validated interval.
    /// The consumer must precharge this continuation and pass its remaining
    /// string/state allowance. No allocation or source cell read happens here.
    pub fn begin_source_text(
        &self,
        witness: &SourceWitness,
        trim: bool,
        limit: usize,
    ) -> Result<SourceTextScan, SourceReadError> {
        // A copied continuation cannot revive a retired or foreign owner.
        let state = self
            .query_source_witness(witness)
            .ok_or(SourceReadError::Gone)?;
        // Watch order is maintained by VT; admission never calls index_of.
        let order = self
            .source_witness_order(witness)
            .ok_or(SourceReadError::Changed)?;
        if state.changed {
            return Err(SourceReadError::Changed);
        }
        // The first pass owns metadata only, so an oversized reply never
        // allocates a buffer proportional to its physical source extent.
        Ok(SourceTextScan {
            watch: witness.watch,
            state,
            order,
            geometry: (self.terminal.size(), self.terminal.reflow_generation),
            trim,
            limit,
            pass: Pass::Count,
            walk: walker::Walker::new(order.0),
            sink: sink::Sink::default(),
            failed: false,
        })
    }

    /// Spends at most the supplied row/cell work allowance across both passes.
    /// A changed source cancels even after completion; take_text alone never
    /// revalidates a watch. Call this immediately before handing text to a host.
    pub fn advance_source_text(
        &self,
        witness: &SourceWitness,
        scan: &mut SourceTextScan,
        work: &mut usize,
    ) -> Result<ScanProgress, SourceReadError> {
        let result = self.copy_source_inner(witness, scan, work);
        if result.is_err() {
            // Drop a partially copied buffer on refusal; an old completed scan
            // cannot become success again after a later source invalidation.
            scan.failed = true;
            scan.sink.discard();
        }
        result
    }

    /// Ownership checks precede sticky completion and every resumed row read.
    fn copy_source_inner(
        &self,
        witness: &SourceWitness,
        scan: &mut SourceTextScan,
        work: &mut usize,
    ) -> Result<ScanProgress, SourceReadError> {
        if scan.failed
            || witness.watch != scan.watch
            || self.query_source_witness(witness) != Some(scan.state)
            || self.source_witness_order(witness) != Some(scan.order)
            || scan.geometry != (self.terminal.size(), self.terminal.reflow_generation)
        {
            return Err(SourceReadError::Changed);
        }
        while scan.pass != Pass::Complete && *work > 0 {
            // Both passes share fixed policy; callbacks cannot change trimming
            // or expand the selected interval while an export is pending.
            let options = walker::ReadOptions {
                state: scan.state,
                last: scan.order.1,
                trim: scan.trim,
                limit: scan.limit,
            };
            if !scan
                .walk
                .advance(self.terminal.buffer(), options, &mut scan.sink, work)?
            {
                return Ok(ScanProgress::Pending);
            }
            match scan.pass {
                Pass::Count => {
                    // Capacity is admitted only after the entire logical length
                    // fits. Copy repeats the same walker, without per-row buffers.
                    scan.sink.start_copy(scan.limit)?;
                    scan.walk = walker::Walker::new(scan.order.0);
                    scan.pass = Pass::Copy;
                }
                Pass::Copy => {
                    scan.sink.finish_copy()?;
                    scan.pass = Pass::Complete;
                }
                Pass::Complete => {}
            }
        }
        // The caller schedules another owner turn only while actual work remains.
        Ok(if scan.pass == Pass::Complete {
            ScanProgress::Complete
        } else {
            ScanProgress::Pending
        })
    }
}

impl SourceTextScan {
    /// Actual retained capacity, including the continuation's inline state.
    pub fn charged_bytes(&self) -> usize {
        size_of::<Self>() + self.sink.capacity()
    }

    /// Moves a complete UTF-8 value without another copy or a source borrow.
    /// Caller revalidates with advance_source_text at the handoff boundary.
    pub fn take_text(&mut self) -> Option<String> {
        (self.pass == Pass::Complete && !self.failed)
            .then(|| self.sink.take())
            .flatten()
    }
}

#[cfg(test)]
/// Released marks, bounded admission and copy semantics use real VT mutations.
mod tests;
