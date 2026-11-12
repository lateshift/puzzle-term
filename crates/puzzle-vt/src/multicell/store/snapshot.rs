//! Dense visible-window copies retain payload buffers without retaining history.
//!
//! These methods serve the detached read-only BufferView, never a live parser.
//! Publication-local keys replace mutable Session keys before readers can run.
use super::{BlockRef, Multicell, MulticellStore, Slot};

/// Snapshot storage has no externally observable slot lifecycle or free-list users.
impl MulticellStore {
    /// Copies a value into a dense publication slot, preserving its text capacity.
    pub(crate) fn snapshot_copy(&mut self, index: usize, source: &Multicell) -> BlockRef {
        if index == self.slots.len() {
            // Cold visible growth owns a fresh payload, never a pointer into the parser.
            self.slots.push(Slot {
                value: Some(source.clone()),
                #[cfg(debug_assertions)]
                generation: 0,
            });
        } else if let Some(target) = self.slots[index].value.as_mut() {
            // Derived String cloning replaces warm payload allocations; reuse them.
            target.text.clone_from(&source.text);
            target.sizing = source.sizing;
            target.column = source.column;
            target.columns = source.columns;
            target.fragments.clone_from(&source.fragments);
            target.revision = source.revision;
        }
        BlockRef {
            // The compact key is meaningful only inside this detached publication.
            key: index as u32,
            #[cfg(debug_assertions)]
            generation: self.slots[index].generation,
        }
    }
    /// Drops retired payloads promptly and recomputes this small visible ownership bill.
    pub(crate) fn snapshot_finish(&mut self, count: usize) {
        // Shrinking releases hidden strings; snapshot storage cannot grow with history.
        self.slots.truncate(count);
        self.live = count;
        // Capacity, rather than string length, is what retained memory accounting charges.
        self.text_bytes = self.iter().map(|(_, block)| block.text.capacity()).sum();
        self.free.clear();
    }
}
