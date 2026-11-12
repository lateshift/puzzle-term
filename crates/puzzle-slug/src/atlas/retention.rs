//! D-39 atlas pressure is resolved before a frame captures shape addresses.
//!
//! Rows stamp one batch clock on every draw. Eviction materializes those clocks
//! only when needed, then clears the least-recently-used eligible page.
//! No insertion clears a live page, so current-frame handles remain valid.
use super::{AtlasError, ShapeKey, SlugAtlas};

/// One retained row's distinct dependencies and most recent draw frame.
pub(super) struct UsageBatch {
    /// Stable keys identify page dependencies; repeated instances need only one key.
    keys: Vec<ShapeKey>,
    /// The row's single draw stamp represents all of its shape dependencies.
    frame: u64,
}

/// Constant-time warm row stamps and infrequent recency-driven reclamation.
impl SlugAtlas {
    /// Registers a cold row's band locations, or stamps its existing batch once.
    /// The iterator is not visited on a warm hit, regardless of instance count.
    pub fn use_batch(&mut self, token: &mut u64, locations: impl Iterator<Item = u32>, frame: u64) {
        // A cache hit writes one clock instead of revisiting every instance.
        if let Some(batch) = self.batches.get_mut(token) {
            batch.frame = frame;
            return;
        }
        self.refresh_batch(token, locations, frame);
    }

    /// Replaces changed row dependencies, reusing the existing key allocation (P1).
    pub fn refresh_batch(
        &mut self,
        token: &mut u64,
        locations: impl Iterator<Item = u32>,
        frame: u64,
    ) {
        let mut keys = self.batches.remove(token).map_or_else(Vec::new, |batch| {
            // Materialize the old row's stamp before replacing its dependency set.
            for key in &batch.keys {
                self.mark_used(*key, batch.frame);
            }
            batch.keys
        });
        // Keep the old row allocation while replacing its dependency list.
        keys.clear();
        // Only live band addresses can become shape dependencies.
        keys.extend(locations.filter_map(|loc| self.locations.get(&loc).copied()));
        // Ordering enables duplicate removal without a per-row hash allocation.
        keys.sort_unstable();
        // Repeated characters in a row need one recency dependency each.
        keys.dedup();
        // Zero denotes an unregistered batch, never a caller-supplied frame number.
        if *token == 0 {
            self.next_batch = self.next_batch.saturating_add(1);
            *token = self.next_batch;
        }
        self.batches.insert(*token, UsageBatch { keys, frame });
    }

    /// Retires a replaced row's metadata while preserving atlas shape identities.
    pub fn forget_batch(&mut self, token: u64) {
        if let Some(batch) = self.batches.remove(&token) {
            // Replaced rows keep their true last draw age for the next LRU decision.
            for key in batch.keys {
                self.mark_used(key, batch.frame);
            }
        }
    }

    /// Clears one least-recently-used page before a frame captures any address.
    /// The preceding draw's dependencies conservatively protect the next build.
    pub fn begin_frame(&mut self, frame: u64) -> Result<bool, AtlasError> {
        // Ordinary frame boundaries do not scan or clear atlas entries.
        if !self.pressure {
            return Ok(false);
        }
        // Materialize all cached-row draws before selecting the oldest eligible page.
        self.flush_usage();
        // Retire inactive row dependencies only after materializing their clocks.
        self.batches
            .retain(|_, batch| batch.frame >= frame.saturating_sub(1));
        // An insertion can request another boundary if this frame still lacks capacity.
        self.pressure = false;
        // Previous-frame rows may be reused in this build and must survive selection.
        let protected = frame.saturating_sub(1);
        let oldest = self
            .pages
            .iter()
            .enumerate()
            .filter(|(_, page)| !page.pinned && !page.keys.is_empty() && page.last_used < protected)
            .min_by_key(|(index, page)| (page.last_used, *index))
            .map(|(index, _)| index);
        let Some(index) = oldest else {
            // A current frame filling every page uses bitmap overflow. Clearing
            // any of these addresses would silently draw another glyph's ink.
            return Ok(false);
        };
        // Clearing changes generations but never relocates another page's shape.
        self.evict_page(index)?;
        Ok(true)
    }

    /// Materializes batch recency only at eviction, keeping warm draws O(rows).
    fn flush_usage(&mut self) {
        for batch in self.batches.values() {
            for key in &batch.keys {
                // An evicted key is harmless; stale batches cannot resurrect its data.
                if let Some(entry) = self.entries.get_mut(key) {
                    entry.last_used = entry.last_used.max(batch.frame);
                    let index = usize::from(entry.handle.glyph_loc[1]) / super::PAGE_ROWS;
                    let page = &mut self.pages[index];
                    page.last_used = page.last_used.max(batch.frame);
                }
            }
        }
    }

    /// Counts cleared pages for evictions/second pressure-run evidence.
    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    /// Counts reserved mirror rows, including unused capacity charged to the GPU.
    pub fn allocated_bytes(&self) -> usize {
        self.curves.bytes() + self.bands.bytes()
    }

    /// Reports whether an insertion needs a future address-safe eviction boundary.
    pub fn needs_eviction(&self) -> bool {
        self.pressure
    }
}
