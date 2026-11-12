//! Bounded free-list slots, with no generation bits taken from Cell.ch.
//! A record is freed only after its final row fragment has been retired.
//! Debug generations never wrap; exhausted slots are permanently unavailable.
//! Records are not reference-counted: the owning VT retires indexed fragments
//! synchronously before freeing a slot. Sealed publications copy their records
//! and therefore never extend a slot's mutable lifetime across threads.
/// Detached visible-window copies reuse payload storage independently of live slots.
mod snapshot;
use super::{BlockRef, Multicell};
use crate::LineId;

/// A slot persists while its record may be replaced through the free list.
#[derive(Clone, Debug)]
struct Slot {
    /// None means no cell may reference this slot.
    value: Option<Multicell>,
    /// Debug-only reuse witness; release builds store only the slot index.
    #[cfg(debug_assertions)]
    generation: u64,
}
/// Screen-local records, created lazily at the first admitted OSC 66 block.
#[derive(Clone, Debug, Default)]
pub struct MulticellStore {
    /// Slot indices remain stable while any covered cell survives.
    slots: Vec<Slot>,
    /// Only wholly unreferenced slots enter this stack.
    free: Vec<u32>,
    /// Sum of live text capacities, updated at ownership changes rather than LF.
    text_bytes: usize,
    /// Number of live slots supports constant-time exact metadata charging.
    live: usize,
    /// Rejected admissions, including key exhaustion, remain observable.
    pub discarded: u64,
}
/// Owns slot lifecycle and heap charging; row witnesses are managed by RowRing.
impl MulticellStore {
    /// Tests live occupancy in constant time, including a previously used empty store.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        // Free-list holes retain capacity, but must not select sparse frame work.
        self.live == 0
    }
    /// Looks up an active slot; stale input can never index unchecked storage.
    #[must_use]
    pub fn get(&self, key: u32) -> Option<&Multicell> {
        #[cfg(test)]
        crate::scroll_operations::block_lookup();
        self.slots.get(key as usize)?.value.as_ref()
    }
    /// Mutates retained payload without exposing ownership outside the VT.
    pub(crate) fn get_mut(&mut self, key: u32) -> Option<&mut Multicell> {
        #[cfg(test)]
        crate::scroll_operations::block_lookup();
        self.slots.get_mut(key as usize)?.value.as_mut()
    }
    /// Admits a complete block, rejecting structural capacity instead of wrapping keys.
    pub(crate) fn insert(
        &mut self,
        block: Multicell,
        limit: usize,
        bytes: usize,
    ) -> Option<BlockRef> {
        if !self.reserve_record(&block, limit, bytes) {
            return None;
        }
        // Reuse is legal only after all covered cells have been erased or retired.
        // A free slot avoids growing metadata during repeated overwrite operations.
        let key = if let Some(key) = self.free.pop() {
            key
        } else {
            // Each live record requires at least one grid cell; the caller derives
            // this structural limit from retained screen/history capacity.
            if self.slots.len() >= limit || self.slots.len() >= u32::MAX as usize {
                self.reject();
                return None;
            }
            let key = self.slots.len() as u32;
            // Keys are never truncated into the cell word: reject before casting
            // could wrap, and initialize a fresh debug witness exactly once.
            self.slots.push(Slot {
                value: None,
                #[cfg(debug_assertions)]
                generation: 0,
            });
            key
        };
        let slot = &mut self.slots[key as usize];
        // A generation is diagnostic only. Reusing a live slot is always a bug.
        debug_assert!(slot.value.is_none());
        // Capacity, not text length, is the owned heap footprint.
        self.text_bytes += block.text.capacity();
        self.live += 1;
        slot.value = Some(block);
        Some(BlockRef {
            key,
            #[cfg(debug_assertions)]
            generation: slot.generation,
        })
    }
    /// Reserves metadata only if its exact retained footprint fits admission.
    fn reserve_record(&mut self, block: &Multicell, limit: usize, bytes: usize) -> bool {
        // Reserve both index arrays together. Retirement can then return a slot
        // without allocating, and admission has charged every spare byte already.
        if self.free.is_empty() && self.slots.len() == self.slots.capacity() {
            // Geometric growth amortizes admissions, but the active/history bound
            // limits spare metadata as well as records actually holding text.
            let capacity = self.slots.capacity().saturating_mul(2).max(1).min(limit);
            let required = capacity
                .saturating_mul(size_of::<Slot>() + size_of::<u32>())
                .saturating_add(self.text_bytes)
                .saturating_add(block.text.capacity());
            if required > bytes || capacity <= self.slots.len() {
                // Refuse before reserving either array: a rejected transmission
                // must not itself increase retained memory beyond the allowance.
                self.reject();
                return false;
            }
            self.slots.reserve_exact(capacity - self.slots.len());
            self.free
                .reserve_exact(capacity.saturating_sub(self.free.len()));
        }
        if self.retained_bytes().saturating_add(block.text.capacity()) > bytes {
            // Existing free slots still own allocation bytes. Reusing a slot only
            // avoids metadata growth; it does not make the new payload free.
            self.reject();
            return false;
        }
        true
    }
    /// Removes a whole record only after the caller has cleared every reference.
    pub(crate) fn remove(&mut self, key: u32) -> Option<Multicell> {
        let slot = self.slots.get_mut(key as usize)?;
        // Taking ownership drops the payload promptly, even if this slot itself
        // remains allocated for future reuse. Empty slots retain no text buffer.
        let block = slot.value.take()?;
        self.text_bytes -= block.text.capacity();
        self.live -= 1;
        #[cfg(debug_assertions)]
        {
            // Never alias a prior generation, even after an impossible number of edits.
            let Some(next) = slot.generation.checked_add(1) else {
                return Some(block);
            };
            slot.generation = next;
        }
        self.free.push(key);
        Some(block)
    }
    /// Eviction detaches just one row; surviving lower fragments retain their text.
    pub(crate) fn retire(&mut self, line: LineId, references: &[BlockRef]) {
        // Only this retiring row is visited. Original vertical offsets remain
        // on surviving fragments so a clipped root still has a rendering origin.
        for reference in references {
            if let Some(block) = self.get_mut(reference.key) {
                block.fragments.retain(|fragment| fragment.line != line);
                if block.fragments.is_empty() {
                    self.remove(reference.key);
                }
            }
        }
    }
    /// Exact heap ownership includes unused slots/free-list capacity, not only live text.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        // Fragment arrays are inline in Slot's record storage and must not be
        // charged again per live block. Free-list capacity is separate ownership.
        self.slots.capacity() * size_of::<Slot>()
            + self.free.capacity() * size_of::<u32>()
            + self.text_bytes
    }
    /// Record storage assigned once to its first retained row.
    #[must_use]
    pub(crate) fn record_bytes(block: &Multicell) -> usize {
        size_of::<Slot>() + block.text.capacity()
    }
    /// Allocated metadata not assigned to a live first fragment remains charged.
    #[must_use]
    pub(crate) fn unused_bytes(&self) -> usize {
        // Live slots are charged to their first retained fragment. Subtract only
        // those slots here, leaving holes and reserved capacity in the store bill.
        (self.slots.capacity() - self.live) * size_of::<Slot>()
            + self.free.capacity() * size_of::<u32>()
            + size_of::<Self>()
    }
    /// Structural active storage: one maximum payload/record per occupied cell.
    #[must_use]
    pub(crate) fn active_allowance(cells: usize) -> usize {
        // An admitted block occupies at least one cell. This conservative bound
        // covers the maximum protocol payload even for a one-cell explicit run.
        cells.saturating_mul(size_of::<Slot>() + 4096 + size_of::<BlockRef>())
    }
    /// Appends a checked grapheme continuation and accounts any capacity growth.
    pub(crate) fn append(&mut self, key: u32, next: char) {
        let Some(block) = self.get_mut(key) else {
            return;
        };
        // A join can grow its String. Debit the previous allocation and charge
        // the replacement without a scan over unrelated live records.
        let before = block.text.capacity();
        block.text.push(next);
        block.revision = block.revision.saturating_add(1);
        // Read capacity before ending the record borrow; the counter belongs to
        // the store rather than the String, so independent checks can verify it.
        let growth = block.text.capacity() - before;
        self.text_bytes += growth;
    }
    /// Recomputes ownership only in the expensive test/debug oracle.
    #[cfg(debug_assertions)]
    pub(crate) fn check_storage(&self) -> Result<(), String> {
        // Production mutation maintains the counter; this independent scan
        // catches a missed ownership update after joins, erase or retirement.
        let owned: usize = self.iter().map(|(_, block)| block.text.capacity()).sum();
        if self.iter().count() != self.live {
            return Err("live slot accounting differs".into());
        }
        if owned != self.text_bytes {
            return Err("block text storage accounting differs".into());
        }
        // Duplicate free entries could hand the same index to two live blocks.
        // Bounds and liveness checks make that aliasing visible before reuse.
        for (index, key) in self.free.iter().enumerate() {
            if self.free[..index].contains(key)
                || self.get(*key).is_some()
                || *key as usize >= self.slots.len()
            {
                return Err("free list contains a duplicate, live or invalid slot".into());
            }
        }
        Ok(())
    }
    /// Iterates live records for explicit queries and consistency checks, never scrolling.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &Multicell)> {
        // Explicit readers skip holes without moving stable slot identities.
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(key, slot)| slot.value.as_ref().map(|block| (key as u32, block)))
    }
    /// Packs live records after a cold resize; Strings move without cloning ink.
    pub(crate) fn compact(&mut self) -> Vec<Option<BlockRef>> {
        // Resizing can reduce the active allowance substantially. Merely freeing
        // records would retain the old slot allocation and violate the new bound.
        let mut remap = vec![None; self.slots.len()];
        let mut slots = Vec::with_capacity(self.live);
        for (old, slot) in self.slots.drain(..).enumerate() {
            // Holes have no cell references; no remapping destination is needed.
            if slot.value.is_none() {
                continue;
            }
            let reference = BlockRef {
                // The caller rewrites every surviving witness before returning
                // to parsing, so packed indices are never observed half-applied.
                key: slots.len() as u32,
                #[cfg(debug_assertions)]
                generation: slot.generation,
            };
            remap[old] = Some(reference);
            slots.push(slot);
        }
        self.slots = slots;
        // Reserve a return slot for each live record: later retirement remains
        // allocation-free even when every compacted slot is subsequently erased.
        self.free = Vec::with_capacity(self.live);
        remap
    }
    /// Checks a row's generation only in builds that retain that witness.
    #[cfg(debug_assertions)]
    pub(crate) fn valid_reference(&self, reference: BlockRef) -> bool {
        // Generation is a debug witness only; it consumes no release cell bits.
        self.slots
            .get(reference.key as usize)
            .is_some_and(|slot| slot.value.is_some() && slot.generation == reference.generation)
    }
    /// Counts and explains discarded blocks without turning capacity into a panic.
    pub(crate) fn reject(&mut self) {
        // Capacity is a recoverable input outcome, never a reason to wrap an
        // index or partially publish a rectangle into the rows.
        self.discarded = self.discarded.saturating_add(1);
        tracing::debug!(
            event = "vt.multicell_discarded",
            "text block exceeds bounded storage or key space"
        );
    }
}
