//! Incremental source-order inspection behind an optional OSC boundary.
//!
//! The registered owner supplies endpoint receipts, never producer coordinates.
//! Every row examined (including identity lookup fallback) consumes one caller
//! visit. A guessed ring slot is verified before use, avoiding quadratic line
//! lookup; gaps/reordering fall back to the same bounded linear cursor.
//! The caller keeps parsing paused until Complete and commits no partial result.
//! Input/frames may run between turns; destructive changes cancel the scan.
//!
//! This is traversal, not an attachment certificate. Its shape stamp detects
//! transforms; consumers separately compare row versions to reject rewrites.
//! A completed scan must not be reused after parsing resumes. New verification
//! starts a new scan, and pending captures remain behind their ordered request.
//! No source text is copied by this seam, and no default Session calls it.
//! Visitor-owned scratch is accounted by that consumer's global storage ledger.
//! Direct retained-row access makes resumption constant-time; history lookup
//! cannot restart a full linear search separately for every witnessed row.
//! First-watch metadata setup uses the same physical credits as source reads.
//! It can require another turn even before the first selected cell is inspected.
//! The captured write clock also cancels source changed by local owner controls;
//! parsing is paused, but control messages must still remain serviceable.
use super::{Endpoint, EndpointReceipt, OscContext};
use puzzle_vt::{Row, Terminal, tracked::LineId};
use std::ops::Range;

/// Opaque owner-local source shape for optional presentation caches.
/// Equal values identify the same document writes/layout, not authority. A
/// consumer must still validate its own live witness whenever it acts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceRevision(Document, u64);

impl SourceRevision {
    /// Fixed metadata also keys a rebuildable core display projection.
    pub(crate) fn sample(term: &Terminal, epoch: u64, ui: u64) -> Self {
        Self(Document::sample(term, epoch), ui)
    }

    /// Saturated UI counters never wrap or authorize reuse of a stale cache.
    pub fn cacheable(self) -> bool {
        self.1 != u64::MAX
    }
}

impl OscContext<'_> {
    /// Samples fixed source metadata without visiting a row or inspecting a cell.
    /// The stamp is local to this registered owner, never a transferable receipt.
    pub fn source_revision(&self) -> SourceRevision {
        SourceRevision::sample(self.terminal, self.epoch(), self.endpoints.fold_revision)
    }
}

/// Read-only row segment inside the captured half-open interval.
pub struct SourceRow<'a> {
    /// Borrow exists only during the visitor; consumers retain bounded values.
    pub row: &'a Row,
    /// Exact source columns; the right endpoint itself is excluded.
    pub columns: Range<usize>,
    /// Captured source gaps let atomic-unit checks reject omitted fragments.
    pub(super) bounds: [Endpoint; 2],
}

/// Bounded inspection either needs another owner turn or completed the interval.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanProgress {
    /// No success or interaction is authorized before all rows are checked.
    Pending,
    /// The visitor has seen every nonempty source segment in order.
    Complete,
}

/// Generic inspection failures never echo source text or protocol fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceReadError {
    /// A source endpoint is foreign, released or evicted.
    Gone,
    /// Geometry, source lifetime or endpoint placement changed while paused.
    Changed,
    /// The interval is empty, reversed or outside the retained cell geometry.
    Invalid,
    /// The trusted visitor refused admission or source validation.
    Rejected,
}

/// Owner-local continuation, valid only behind its still-paused parser boundary.
pub struct SourceScan {
    /// Receipts are checked on every resume; copied receipts alone do not pin them.
    pub(super) receipts: [EndpointReceipt; 2],
    /// Resolved start/end from the same document before any inspection.
    pub(super) endpoints: [Endpoint; 2],
    /// Coarse transform/eviction identity, not a per-scalar observer.
    document: Document,
    /// Direct ring index at which the next physical visit resumes.
    next: usize,
    /// True until the start identity is located in source order.
    seeking: bool,
    /// A guessed first slot needs verification, then at most one fallback to zero.
    guessed: bool,
    /// Completion is sticky: a repeated call must never replay visitor effects.
    pub(super) complete: bool,
    /// First-watch metadata setup is separately charged to the same visit credits.
    ordering_next: usize,
    /// No watch can install until this bounded preparation is complete.
    pub(super) ordering_ready: bool,
    /// Source-order endpoint slots were verified by the row walker, not index_of.
    pub(super) indices: [usize; 2],
    /// Failure is sticky too; consumers must begin a new validated capture.
    failed: bool,
}

