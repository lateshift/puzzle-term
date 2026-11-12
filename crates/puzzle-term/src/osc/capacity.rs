//! Registration-owned tables and shared VT pools spend one source allowance.
//! Admission stages empty vectors, measures actual allocator capacities and
//! counts the old allocation until commit. No failed mark/watch may leave an
//! over-budget high-water allocation hidden behind a released receipt.
//!
//! SourceWitness and EndpointReceipt values live inline in the consumer's own
//! tables. This meter covers their shared backing owners, not those inline
//! values again. A copied receipt shares one immutable registration identity;
//! the reference-count allocation remains charged while this owner exists.
//! Ordinary selection/search storage belongs to the public tracker arena and
//! is intentionally excluded from the optional consumer's private-source bill.
use super::{EndpointError, OscContext, anchors::Entry};

/// The registration adopts new storage only after all shared owners fit.
impl OscContext<'_> {
    /// All retained source bytes, including owner identity and unused capacity.
    /// This includes the private marker and witness meters: charge it once,
    /// rather than adding those two public component observations again.
    pub fn source_storage_bytes(&self) -> usize {
        self.private_marker_storage_bytes()
            + self.source_witness_storage_bytes()
            + self.endpoints.entries.capacity() * size_of::<Entry>()
            + usize::from(self.endpoints.owner.is_some()) * owner_bytes()
    }

    /// Admit the peak for upcoming captures before any live source owner grows.
    /// Limits on simultaneous endpoint/watch counts still apply at capture.
    /// `maximum` is the consumer's remaining total source allowance; it includes
    /// existing source allocations, not just the incremental acquisition cost.
    pub fn reserve_source_storage(
        &mut self,
        endpoints: usize,
        witnesses: usize,
        maximum: usize,
    ) -> Result<(), EndpointError> {
        // Stage one owner at a time with all retained peers still charged.
        // No proposal known to exceed the allowance is allocated speculatively.
        // Before the first endpoint, bill the future Arc identity even though
        // capture, rather than reservation, will construct that immutable owner.
        let identity =
            usize::from(self.endpoints.owner.is_some() || endpoints != 0) * owner_bytes();
        // VT derives the two private gap slots owned by each range watch.
        // Registration vectors contain references, not a second copy of slots.
        let core = self.private_marker_storage_bytes() + self.terminal.source_watch_storage_bytes();
        let peer = core.checked_add(identity).ok_or(EndpointError::Limit)?;
        let watch_bytes = self.endpoints.watches.capacity() * size_of::<super::witness::Watch>();
        let entry_limit = maximum
            .checked_sub(peer)
            .and_then(|n| n.checked_sub(watch_bytes))
            .ok_or(EndpointError::Limit)?;
        let mut entries = reserve(&self.endpoints.entries, endpoints, entry_limit)?;
        let entries_peak = peak(&self.endpoints.entries, &entries);
        let watch_limit = maximum
            .checked_sub(peer)
            .and_then(|n| n.checked_sub(entries_peak))
            .ok_or(EndpointError::Limit)?;
        let mut watches = reserve(&self.endpoints.watches, witnesses, watch_limit)?;
        let watches_peak = peak(&self.endpoints.watches, &watches);
        let overhead = entries_peak
            .checked_add(watches_peak)
            .and_then(|n| n.checked_add(identity))
            .ok_or(EndpointError::Limit)?;
        let available = maximum.checked_sub(overhead).ok_or(EndpointError::Limit)?;
        // The VT stages its two pools together. On refusal the registration's
        // proposals drop without changing any live receipt or capacity.
        if !self
            .terminal
            .reserve_private_source(endpoints, witnesses, available)
        {
            return Err(EndpointError::Limit);
        }
        // All fallible work is finished. Moving old elements preserves each
        // receipt's serial and each watch's sole tracker ownership.
        if let Some(mut next) = entries.take() {
            next.append(&mut self.endpoints.entries);
            self.endpoints.entries = next;
        }
        // No callback or allocation runs between these moves; the registration
        // cannot expose a half-adopted plan to another source consumer.
        if let Some(mut next) = watches.take() {
            next.append(&mut self.endpoints.watches);
            self.endpoints.watches = next;
        }
        Ok(())
    }
}

/// The identity has zero payload; Arc retains two native reference-count words.
fn owner_bytes() -> usize {
    // Owner is zero-sized and has alignment one. Unlike receipt clones, this
    // unique allocation is billed only once for the registration's lifetime.
    2 * size_of::<usize>()
}

/// Plan growth without moving the old elements or changing their identities.
fn reserve<T>(
    old: &Vec<T>,
    additional: usize,
    maximum: usize,
) -> Result<Option<Vec<T>>, EndpointError> {
    let needed = old
        .len()
        .checked_add(additional)
        .ok_or(EndpointError::Limit)?;
    if old.capacity() >= needed {
        return Ok(None);
    }
    // Count old and proposed storage before allocation, then recheck actual
    // capacity. A failed plan never transfers its elements or issuance state.
    let minimum = old
        .capacity()
        .checked_add(needed)
        .and_then(|n| n.checked_mul(size_of::<T>()))
        .ok_or(EndpointError::Limit)?;
    if minimum > maximum {
        return Err(EndpointError::Limit);
    }
    let mut proposed = Vec::new();
    proposed
        .try_reserve_exact(needed)
        .map_err(|_| EndpointError::Limit)?;
    let next = Some(proposed);
    if peak(old, &next) > maximum {
        return Err(EndpointError::Limit);
    }
    Ok(next)
}

/// A proposal coexists with the full retained old vector until commit.
fn peak<T>(old: &Vec<T>, proposed: &Option<Vec<T>>) -> usize {
    (old.capacity() + proposed.as_ref().map_or(0, Vec::capacity)) * size_of::<T>()
}

#[cfg(test)]
/// Admission rollback and warmed captures are observable through the context.
mod tests;
