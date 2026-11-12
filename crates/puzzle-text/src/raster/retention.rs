//! Raster page reclamation keeps complete native builds and browser admission distinct.
//!
//! Native insertion recycles cold pages directly; referenced pages require a draw flush.
//! Visible cached rows pin their pages before new geometry collects addresses.
//! A clear changes the shared incarnation, never the UV of a surviving glyph.
//! The final context epoch invalidates other sessions after this build completes.
//! Browser pressure remains an explicit request for its next capture boundary.
//! Clears are owned uploads ordered after older pending writes and before new ink.
//! CPU mirrors include transparent gutters so device restore repeats exact samples.
use super::{AtlasRect, BitmapKind, PAGE, RasterAtlas, RasterUpload};

/// One owner controls both page ages and stale-handle invalidation.
impl RasterAtlas {
    /// Processes a requested LRU eviction before VT damage capture.
    pub fn begin_frame(&mut self) -> bool {
        // One context ordinal covers all row cache hits and newly prepared samples.
        self.frame = self.frame.saturating_add(1);
        // Old keys belong only to the image just built; generation changes rebuild later rows.
        if !self.retired.is_empty() {
            self.retired.clear();
            // Prune only after pressure: warm frames never walk the glyph population.
            self.grid
                .outlines
                .retain(|key| self.entries.contains_key(key));
        }
        let Some(kind) = self.pending.take() else {
            return false;
        };
        // Recycle an entire page; shelf holes need no fragmentation allocator.
        // Both formats have independent budgets; only the requested format may supply space.
        let victim = self
            .pages
            .iter()
            .enumerate()
            .filter(|(_, p)| p.kind == kind)
            .min_by_key(|(_, p)| p.used)
            .map(|(i, _)| i);
        let Some(victim) = victim else {
            return false;
        };
        self.recycle(victim);
        true
    }
    /// Selects complete native preparation without changing browser page admission.
    pub fn set_synchronous(&mut self) {
        // A native font owner is complete; no renderer recovery turn admits its ink.
        self.synchronous = true;
    }
    /// Recycles the LRU page, owning old pixels if recorded draws still reference it.
    pub(super) fn reclaim(&mut self, kind: BitmapKind) -> Result<(), String> {
        // Queue pending coverage before capturing the victim: its GPU pixels must be current.
        self.flush_grid()?;
        // Older stamps select cold victims first; all-current pressure records a flush.
        let victim = self
            .pages
            .iter()
            .enumerate()
            .filter(|(_, page)| page.kind == kind)
            .min_by_key(|(index, page)| (page.used, *index))
            .map(|(index, _)| index)
            .ok_or("raster kind has no allocated page")?;
        // A used victim cannot be overwritten until its recorded draws have executed.
        // Own its exact pixels now; the neutral frame tells the GPU where to flush.
        if self.pages[victim].used == self.frame {
            self.retain_version(victim);
        }
        self.recycle(victim);
        Ok(())
    }
    /// Orders the clear before replacement ink; outstanding older uploads remain valid.
    fn recycle(&mut self, victim: usize) {
        self.evictions = self.evictions.saturating_add(1);
        // Mixed CPU storage uses separate per-format GPU layer indices.
        let kind = self.pages[victim].kind;
        let index = self.page_index(victim);
        // Retire every key naming the old shelf incarnation before returning a new address.
        self.entries
            .retain(|_, h| h.kind != kind || h.page != index);
        let page = &mut self.pages[victim];
        // Recycled grid shelves have no authored source left; discard that obsolete CPU mirror.
        if kind == BitmapKind::Mask && self.grid.reproducible_enabled() {
            page.pixels = Vec::new();
        } else {
            // Authored-only pages retain their mirror and clear abandoned shelf tails too.
            page.pixels.fill(0);
        }
        // The replacement incarnation has no generated coverage until its next grid crop.
        page.gpu = false;
        // Resetting the shelf assigns future addresses; survivor pages are never repacked.
        page.x = 0;
        page.y = 0;
        page.height = 0;
        page.shelves.clear();
        // The same layer/rectangle may now contain unrelated ink within this frame.
        page.epoch = page.epoch.wrapping_add(1);
        // A later reuse within this build must retain this new incarnation before clearing.
        page.used = self.frame;
        // Clearing the whole mirror also removes pixels from the old shelf layout.
        self.dirty.push(RasterUpload {
            kind,
            page: index,
            rect: AtlasRect {
                x: 0,
                y: 0,
                width: PAGE,
                height: PAGE,
            },
            pixels: page.pixels.clone(),
            gpu: None,
            capture: None,
        });
        self.grid
            .outlines
            .retain(|key| self.entries.contains_key(key) || self.retired.contains_key(key));
        // Hosts publish this incarnation to other session caches after the current build.
        self.generation = self.generation.wrapping_add(1);
    }
    /// Reports deferred eviction so a failed frame can request one recovery turn.
    pub fn needs_eviction(&self) -> bool {
        self.pending.is_some()
    }
}