/// Cheap source shape sampled only for registered work, never while printing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Document {
    /// Registration epoch distinguishes clear/reset/screen lifetimes.
    epoch: u64,
    /// Reset/screen/clear contexts are explicit even for derived core caches.
    epochs: [u64; 3],
    /// Owner controls that write source cancel a pending capture too.
    write_clock: u64,
    /// Reflow invalidates a partially captured physical-row layout.
    reflow: u64,
    /// Height changes may move retained rows even without a width reflow.
    size: (usize, usize),
    /// Detects growth and history eviction without a retained-row scan.
    count: usize,
    /// End identities detect recycling and row replacement.
    edges: [Option<LineId>; 2],
}

impl Document {
    /// Constant-time stamp; actual row versions are inspected by the visitor.
    fn sample(term: &Terminal, epoch: u64) -> Self {
        let ring = &term.screen().ring;
        Self {
            epoch,
            epochs: [
                term.reset_epoch,
                term.screen_epoch,
                term.source_clear_epoch(),
            ],
            write_clock: term.write_clock(),
            reflow: term.reflow_generation,
            size: term.size(),
            count: ring.scrollback_len() + ring.visible_rows(),
            edges: [ring.oldest_id(), ring.newest_id()],
        }
    }
}

/// Begin/resume are synchronous owner operations; no Session or row borrow escapes.
impl OscContext<'_> {
    /// Starts a half-open source inspection after validating both receipt owners.
    /// This allocates nothing; receipt clones share an existing immutable token.
    pub fn begin_source_scan(
        &self,
        start: &EndpointReceipt,
        end: &EndpointReceipt,
    ) -> Result<SourceScan, SourceReadError> {
        let first = self.query_endpoint(start).ok_or(SourceReadError::Gone)?;
        let last = self.query_endpoint(end).ok_or(SourceReadError::Gone)?;
        if first.line == last.line && first.column >= last.column {
            return Err(SourceReadError::Invalid);
        }
        let document = Document::sample(self.terminal, self.epoch());
        // Monotonic IDs usually still occupy this guessed slot. Region edits may
        // reorder or skip identities, so resume verifies it under the visit cap.
        let next = document.edges[0]
            .and_then(|oldest| first.line.0.checked_sub(oldest.0))
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < document.count)
            .unwrap_or(0);
        Ok(SourceScan {
            receipts: [start.clone(), end.clone()],
            endpoints: [first, last],
            document,
            next,
            seeking: true,
            guessed: next != 0,
            complete: false,
            ordering_next: 0,
            ordering_ready: false,
            indices: [0; 2],
            failed: false,
        })
    }

    /// Prepares first-watch order metadata under the same physical visit budget.
    /// Existing watches already maintain it; no full-history loop hides in admission.
    pub fn prepare_source_witness(
        &mut self,
        scan: &mut SourceScan,
        remaining: &mut usize,
    ) -> Result<ScanProgress, SourceReadError> {
        if !self.source_scan_current(scan) {
            return Err(SourceReadError::Changed);
        }
        scan.ordering_ready = self
            .terminal
            .prepare_source_watch_rows(&mut scan.ordering_next, remaining);
        Ok(if scan.ordering_ready {
            ScanProgress::Complete
        } else {
            ScanProgress::Pending
        })
    }

    /// Shape, source writes and owner receipts must still match the paused capture.
    pub(super) fn source_scan_current(&self, scan: &SourceScan) -> bool {
        !scan.failed
            && scan.document == Document::sample(self.terminal, self.epoch())
            && self.query_endpoint(&scan.receipts[0]) == Some(scan.endpoints[0])
            && self.query_endpoint(&scan.receipts[1]) == Some(scan.endpoints[1])
    }

    /// Visits at most the supplied physical allowance, decrementing every visit.
    /// The callback may reject without committing partial state; parsing must
    /// remain paused behind this operation until it completes or fails.
    /// Source rewrite verification, atomic-cell rules and retained-byte admission
    /// belong to the source consumer, not this generic read-only traversal.
    pub fn visit_source_rows(
        &self,
        scan: &mut SourceScan,
        remaining: &mut usize,
        mut visitor: impl FnMut(SourceRow<'_>) -> bool,
    ) -> Result<ScanProgress, SourceReadError> {
        if scan.failed {
            return Err(SourceReadError::Changed);
        }
        // Record terminal failure once. A later retry cannot replay a visitor
        // that already refused, even if its caller still holds this scan value.
        let result = self.visit_inner(scan, remaining, &mut visitor);
        // Preserve failure even if a caller accidentally retries old continuation.
        scan.failed = result.is_err();
        result
    }

    /// Separates cancellation checks from the bounded physical-row walker.
    fn visit_inner(
        &self,
        scan: &mut SourceScan,
        remaining: &mut usize,
        visitor: &mut impl FnMut(SourceRow<'_>) -> bool,
    ) -> Result<ScanProgress, SourceReadError> {
        // Check source lifetime before sticky completion: even a fully traversed
        // old document cannot become success after a reset or geometry change.
        if !self.source_scan_current(scan) {
            return Err(SourceReadError::Changed);
        }
        if scan.complete {
            return Ok(ScanProgress::Complete);
        }
        while *remaining > 0 {
            let row = self
                .terminal
                .screen()
                .ring
                .retained(scan.next)
                .ok_or(SourceReadError::Invalid)?;
            // Failed guesses and non-source lookup rows cost a physical visit too.
            *remaining -= 1;
            if scan.seeking && row.id != scan.endpoints[0].line {
                scan.advance_lookup();
                continue;
            }
            if scan.seeking {
                scan.indices[0] = scan.next;
            }
            scan.seeking = false;
            // Pending-wrap gaps may equal width. A following hard-line gap at
            // column zero adds no cells to the half-open source interval.
            let columns = scan.columns(row)?;
            if !columns.is_empty()
                && !visitor(SourceRow {
                    row,
                    columns,
                    bounds: scan.endpoints,
                })
            {
                return Err(SourceReadError::Rejected);
            }
            // Completion occurs at the captured end identity, never by assuming
            // monotonically ordered IDs after a scroll-region edit.
            if row.id == scan.endpoints[1].line {
                scan.indices[1] = scan.next;
                scan.complete = true;
                return Ok(ScanProgress::Complete);
            }
            scan.next += 1;
        }
        Ok(ScanProgress::Pending)
    }
}

/// Continuation metadata has no mutable reference into the grid or render thread.
impl SourceScan {
    /// Exact inline continuation size; receipt owner storage is shared and charged once.
    pub fn charged_bytes(&self) -> usize {
        size_of::<Self>()
    }

    /// A wrong monotonic guess falls back once, then walks source order linearly.
    fn advance_lookup(&mut self) {
        if self.guessed {
            self.next = 0;
            self.guessed = false;
        } else {
            self.next += 1;
        }
    }

    /// Enforces the captured gap geometry, without expanding or repairing it.
    fn columns(&self, row: &Row) -> Result<Range<usize>, SourceReadError> {
        let first = if row.id == self.endpoints[0].line {
            self.endpoints[0].column
        } else {
            0
        };
        let last = if row.id == self.endpoints[1].line {
            self.endpoints[1].column
        } else {
            row.cells.len()
        };
        if first > last || last > row.cells.len() {
            return Err(SourceReadError::Invalid);
        }
        Ok(first..last)
    }
}

#[cfg(test)]
mod tests;
