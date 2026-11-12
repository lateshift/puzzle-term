//! Generation-checked C identities never expose a Rust allocation address.
//!
//! The low word is a one-based slot, the next 31 bits are its generation, and
//! the high bit distinguishes apps from surfaces. Retired generations never
//! wrap: exhausting a slot permanently removes it from the reusable list.
//! Registry lookup is constant-time; allocation/reuse is a cold lifecycle edge.
//! A foreign or stale identity fails before any value or native pointer is read.
//!
//! IDs have three independent fields: a namespace bit distinguishes App from
//! Surface, a generation identifies the current occupant, and a one-based slot
//! leaves zero permanently invalid. Looking up a stale ID never reconstructs
//! a pointer from host-controlled bits; it checks all fields before borrowing.
//! Removal takes the value before publishing the slot for reuse. Generation
//! exhaustion retires that slot, preventing wraparound from reviving an old ID.
//! The table itself has no thread policy: the registry checks App affinity before
//! exposing any value whose owner is a host thread. Pure tables are Miri-tested.
//! Marker tables are surface-local; their IDs must be used with that surface.
use crate::abi::ResultCode;

/// One incarnation survives removal so a later occupant rejects the old ID.
struct Slot<T> {
    /// Zero is never issued; the last generation retires instead of wrapping.
    generation: u32,
    /// A vacant slot contains no App, Surface or foreign callback context.
    value: Option<T>,
}
/// One namespace owns its values, with no pointer handles crossing the C boundary.
pub(crate) struct Table<T, const SURFACE: bool> {
    /// Storage grows only when creating more simultaneous native objects.
    slots: Vec<Slot<T>>,
    /// Vacancies permit reuse without scanning all historical objects.
    free: Vec<usize>,
}
impl<T, const SURFACE: bool> Table<T, SURFACE> {
    /// Empty tables can initialize the process registry without running host code.
    pub(crate) const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }
    /// Issues an ID after ownership transfers; slot exhaustion returns the value.
    pub(crate) fn insert(&mut self, value: T) -> Result<u64, (ResultCode, T)> {
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                if self.slots.len() >= u32::MAX as usize {
                    return Err((ResultCode::Exhausted, value));
                }
                self.slots.push(Slot {
                    generation: 1,
                    value: None,
                });
                self.slots.len() - 1
            }
        };
        self.slots[index].value = Some(value);
        // One-based slots reserve all-zero for the C null identity.
        Ok(((SURFACE as u64) << 63)
            | (u64::from(self.slots[index].generation) << 32)
            | (index as u64 + 1))
    }
    /// Checks both namespace and incarnation before revealing an occupied slot.
    fn index(&self, id: u64) -> Result<usize, ResultCode> {
        if (id >> 63 != 0) != SURFACE || id as u32 == 0 {
            return Err(ResultCode::InvalidHandle);
        }
        let index = (id as u32 - 1) as usize;
        let generation = ((id >> 32) & 0x7fff_ffff) as u32;
        let slot = self.slots.get(index).ok_or(ResultCode::InvalidHandle)?;
        if slot.generation != generation || slot.value.is_none() {
            return Err(ResultCode::InvalidHandle);
        }
        Ok(index)
    }
    /// Read-only metadata supports owner-thread checks before native access.
    pub(crate) fn get(&self, id: u64) -> Result<&T, ResultCode> {
        self.slots[self.index(id)?]
            .value
            .as_ref()
            .ok_or(ResultCode::InvalidHandle)
    }
    /// Mutation still requires the outer registry's host-affinity check.
    pub(crate) fn get_mut(&mut self, id: u64) -> Result<&mut T, ResultCode> {
        let index = self.index(id)?;
        self.slots[index]
            .value
            .as_mut()
            .ok_or(ResultCode::InvalidHandle)
    }
    /// Enumerates occupied capabilities during owner-wide settings replacement.
    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = (usize, &mut T)> {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(index, slot)| slot.value.as_mut().map(|value| (index, value)))
    }
    /// Invalidates the incarnation before returning its value for host-thread drop.
    pub(crate) fn remove(&mut self, id: u64) -> Result<T, ResultCode> {
        let index = self.index(id)?;
        let slot = &mut self.slots[index];
        let value = slot.value.take().ok_or(ResultCode::InvalidHandle)?;
        if slot.generation < 0x7fff_ffff {
            slot.generation += 1;
            self.free.push(index);
        }
        Ok(value)
    }
}

#[cfg(test)]
/// Stale, foreign-kind and exhausted incarnations must never alias a replacement.
mod tests;
