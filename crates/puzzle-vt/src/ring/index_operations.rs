//! Thread-local test observer for actual product index_of entries.
//! The checker regression resets this after creating its source, then requires
//! zero entries during full validation. No observer exists in product builds.
use std::cell::Cell;
thread_local! {
    /// Independent test threads cannot contaminate one another's operation count.
    static LOOKUPS: Cell<usize> = const { Cell::new(0) };
}
/// Records entry into the real lookup, including its fast path and fallback.
pub(super) fn record() {
    LOOKUPS.set(LOOKUPS.get() + 1);
}
/// Starts a new observation after fixture construction has finished.
pub(crate) fn reset() {
    LOOKUPS.set(0);
}
/// Returns the exact number of product lookup calls in the observed operation.
pub(crate) fn read() -> usize {
    LOOKUPS.get()
}
