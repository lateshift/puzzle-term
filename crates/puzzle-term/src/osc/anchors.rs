//! Handler-owned endpoint receipts on the singular Session owner (M19.2).
//! A receipt captures a gap, never a caller-supplied coordinate. Its private
//! owner token prevents a copied receipt from resolving in another handler or
//! Session. Tracking the preceding cell plus an after-edge preserves a gap at
//! pending wrap through reflow without changing the terminal's cursor state.
//! Storage is lazy and bounded by the trusted handler's declared admission cap;
//! the optional protocol accounts these entries against its global allowance.
//!
//! The owner token is not an authentication credential. It prevents accidental
//! cross-registration use of otherwise identical numeric IDs in trusted Rust
//! code. The optional protocol separately verifies its producer's MAC and maps
//! producer names to these private receipts; PTY bytes cannot construct one.
use puzzle_vt::{
    Terminal,
    tracked::{LineId, PositionState, PrivateTrackedPos},
};
use std::sync::Arc;

/// Opaque terminal-captured endpoint, meaningful only to its registered owner.
#[derive(Clone, Debug)]
pub struct EndpointReceipt {
    /// Private allocation identity is retained by receipts, preventing address reuse.
    owner: Arc<Owner>,
    /// Source lifetime at capture, never reused after a destructive transition.
    epoch: u64,
    /// Monotonic receipt identity, independent of reusable VT tracking slots.
    id: u64,
}

/// An allocation identity contains no mutable state or global registry.
#[derive(Debug)]
pub(super) struct Owner;

/// Resolved gap in the current buffer, intended only for trusted handler code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// Retained source row; this is never a producer-provided position.
    pub line: LineId,
    /// Gap before this column; width itself denotes the row's trailing edge.
    pub column: usize,
}

/// Capture refusal is explicit; no old endpoint is evicted to admit another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointError {
    /// The handler's simultaneous endpoint allowance is already in use.
    Limit,
    /// A receipt or source generation cannot be advanced without wrapping.
    Exhausted,
    /// No retained source row contains the terminal cursor.
    Gone,
}

/// One live owner reference into VT's shared scrolling/reflow tracker.
pub(super) struct Entry {
    /// Receipt identity survives swap-removal of unrelated entries.
    id: u64,
    /// Shared tracker owns all row movement, eviction and reflow rules.
    tracked: PrivateTrackedPos,
    /// An after-edge tracks the preceding cell rather than a disposable blank.
    after: bool,
}

/// One registered handler owns this bounded, lazily allocated source state.
pub(super) struct Store {
    /// Allocated only on the first successful endpoint capture.
    pub(super) owner: Option<Arc<Owner>>,
    /// Live endpoint allocations, recycled as the handler releases receipts.
    pub(super) entries: Vec<Entry>,
    /// Source witnesses own separate trackers after named endpoints are released.
    pub(super) watches: Vec<super::witness::Watch>,
    /// A constant-time empty guard avoids enumerating uncollapsed attachments.
    pub(super) folded: usize,
    /// Saturating UI generation invalidates derived row projections without wrapping.
    pub(super) fold_revision: u64,
    /// Last observed reset, screen transition and destructive-clear generations.
    source: [u64; 3],
    /// Public generation begins at one and never wraps.
    epoch: u64,
    /// Next capture identity, never reused even after release or clear.
    next: u64,
}

/// Registration observes the current source without allocating endpoint state.
impl Store {
    /// Starts an independent handler lifetime at the source's current generation.
    pub(super) fn new(term: &Terminal) -> Self {
        Self {
            owner: None,
            entries: Vec::new(),
            watches: Vec::new(),
            folded: 0,
            fold_revision: 0,
            source: source(term),
            epoch: 1,
            next: 1,
        }
    }

    /// UI dispatch may run within a command cascade before lifecycle delivery.
    /// Refuse its old source without consuming the transition notification.
    pub(super) fn is_current(&self, term: &Terminal) -> bool {
        self.source == source(term)
    }

    /// Applies destructive lifecycle changes before a callback sees the source.
    pub(super) fn sync(&mut self, term: &mut Terminal) -> bool {
        let current = source(term);
        if self.source == current {
            return false;
        }
        // Observe all three identities together: a switch away and back can
        // leave identical visible geometry while still replacing its document.
        self.source = current;
        self.clear(term);
        // Saturation permanently disables capture, instead of aliasing old receipts.
        self.epoch = self.epoch.saturating_add(1);
        true
    }

