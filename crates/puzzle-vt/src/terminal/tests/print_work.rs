//! Test-only attribution of unused-path work; no counter enters shipped VT code.
//! Each hook observes an actual operation, including mandatory version stamps.
//! Parallel tests have independent receipts and panic cleanup restores the slot.
use std::cell::RefCell;

/// Operation indices name the work rather than estimating it from input bytes.
#[derive(Clone, Copy)]
pub(crate) enum Operation {
    /// One decoded scalar enters the slice-selected print loop.
    Scalar,
    /// One content mutation issues a version, including metadata/recycled rows.
    Version,
    /// Parser span chooses the watched or plain instantiation once.
    WatchChoice,
    /// Inspect the reply queues and deferred policy requests.
    ReplyCheck,
    /// Prepare a column-bounded atomic-glyph edge, absent in ordinary printing.
    Edge,
    /// Retry ordinary printing through a sparse rectangular reservation.
    MulticellRetry,
    /// One actual store into the shared clock word, separate from the row stamp.
    ClockStore,
}

thread_local! {
    /// Observation exists only within the measuring test's dynamic scope.
    static ACTIVE: RefCell<Option<[u64; 7]>> = const { RefCell::new(None) };
}

/// Restores inactive observation even if a functional assertion fails.
struct Scope;

/// Test isolation must survive panic unwinding as well as normal return.
impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with_borrow_mut(|slot| *slot = None);
    }
}

/// Charge an executed operation; ordinary unrelated tests retain no receipt.
pub(crate) fn note(operation: Operation) {
    ACTIVE.with_borrow_mut(|slot| {
        if let Some(counts) = slot {
            counts[operation as usize] += 1;
        }
    });
}

/// Returns physical work without sampling clocks or storing product state.
pub(crate) fn measure(operation: impl FnOnce()) -> [u64; 7] {
    ACTIVE.with_borrow_mut(|slot| {
        assert!(slot.is_none(), "nested print-work observation");
        *slot = Some([0; 7]);
    });
    let _scope = Scope;
    operation();
    ACTIVE.with_borrow(|slot| slot.unwrap_or([0; 7]))
}
