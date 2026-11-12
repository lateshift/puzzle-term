//! Bounded raster pages for authored samples and reproducible grid ink (§8.9, §10.3).
//!
//! Layout owns cells. This cache owns only size-specific samples and atlas
//! addresses. A page is a 2048-square shelf store.
//! Mask and color shelves are independent: a large color glyph cannot spend
//! the smaller mask-page allowance. Page indices count only the chosen kind,
//! because the shader binds two texture arrays with separate formats.
//! A mixed CPU vector simplifies iteration without changing those addresses.
//!
//! Authored rectangles retain a one-texel transparent border for transformed
//! interpolation. Reproducible grid masks cannot be reused under transforms:
//! their exact nearest-sampled crops pack without a permanent empty border.
//! Shelf positions advance monotonically and wrap only between whole glyphs.
//! A failed fit never updates the shelf cursor, preserving the next insertion.
//!
//! Width/height and byte products are checked before copying any source row.
//! Oversized dimensions cannot overflow the padding addition or index math.
//! Subpixel data is rejected because dual-source blending remains deferred.
//! Zero-area samples are rejected because an atlas handle must name real ink.
//! The caller handles invisible controls before this raster seam is crossed.
//!
//! A lookup stamps the current incarnation onto surviving page addresses.
//! Global generation changes deliberately invalidate all session row caches:
//! they may contain a mixture of surviving and evicted shelf rectangles.
//! Rebuilding those rows chooses fresh handles without changing VT widths.
//! Browser admission asks the host for one later recovery turn.
//! Native insertion recycles cold pages or records a flush before reusing referenced ink.
//!
//! Recycling clears the entire CPU page and queues that clear before new ink.
//! A hidden frame may still own older incremental uploads. Queue order places
//! the page clear after those uploads and the replacement glyph after the clear.
//! Thus old shelf pixels cannot leak into gaps in the new page incarnation.
//!
//! Restoration sends full pages, including gutters and unoccupied pixels.
//! It replaces pending CPU dirty copies with a complete latest mirror image.
//! No lookup key, glyph origin or sample is changed by restoring the device.
//! Later cold inserts can again use small rectangular writes on the same page.
//! Atlas bytes count reserved space, not just ink, matching GPU allocation.
//! Warm hits touch a maximum of six pages and allocate no request strings.
//! Source IDs belong to FontSet, which retains the complete bounded grapheme.
//! Mask pixels use one byte,
//! color pixels four. The default four mask/two color slots bound mirrors
//! and GPU-equivalent capacity to 48 MiB. Pages allocate lazily on first use.
//!
//! Browser full shelves request eviction before the next snapshot.
//! Native full shelves recycle cold pages inside insertion without moving survivors.
//! Cached visible rows pin their pages before any cold row starts preparing.
//! Reclamation
//! advances the shared context generation and copies all affected rows again.
//! Victims are the least recently drawn pages of the requested bitmap kind.
//! Assembly touches pages even when VT row caches skip glyph preparation.
//! Each reset invalidates all handles through a common generation. Surviving
//! entries keep their UV addresses but receive that new generation on lookup.
//! Authored CPU pages remain the source of truth after uploads and device loss.
//! GPU grid masks are disposable: their font outlines regenerate them on demand.
//! Pure GPU pages retain shelf metadata only; mixed pages mirror authored pixels.
use crate::glyph::{AtlasRect, BitmapKind, GlyphKey, RasterGlyph};
// Keys are fixed validated glyph/size words in bounded stores; Fx avoids
// string-oriented randomized hashing on every warm cell lookup (design §2.6).
use rustc_hash::FxHashMap as HashMap;
/// Fixed page side required by the render contract.
pub const PAGE: u32 = 2048;
/// Reserved bytes for one coverage layer.
pub const MASK_PAGE_BYTES: usize = PAGE as usize * PAGE as usize;
/// Reserved bytes for one authored RGBA layer.
pub const COLOR_PAGE_BYTES: usize = MASK_PAGE_BYTES * 4;
/// A complete mixed frame needs both formats, even under the smallest budget.
pub const MIN_BUDGET_BYTES: usize = MASK_PAGE_BYTES + COLOR_PAGE_BYTES;
/// Budget-derived capacities and cold-path replacement.
mod budget;
/// Complete placement-independent identity of size-specific source samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RasterKey {
    /// FontSet's source identity includes the complete cluster, style and presentation.
    pub glyph: GlyphKey,
    /// Exact device font-size bits, including fractional CSS sizes after DPR scaling.
    pub size: u32,
    /// Fitting width, height and row-baseline bits distinguish grid/free-text crops.
    pub fit: [u32; 3],
    /// Exact device-pixel ratio bits; identical ppem alone cannot reuse another context.
    pub dpr: u32,
}
/// A shelf address checked against the current CPU page incarnation.
#[derive(Clone, Copy, Debug)]
pub struct RasterHandle {
    /// All row caches rebuild when eviction advances this incarnation.
    pub generation: u64,
    /// Array-layer index within this bitmap kind's GPU texture.
    pub page: u32,
    /// Page contents selected by an ordered native draw segment, independent of UV.
    pub epoch: u16,
    /// Pixel region inside the page, excluding transparent shelf padding.
    pub rect: AtlasRect,
    /// Mask tint or authored color interpretation of these samples.
    pub kind: BitmapKind,
    /// Device-pixel crop origin relative to the immutable glyph baseline.
    pub offset: [i32; 2],
}
/// One owned upload retained by the frame until written to the GPU.
pub struct RasterUpload {
    /// The GPU format determines the byte stride.
    pub kind: BitmapKind,
    /// Texture array layer, independent of the destination terminal row.
    pub page: u32,
    /// Bounded destination pixels in the fixed-size page.
    pub rect: AtlasRect,
    /// GPU crop ticket and source coordinates; absent for authored CPU bitmaps.
    pub gpu: Option<(u64, [u32; 2], std::sync::Arc<()>)>,
    /// Queue-ordered page capture before an in-build reuse.
    pub capture: Option<u16>,
    /// Tightly packed mask or straight-RGBA rows; empty for GPU-only operations.
    pub pixels: Vec<u8>,
}
/// One lazily allocated shelf page and its restoration mirror.
struct Page {
    /// Row-major coverage or RGBA samples; no font data is retained here.
    pixels: Vec<u8>,
    /// GPU-generated coverage requires a GPU snapshot even when authored masks share this page.
    gpu: bool,
    /// The two formats have independent page queues and byte costs.
    kind: BitmapKind,
    /// Next horizontal shelf position, including one transparent border texel.
    x: u32,
    /// Current shelf top; completed shelves never overlap.
    y: u32,
    /// Tallest glyph in the current shelf determines the following shelf.
    height: u32,
    /// Native shelves reuse short-glyph rows rather than wasting their tall neighbors' height.
    shelves: Vec<shelf::Shelf>,
    /// Last assembled frame drawing any region in this page.
    used: u64,
    /// Reuse stamp carried by instances; the GPU samples only the layer bits.
    epoch: u16,
}
/// Shared size-specific raster cache with an explicit page/byte budget.
pub struct RasterAtlas {
    /// Complete source keys resolve to shelf handles rather than duplicate bitmaps.
    entries: HashMap<RasterKey, RasterHandle>,
    /// Per-kind page indices stay stable until that page is recycled.
    pages: Vec<Page>,
    /// Owned incremental uploads transfer to the neutral frame contract.
    dirty: Vec<RasterUpload>,
    /// Incarnation shared by all handles, including unchanged-row cache hits.
    generation: u64,
    /// Build ordinal supplied by the host's pre-snapshot frame boundary.
    frame: u64,
    /// Maximum mask/color pages; defaults allocate at most 48 MiB.
    limits: [usize; 2],
    /// Validated byte allowance shared by the mask and color arrays.
    budget_bytes: usize,
    /// A failed cold insertion requests one eviction of this kind next frame.
    pending: Option<BitmapKind>,
    /// Native builds may recycle cold pages immediately; browser admission stays deferred.
    synchronous: bool,
    /// Earlier page contents owned by this build until its ordered draws execute.
    versions: Vec<RasterPageVersion>,
    /// Whole-page reclamations are recorded independently of vector churn.
    evictions: u64,
    /// Cold outline sampling is absent until desktop chooses a smaller mask.
    grid: grid::Cache,
    /// Earlier keys stay drawable within this frame, including a later cursor redraw.
    retired: HashMap<RasterKey, RasterHandle>,
    /// Optional lookup evidence distinguishes churn from net atlas population.
    pub(crate) metrics: Option<crate::cache_metrics::CacheMetrics>,
}
/// The default allowance is shared by every session using this text context.
impl Default for RasterAtlas {
    /// Reserves four coverage pages and two authored-color pages.
    fn default() -> Self {
        Self::new(4, 2)
    }
}
/// Owns validated shelves, generation changes and restoration data.
impl RasterAtlas {
    /// Defines a byte budget as page counts: mask×4 MiB + color×16 MiB.
    pub fn new(mask_pages: usize, color_pages: usize) -> Self {
        Self {
            entries: HashMap::default(),
            pages: Vec::new(),
            dirty: Vec::new(),
            generation: 1,
            frame: 0,
            // Reserve authored colors first, then clamp coverage within the same 48 MiB.
            limits: [
                mask_pages.clamp(1, (48 - color_pages.clamp(1, 2) * 16) / 4),
                color_pages.clamp(1, 2),
            ],
            budget_bytes: puzzle_slug::atlas::DEFAULT_BUDGET_BYTES,
            pending: None,
            synchronous: false,
            versions: Vec::new(),
            retired: HashMap::default(),
            grid: grid::Cache::default(),
            evictions: 0,
            metrics: None,
        }
    }
    /// Whether no current or earlier-batch ink can answer a placement lookup.
    /// Page storage alone is insufficient: retired keys remain drawable until
    /// this frame ends, even after their physical shelf has been recycled.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.retired.is_empty()
    }
    /// Looks up a current region before invoking an expensive platform rasterizer.
    pub fn get(&mut self, key: RasterKey) -> Option<RasterHandle> {
        let found = self
            .entries
            .get(&key)
            .or_else(|| self.retired.get(&key))
            .copied();
        crate::cache_metrics::CacheMetrics::lookup(&mut self.metrics, found.is_some(), 0);
        let mut handle = found?;
        handle.generation = self.generation;
        self.mark_used(handle.kind, handle.page);
        Some(handle)
    }
    /// Packs one bounded bitmap, preserving every existing address this frame.
    pub fn prepare(&mut self, key: RasterKey, glyph: RasterGlyph) -> Result<RasterHandle, String> {
        if let Some(handle) = self.get(key) {
            return Ok(handle);
        }
        // Validate format before dimensions; deferred LCD samples never reserve shelves.
        let channels = channels(glyph.kind)?;
        let bytes = glyph
            .width
            .checked_mul(glyph.height)
            .and_then(|n| n.checked_mul(channels));
        if glyph.width == 0
            || glyph.height == 0
            || glyph.width > PAGE - 2
            || glyph.height > PAGE - 2
            || bytes.map(|n| n as usize) != Some(glyph.pixels.len())
        {
            return Err("raster glyph exceeds page or pixel count".into());
        }
        // Source identity is bounded independently of reserved GPU/CPU bytes.
        if self.entries.len() >= 65536 {
            if self.synchronous {
                // Cold keys leave with their page; current-frame references remain pinned.
                self.reclaim(glyph.kind)?;
            } else {
                return Err("raster key allowance exceeded".into());
            }
        }
        let (slot, rect) = self.reserve(glyph.kind, glyph.width, glyph.height, 1)?;
        let page_index = self.page_index(slot);
        let page = &mut self.pages[slot];
        // Only authored samples need restoration bytes. GPU coverage is re-created from fonts.
        if page.pixels.is_empty() {
            page.pixels.resize((PAGE * PAGE * channels) as usize, 0);
        }
        // Copy exact tightly packed rows into the retained restoration mirror.
        for y in 0..glyph.height {
            let start = ((rect.y + y) * PAGE + rect.x) as usize * channels as usize;
            let source = (y * glyph.width * channels) as usize;
            let len = (glyph.width * channels) as usize;
            page.pixels[start..start + len].copy_from_slice(&glyph.pixels[source..source + len]);
        }
        page.used = self.frame;
        let handle = RasterHandle {
            generation: self.generation,
            page: page_index,
            epoch: page.epoch,
            rect,
            kind: glyph.kind,
            offset: glyph.offset_px,
        };
        self.dirty.push(RasterUpload {
            kind: glyph.kind,
            page: page_index,
            rect,
            pixels: glyph.pixels,
            gpu: None,
            capture: None,
        });
        self.entries.insert(key, handle);
        Ok(handle)
    }
    /// Refreshes usage from assembled instances, including unchanged row caches.
    pub fn mark_used(&mut self, kind: BitmapKind, index: u32) {
        if let Some(page) = self
            .pages
            .iter_mut()
            .filter(|p| p.kind == kind)
            .nth(index as usize)
        {
            page.used = self.frame;
        }
    }
    /// Transfers dirty copies while retaining complete CPU source pages.
    pub fn take_dirty(&mut self) -> Vec<RasterUpload> {
        std::mem::take(&mut self.dirty)
    }
    /// Marks every allocated CPU page for upload after device recreation.
    pub fn restore(&mut self) {
        self.dirty.clear();
        self.invalidate_gpu_grid();
        for (slot, page) in self.pages.iter().enumerate() {
            if page.pixels.is_empty() {
                continue;
            }
            self.dirty.push(RasterUpload {
                kind: page.kind,
                page: self.page_index(slot),
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
        }
    }
    /// Number of currently reusable size-specific glyphs, independently of atlas reserve bytes.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
    /// Total page replacements, so stress curves include bitmap cache churn.
    pub fn evictions(&self) -> u64 {
        self.evictions
    }
    /// Reports reserved atlas capacity, including GPU-only and unused shelf space.
    pub fn allocated_bytes(&self) -> usize {
        self.pages
            .iter()
            .map(|p| (PAGE * PAGE * channels(p.kind).unwrap_or(0)) as usize)
            .sum()
    }
    /// Reports only retained CPU source pixels, excluding reproducible GPU grid masks.
    pub fn cpu_mirror_bytes(&self) -> usize {
        self.pages.iter().map(|page| page.pixels.len()).sum()
    }
    /// Returns a handle incarnation for shared row-cache invalidation.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Converts a mixed CPU vector position into this kind's array layer.
    fn page_index(&self, slot: usize) -> u32 {
        self.pages[..slot]
            .iter()
            .filter(|p| p.kind == self.pages[slot].kind)
            .count() as u32
    }
    /// Tries existing shelves, then lazily allocates a page under the byte limit.
    fn reserve(
        &mut self,
        kind: BitmapKind,
        width: u32,
        height: u32,
        gutter: u32,
    ) -> Result<(usize, AtlasRect), String> {
        for (slot, page) in self.pages.iter_mut().enumerate() {
            if page.kind == kind
                && let Some(rect) = page.reserve(width, height, self.synchronous, gutter)
            {
                return Ok((slot, rect));
            }
        }
        let count = self.pages.iter().filter(|p| p.kind == kind).count();
        let limit = self.limits[usize::from(kind == BitmapKind::Color)];
        if count >= limit {
            if self.synchronous {
                // Referenced victims retain their pixels for an ordered draw flush before reuse.
                self.reclaim(kind)?;
                return self.reserve(kind, width, height, gutter);
            }
            // Browser admission retains its existing next-capture eviction policy.
            self.pending = Some(kind);
            return Err("raster page full; LRU eviction requested".into());
        }
        let mut page = Page {
            // GPU-only coverage pages have shelf metadata, but no CPU pixel mirror.
            pixels: if kind == BitmapKind::Mask && self.grid.reproducible_enabled() {
                Vec::new()
            } else {
                vec![0; (PAGE * PAGE * channels(kind)?) as usize]
            },
            kind,
            gpu: false,
            x: 0,
            y: 0,
            height: 0,
            shelves: Vec::new(),
            used: self.frame,
            epoch: 0,
        };
        let rect = page
            .reserve(width, height, self.synchronous, gutter)
            .ok_or("raster shelf cannot fit glyph")?;
        self.pages.push(page);
        Ok((self.pages.len() - 1, rect))
    }
}
/// Rejects the deferred LCD mode before any buffer arithmetic or GPU upload.
fn channels(kind: BitmapKind) -> Result<u32, String> {
    match kind {
        BitmapKind::Mask => Ok(1),
        BitmapKind::Color => Ok(4),
        BitmapKind::Subpixel => Err("subpixel rasters are deferred".into()),
    }
}

/// Page recency, address-safe reclamation and ordered clear ownership.
mod retention;
/// Immutable page contents preserve ink across capacity-triggered draw flushes.
mod streaming;
pub use streaming::RasterPageVersion;

#[cfg(test)]
/// Small-budget eviction, bounded input and restoration contracts.
mod tests;

/// Size-specific Slug coverage uses the same bounded shelf and restoration mirror.
mod grid;
pub use grid::{GridRasterizer, MaskDraw};

/// Native best-fit rows reduce mixed-script shelf waste without moving any glyph.
mod shelf;

impl RasterAtlas {
    /// Enables cumulative requests without clearing the underlying atlas.
    pub fn measure_cache(&mut self) {
        self.metrics.get_or_insert_with(Default::default);
    }
    /// Reports current lookups with page-reclamation totals from the same owner.
    pub fn cache_metrics(&self) -> crate::cache_metrics::CacheMetrics {
        let mut metrics = self.metrics.unwrap_or_default();
        metrics.evictions = self.evictions;
        metrics
    }
}