    /// Releases exactly this owner's trackers; copied receipts become inert.
    pub(super) fn clear(&mut self, term: &mut Terminal) {
        // Draining preserves capacity for the next epoch and releases only this
        // registration's handles, never a selection or another plugin's marker.
        for watch in self.watches.drain(..) {
            term.release_source_watch(watch.id);
        }
        if self.folded != 0 {
            self.fold_revision = self.fold_revision.saturating_add(1);
        }
        self.folded = 0;
        for entry in self.entries.drain(..) {
            term.remove_private_marker(entry.tracked);
        }
    }

    #[cfg(test)]
    /// Test-only ownership count verifies release and session-end cleanup.
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Supplies the immutable generation for name tables and lifecycle decisions.
    pub(super) fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Captures the current cursor gap without triggering delayed wrap or printing.
    pub(super) fn capture(
        &mut self,
        term: &mut Terminal,
        limit: usize,
    ) -> Result<EndpointReceipt, EndpointError> {
        // Admission precedes both tracker allocation and serial consumption.
        // A refusal can therefore be acknowledged without a partial endpoint.
        if self.entries.len() >= limit {
            return Err(EndpointError::Limit);
        }
        if self.epoch == u64::MAX || self.next == u64::MAX || self.source.contains(&u64::MAX) {
            return Err(EndpointError::Exhausted);
        }
        let line = term
            .screen()
            .row(term.cursor.row)
            .ok_or(EndpointError::Gone)?
            .id;
        // A trailing gap follows ink through reflow; tracking the blank at the
        // next column would clamp at a hard line's new edge and lose that gap.
        let after = term.cursor.pending_wrap || term.cursor.column > 0;
        let column = if term.cursor.pending_wrap {
            term.cursor.column
        } else if after {
            term.cursor.column - 1
        } else {
            0
        };
        // Terminal validates that this row/column is retained before adding a
        // handle to its existing screen-aware registry. No new tracking engine.
        let tracked = term
            .add_private_marker(line, column)
            .ok_or(EndpointError::Gone)?;
        // Retaining the token in every receipt prevents a dropped registration's
        // address from being reused while an old receipt still exists elsewhere.
        let owner = self.owner.get_or_insert_with(|| Arc::new(Owner)).clone();
        let id = self.next;
        self.next += 1;
        self.entries.push(Entry { id, tracked, after });
        Ok(EndpointReceipt {
            owner,
            epoch: self.epoch,
            id,
        })
    }

    /// Resolves only a live receipt from this registration and source lifetime.
    pub(super) fn query(&self, term: &Terminal, receipt: &EndpointReceipt) -> Option<Endpoint> {
        let entry = self.entry(receipt)?;
        // Gone remains Gone after history recycling; a matching new row ID or
        // matching text is never used to resurrect an evicted source position.
        match term.tracked.get_private(entry.tracked) {
            PositionState::Present { line, column } => Some(Endpoint {
                line,
                column: column + usize::from(entry.after),
            }),
            PositionState::Gone => None,
        }
    }

    /// A repeated or foreign release cannot free another owner's tracker slot.
    pub(super) fn release(&mut self, term: &mut Terminal, receipt: &EndpointReceipt) -> bool {
        if self.entry(receipt).is_none() {
            return false;
        }
        let Some(index) = self.entries.iter().position(|entry| entry.id == receipt.id) else {
            return false;
        };
        // Entries have stable IDs, so swap-removal cannot change the identity
        // of the moved survivor. VT's release also invalidates its copied handle.
        term.remove_private_marker(self.entries.swap_remove(index).tracked);
        true
    }

    /// Ownership survives a caller's endpoint release while a record owns its watch.
    pub(super) fn owns(&self, receipt: &EndpointReceipt) -> bool {
        self.epoch == receipt.epoch
            && self
                .owner
                .as_ref()
                .is_some_and(|owner| Arc::ptr_eq(owner, &receipt.owner))
    }

    /// Owner identity and epoch are checked before any reusable tracking handle.
    fn entry(&self, receipt: &EndpointReceipt) -> Option<&Entry> {
        if !self.owns(receipt) {
            return None;
        }
        self.entries.iter().find(|entry| entry.id == receipt.id)
    }
}

/// Coarse source facts are read only at registered-handler/Session boundaries.
fn source(term: &Terminal) -> [u64; 3] {
    [
        term.reset_epoch,
        term.screen_epoch,
        term.source_clear_epoch(),
    ]
}
