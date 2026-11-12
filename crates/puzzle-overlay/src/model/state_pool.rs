//! Reusable immutable payload storage for dirty models (P1, P11, D-50).
//!
//! A payload is mutable only while the pool is its sole owner. Published slots
//! keep strong references, preventing reuse until all readers have released that
//! value. Growth follows the actual number of simultaneously live publications,
//! not a tuned cache size. Three rotating slots plus the latest model value bound
//! the normal working set; no Session, renderer or pointer state is shared.
use super::{SharedState, ViewState};
use std::sync::Arc;

/// Model-owned allocations recycle after immutable publications stop referring to them.
pub struct StatePool<T> {
    /// Retained typed owners allow safe unique mutation without dynamic casts.
    values: Vec<Arc<T>>,
}
/// An empty pool allocates only when its first dirty state is published.
impl<T> Default for StatePool<T> {
    /// No payload is allocated by constructing an unused model.
    fn default() -> Self {
        Self { values: Vec::new() }
    }
}
/// Reuse cannot alter any value still visible to a snapshot or a drawing callback.
impl<T: ViewState + Clone> StatePool<T> {
    /// Publishes a value, preserving warmed storage when an old publication is free.
    pub fn publish(&mut self, value: &T) -> SharedState {
        for owned in &mut self.values {
            // Snapshots keep their own strong references; only an unshared value is writable.
            if let Some(target) = Arc::get_mut(owned) {
                // Payload types with heap buffers should implement capacity-reusing clone_from.
                target.clone_from(value);
                return owned.clone();
            }
        }
        // Initial warming, registration or a larger live publication set is cold growth.
        let owned = Arc::new(value.clone());
        self.values.push(owned.clone());
        owned
    }
}
