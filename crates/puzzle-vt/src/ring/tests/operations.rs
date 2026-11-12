//! Test-only observation at physical cell writes and handle movement primitives.
//!
//! Nothing in this module is compiled into the core library. The observer is
//! thread-local so parallel tests cannot charge each other's work. It is inactive
//! by default: allocation-budget tests do not allocate an observation set.
//! A handle count means distinct boxed row owners participating in movement,
//! including the incoming owner. Slice/deque rotation counts its actual operands;
//! wrapped-cycle swaps additionally count every exchange, including repeated
//! participation of a cycle pivot. Both counts are bounded in the assertions.
//! Cell counts include backing initialization, copies and mutable access (a
//! conservative write), so eager clearing or new cold rows cannot pass as lazy.
use crate::{Row, ring::RowHandle};
use std::{
    cell::RefCell,
    collections::{HashSet, VecDeque},
};

/// One measured scroll's physical storage work, independent of logical columns.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// Stores, copies, initialization and conservatively writable cell borrows.
    pub(crate) written: usize,
    /// Physical slots overwritten with a fill value, excluding row-local templates.
    pub(crate) cleared: usize,
    /// Distinct owned row handles passed to a movement primitive.
    pub(crate) handles: HashSet<usize>,
    /// Actual swaps in the wrapped-region cycle, rather than deduplicated owners.
    pub(crate) exchanges: usize,
    /// Actual sparse text lookups, which ordinary output must never perform.
    pub(crate) block_lookups: usize,
    /// Existing Unicode predecessor special-layout classifications.
    pub(crate) unicode_classifications: usize,
    /// New OSC 66 predicates evaluated for an ordinary predecessor.
    pub(crate) ordinary_block_probes: usize,
    /// Present anchors visited during eviction, excluding retired registry slots.
    pub(crate) anchors_examined: usize,
}

thread_local! {
    /// Isolates observations without adding synchronization to production code.
    static ACTIVE: RefCell<Option<Counts>> = const { RefCell::new(None) };
}

/// Measures exactly one operation, leaving ordinary tests completely unobserved.
pub(crate) fn measure(operation: impl FnOnce()) -> Counts {
    ACTIVE.with_borrow_mut(|slot| {
        assert!(slot.is_none());
        *slot = Some(Counts::default());
    });
    operation();
    ACTIVE.with_borrow_mut(|slot| {
        // The observer is installed above and only this scope removes it.
        let Some(counts) = slot.take() else {
            panic!("scroll observation disappeared during the measured operation");
        };
        counts
    })
}

/// Charges physical backing stores at the point their destinations are written.
pub(crate) fn written(count: usize) {
    ACTIVE.with_borrow_mut(|slot| {
        if let Some(counts) = slot {
            counts.written += count;
        }
    });
}

/// Charges eager fill and implicit-gap materialization, never logical BCE changes.
pub(crate) fn cleared(count: usize) {
    ACTIVE.with_borrow_mut(|slot| {
        if let Some(counts) = slot {
            counts.cleared += count;
        }
    });
}

/// Records actual row owners supplied to a rotation or push operation.
pub(crate) fn moved<'a>(rows: impl Iterator<Item = &'a Row>) {
    ACTIVE.with_borrow_mut(|slot| {
        if let Some(counts) = slot {
            counts
                .handles
                .extend(rows.map(|row| std::ptr::from_ref(row) as usize));
        }
    });
}

/// Records both actual swap operands and the raw exchange count.
pub(crate) fn exchanged(first: &Row, second: &Row) {
    moved([first, second].into_iter());
    ACTIVE.with_borrow_mut(|slot| {
        if let Some(counts) = slot {
            counts.exchanges += 1;
        }
    });
}

/// VecDeque insertion shifts the shorter side, plus the new incoming owner.
pub(crate) fn inserting(rows: &VecDeque<RowHandle>, index: usize, incoming: &Row) {
    // Observe the actual shift span selected by VecDeque::insert, not the
    // terminal's requested region. The unchanged history is never charged.
    if index <= rows.len() - index {
        moved(rows.range(..index).map(Box::as_ref));
    } else {
        moved(rows.range(index..).map(Box::as_ref));
    }
    moved(std::iter::once(incoming));
}

/// Pinned std VecDeque::rotate_left copies only its shorter side. With a full
/// deque wrap_copy's source equals its destination, so only the head changes.
/// Observing that actual storage state avoids charging logical row renumbering
/// as handle writes. This mirrors Rust 1.97.1 alloc/collections/vec_deque.
pub(crate) fn rotating(rows: &VecDeque<RowHandle>, shift: usize) {
    if rows.len() == rows.capacity() {
        return;
    }
    let tail = rows.len() - shift;
    if shift <= tail {
        moved(rows.range(..shift).map(Box::as_ref));
    } else {
        moved(rows.range(shift..).map(Box::as_ref));
    }
}

/// Counts actual eviction work rather than the registry's retained high-water mark.
pub(crate) fn anchor_examined() {
    ACTIVE.with_borrow_mut(|slot| {
        if let Some(counts) = slot {
            counts.anchors_examined += 1;
        }
    });
}

/// Observes real record resolution, not the constant-time absent-store dispatch.
pub(crate) fn block_lookup() {
    ACTIVE.with_borrow_mut(|slot| {
        if let Some(counts) = slot {
            counts.block_lookups += 1;
        }
    });
}

/// Charges the existing Unicode predecessor layout classification.
pub(crate) fn unicode_classification() {
    ACTIVE.with_borrow_mut(|slot| {
        if let Some(counts) = slot {
            counts.unicode_classifications += 1;
        }
    });
}

thread_local! {
    /// A regression may cap LF attempts without sleeping or leaving a worker behind.
    static LINE_FEEDS_LEFT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// A panic from the bound must restore the calling test thread's prior observer.
struct LineFeedScope(
    /// Restore a surrounding test observer when this nested scope ends.
    Option<usize>,
);

/// Keep the guard test-local, including a regression's intentional fail-first panic.
impl Drop for LineFeedScope {
    /// Restoration runs on both normal completion and the fail-first panic.
    fn drop(&mut self) {
        LINE_FEEDS_LEFT.set(self.0);
    }
}

/// One physical LF attempt spends exactly one deterministic test credit.
pub(crate) fn line_feed_step() {
    if let Some(left) = LINE_FEEDS_LEFT.get() {
        assert!(left > 0, "text-sizing line-feed iteration bound exhausted");
        LINE_FEEDS_LEFT.set(Some(left - 1));
    }
}

/// Only selected regression feeds install this hard operation bound.
pub(crate) fn bounded_line_feeds(limit: usize, operation: impl FnOnce()) -> usize {
    let _scope = LineFeedScope(LINE_FEEDS_LEFT.replace(Some(limit)));
    operation();
    limit - LINE_FEEDS_LEFT.get().unwrap_or(0)
}
