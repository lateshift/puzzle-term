//! Fixed curve/band page pairs implement address-stable LRU eviction (D-39).
//!
//! Each page owns the same row range in both textures. Packing either plane
//! past that range rolls back the shape, then tries another page or grows.
//! Eviction never packs, copies or uploads surviving geometry. Only later
//! writes to the cleared range become dirty; stale handles fail its generation.
use super::{AtlasError, PAGE_TEXELS, ShapeKey, SlugAtlas};
use crate::{bands::BandSet, shape::QuadShape};

/// Allocation cursors and lifetime metadata for one immutable address range.
pub(super) struct Page {
    /// End of occupied curve data within this page.
    pub(super) curve_end: usize,
    /// End of occupied band data within this page.
    pub(super) band_end: usize,
    /// Incarnation assigned when these addresses were last cleared.
    pub(super) generation: u64,
    /// Most recent use of any shape in the page, including cached row draws.
    pub(super) last_used: u64,
    /// Startup font packs keep their entire page until successful font takeover.
    pub(super) pinned: bool,
    /// Identities permit reclamation proportional to the cleared page's population.
    pub(super) keys: Vec<ShapeKey>,
}

/// Empty page metadata does not allocate texel storage independently.
impl Page {
    /// Starts both cursors at the range's first texel.
    pub(super) fn new(index: usize, generation: u64) -> Self {
        Self {
            curve_end: index * PAGE_TEXELS,
            band_end: index * PAGE_TEXELS,
            generation,
            last_used: 0,
            pinned: false,
            keys: Vec::new(),
        }
    }
}

/// Whole-shape page admission and allocation-free reclamation of old addresses.
impl SlugAtlas {
    /// Tries existing page tails before reserving another address range.
    pub(super) fn pack_in_pages(
        &mut self,
        shape: &QuadShape,
        bands: &BandSet,
    ) -> Result<[u16; 2], AtlasError> {
        // The active tail is normally the only one with useful room. A failed
        // attempt must not consume either plane or pollute its upload spans.
        // Reusing the last successful page avoids scanning all tails for normal glyphs.
        let active = self.active_page;
        match self.pack_on_page(active, shape, bands) {
            // Capacity alone can try another page; malformed geometry must propagate.
            Err(AtlasError::Capacity) => (),
            result => return result,
        }
        // A cleared or partially filled nonactive page may still accept this whole shape.
        for page in 0..self.pages.len() {
            // Do not repeat the failed transactional attempt on the active tail.
            if page == active {
                continue;
            }
            match self.pack_on_page(page, shape, bands) {
                // Capacity alone can try another page; malformed geometry must propagate.
                Err(AtlasError::Capacity) => (),
                result => return result,
            }
        }
        // An insertion may grow, but never clear a page while callers have
        // collected addresses. Pressure instead requests the next boundary.
        // Adding address space is permitted while building; reclaiming old addresses is not.
        self.add_page()?;
        self.pack_on_page(self.pages.len() - 1, shape, bands)
    }

    /// Binds packers to one page pair and commits its cursors only on success.
    fn pack_on_page(
        &mut self,
        page: usize,
        shape: &QuadShape,
        bands: &BandSet,
    ) -> Result<[u16; 2], AtlasError> {
        // Cheap lower bounds reject exhausted tails before allocating curve
        // locations or writing texels that another full plane would roll back.
        // Both planes share a row range even though their occupied lengths differ.
        let end = (page + 1) * PAGE_TEXELS;
        // Every contour contributes its initial endpoint in addition to the curve texels.
        let min_curves = shape.curves.len() + shape.contours.len();
        // Headers plus the longest list are a lower bound even with full deduplication.
        let min_bands = bands.horizontal.len()
            + bands.vertical.len()
            + bands
                .horizontal
                .iter()
                .chain(&bands.vertical)
                .map(Vec::len)
                .max()
                .unwrap_or(0);
        // Reject only proven exhaustion; the exact packer still checks alignment and B2.
        if end - self.pages[page].curve_end < min_curves
            || end - self.pages[page].band_end < min_bands
        {
            return Err(AtlasError::Capacity);
        }
        // Binding the page limits ensures a late band failure rolls back curve writes too.
        self.active_page = page;
        self.curves.cursor = self.pages[page].curve_end;
        self.bands.cursor = self.pages[page].band_end;
        self.curves.limit = (page + 1) * PAGE_TEXELS;
        self.bands.limit = self.curves.limit;
        let result = self.pack_transaction(shape, bands);
        // Failed attempts do not consume either saved page cursor.
        if result.is_ok() {
            self.pages[page].curve_end = self.curves.cursor;
            self.pages[page].band_end = self.bands.cursor;
        }
        result
    }

    /// Grows texture capacities geometrically, charging both full mirrors.
    pub(super) fn add_page(&mut self) -> Result<(), AtlasError> {
        // A new pair has fixed row bounds independent of texture doubling capacity.
        let needed = (self.pages.len() + 1) * PAGE_TEXELS;
        let curve_capacity = needed.max(self.curves.texels.len()).next_power_of_two();
        let band_capacity = needed.max(self.bands.texels.len()).next_power_of_two();
        // Preflight the combined doubling before either Vec changes. A failed
        // second-plane growth must not strand unusable, charged curve rows.
        let bytes = curve_capacity * 16 + band_capacity * 4;
        // The two textures share the 48-MiB ceiling rather than claiming it separately.
        if bytes > self.budget_bytes {
            return Err(AtlasError::Capacity);
        }
        self.curves
            .grow(needed, self.bands.bytes(), self.budget_bytes)?;
        self.bands
            .grow(needed, self.curves.bytes(), self.budget_bytes)?;
        // Metadata names only address ranges whose two mirrors now exist.
        self.pages
            .push(Page::new(self.pages.len(), self.generation));
        Ok(())
    }

    /// Clears one cold page without moving any other shape or dirtying texels.
    pub(super) fn evict_page(&mut self, index: usize) -> Result<(), AtlasError> {
        // Old handles must differ before any cleared address is reused by a new shape.
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(AtlasError::GenerationExhausted)?;
        let page = &mut self.pages[index];
        // Removing page-local identities leaves surviving pages and their bytes untouched.
        for key in page.keys.drain(..) {
            if let Some(stored) = self.entries.remove(&key) {
                let [x, y] = stored.handle.glyph_loc;
                // Row batches cannot resolve a cleared location to its former shape.
                self.locations
                    .remove(&crate::encoding::encode_glyph_loc([x, y]));
            }
        }
        // Preserve the keys allocation for later insertions into this page.
        // Former GPU texels need no zero upload: no valid handle references them.
        page.curve_end = index * PAGE_TEXELS;
        page.band_end = index * PAGE_TEXELS;
        page.generation = generation;
        page.last_used = 0;
        self.generation = generation;
        self.evictions = self.evictions.saturating_add(1);
        Ok(())
    }
}
