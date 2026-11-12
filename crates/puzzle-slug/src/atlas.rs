//! Prepared shapes and CPU texture mirrors for Slug (design §9.2, §9.8).
//!
//! The curve and band planes grow by rows under one 48 MiB default budget.
//! Handles carry their page incarnation, because eviction reuses addresses.
//! Live shapes never move. Clearing one cold page therefore needs neither
//! outline preparation nor a full texture re-upload (D-39).
//!
//! The mirrors use the reference's common 4096-texel width but grow as
//! separate planes. The budget counts all allocated rows, including empty
//! reserve rows, because those rows become GPU memory in M3. Band lists
//! refer to curve locations, so curves must be packed before band headers.
//!
//! A handle is visible only after both planes have accepted a whole shape.
//! Capacity failure requests between-frame page eviction in recency order.
//! A shape fits entirely in one curve/band page pair. A page generation
//! invalidates only its cleared handles; a cache epoch also causes rows
//! to collect their dependencies again before drawing.
/// Packs sorted band lists under B1 and B2.
mod band;
/// Packs quadratic contours under C1.
mod curve;
/// Imports validated, prebuilt planes without outline preparation (§8.8).
mod import;
/// Rolls back an incomplete two-plane shape write.
mod insert;
/// Registers validated prebuilt handles and startup lifetime protection.
mod packed;
/// Address-stable page allocation, growth and whole-page reclamation.
mod pages;
/// Between-frame LRU eviction and constant-time cached-row usage stamps (D-39).
mod retention;
/// Grown CPU planes and dirty row ranges.
mod store;

use crate::{
    bands::{BandBuilder, BandError, BandSet},
    shape::{QuadShape, ShapeError},
};
use kurbo::BezPath;
use std::collections::{HashMap, HashSet};
pub use store::DirtySpan;
use store::TexelStore;

/// Fixed width in both reference textures, log2 width 12.
pub const TEXTURE_WIDTH: usize = 4096;
/// Shared default memory allowance for both CPU mirrors (design §9.8).
pub const DEFAULT_BUDGET_BYTES: usize = 48 * 1024 * 1024;
/// Rows in each plane of a page pair, derived from C1/B2 rather than a corpus.
/// B2 permits at most 65,535 distinct referenced curves. Disconnected contours
/// cost two texels each, plus one C1 padding texel per 4,095 usable columns:
/// at most 131,103 texels, requiring 33 rows. The next power-of-two allocation
/// is 64 rows; the <=65,535 band block and <=64-curve overlay bands also fit.
pub const PAGE_ROWS: usize = 64;
/// Linear extent of one page in either plane.
pub const PAGE_TEXELS: usize = PAGE_ROWS * TEXTURE_WIDTH;
/// One complete curve/band page pair, including both texture formats.
pub const MIN_BUDGET_BYTES: usize = PAGE_TEXELS * (size_of::<[f32; 4]>() + size_of::<[u16; 2]>());

/// Stable caller-provided identity for one glyph or overlay outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShapeKey(pub u64);

/// Winding interpretation encoded in bit 28 of `band_info`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    /// Fill regions with any nonzero winding count.
    NonZero,
    /// Alternate inside and outside at every contour crossing.
    EvenOdd,
}

/// Coordinates and flags consumed by the future GPU instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeHandle {
    /// Page incarnation; clearing that page invalidates its former handles.
    pub generation: u64,
    /// Header origin: x in the low 16 bits, y in the high 16 bits.
    pub glyph_loc: [u16; 2],
    /// Band maxima and E flag in the reference vertex-shader bit layout.
    pub band_info: u32,
    /// Tight em-space bounds before vertex dilation.
    pub bbox: [f32; 4],
}

/// A malformed path or unrepresentable packed block is reported safely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasError {
    /// Kurbo path conversion did not produce a finite closed outline.
    Shape(ShapeError),
    /// Band limits or outline bounds are invalid.
    Band(BandError),
    /// The 48 MiB budget or 16-bit texture row limit would be exceeded.
    Capacity,
    /// B1/B2 or 16-bit list count cannot represent this shape as one block.
    BandBlockTooLarge,
    /// A band referred to a curve absent from the packed outline.
    InvalidCurveIndex,
    /// No unique generation remains for stale-handle detection.
    GenerationExhausted,
}

/// Reports why a shape needs rejection or later overlay chunking.
impl std::fmt::Display for AtlasError {
    /// Converts a bounded packing error to readable text.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shape(error) => write!(f, "{error}"),
            Self::Band(error) => write!(f, "{error}"),
            Self::Capacity => write!(f, "Slug CPU mirror capacity exceeded"),
            Self::BandBlockTooLarge => write!(f, "shape needs band-list chunking"),
            Self::InvalidCurveIndex => write!(f, "band references an absent curve"),
            Self::GenerationExhausted => write!(f, "Slug handle generations exhausted"),
        }
    }
}

/// Standard error contract for shape preparation.
impl std::error::Error for AtlasError {}

/// Current address and recency for one shape; source geometry is not retained.
struct StoredShape {
    /// The current texture location and fill flag.
    handle: ShapeHandle,
    /// Last frame whose renderer used this shape.
    last_used: u64,
}

/// CPU source of truth for the two Slug textures.
pub struct SlugAtlas {
    /// Rgba32Float mirror: p1/p2 then the next texel's p3.
    curves: TexelStore<[f32; 4]>,
    /// Rg16Uint mirror: headers and curve-location lists.
    bands: TexelStore<[u16; 2]>,
    /// Prepared handles indexed by stable caller keys.
    entries: HashMap<ShapeKey, StoredShape>,
    /// Pack-only glyphs cannot be recreated from fonts until validated takeover.
    pinned: HashSet<ShapeKey>,
    /// Cache epoch increments on eviction or reset, while live addresses stay fixed.
    generation: u64,
    /// Combined allocated bytes may not exceed this limit.
    budget_bytes: usize,
    /// A failed insertion requests eviction at the next address-safe boundary.
    pressure: bool,
    /// Fixed address ranges with independent cursors and incarnation counters.
    pages: Vec<pages::Page>,
    /// Page whose cursors the packers currently address.
    active_page: usize,
    /// Current locations resolve cached row batches without scanning all shapes.
    locations: HashMap<u32, ShapeKey>,
    /// Row batches defer per-shape age updates until an actual eviction.
    batches: HashMap<u64, retention::UsageBatch>,
    /// Tokens remain unique even after batch metadata is retired.
    next_batch: u64,
    /// Total cleared pages, exposed for evictions/second flood measurements.
    evictions: u64,
    /// Last drained plane-span counts, before and after contiguous union.
    upload_spans: [usize; 2],
}

/// The regular glyph store starts with the approved 48 MiB budget.
impl Default for SlugAtlas {
    /// Allocates the reference's 64 initial rows for each plane.
    fn default() -> Self {
        Self {
            curves: TexelStore::new(),
            bands: TexelStore::new(),
            entries: HashMap::new(),
            pinned: HashSet::new(),
            generation: 1,
            budget_bytes: DEFAULT_BUDGET_BYTES,
            pressure: false,
            pages: vec![pages::Page::new(0, 1)],
            active_page: 0,
            locations: HashMap::new(),
            batches: HashMap::new(),
            next_batch: 0,
            evictions: 0,
            upload_spans: [0; 2],
        }
    }
}

/// Preparation, lookup, usage tracking and CPU mirror access.
impl SlugAtlas {
    /// Creates a store with a caller budget at or above the 64-row minimum.
    ///
    /// Two formats consume different bytes per row: floating curve texels
    /// use sixteen bytes and integer band texels use four. Comparing both
    /// reserved capacities prevents a store from promising memory it lacks.
    pub fn with_budget(budget_bytes: usize) -> Result<Self, AtlasError> {
        let atlas = Self {
            budget_bytes,
            ..Self::default()
        };
        if atlas.curves.bytes() + atlas.bands.bytes() > budget_bytes {
            return Err(AtlasError::Capacity);
        }
        Ok(atlas)
    }

    /// Changes the cap without retiring addresses when existing planes already fit.
    /// A caller shrinking below allocated capacity must replace the atlas instead.
    pub fn set_capacity_limit(&mut self, bytes: usize) -> Result<(), AtlasError> {
        if bytes < self.allocated_bytes() {
            return Err(AtlasError::Capacity);
        }
        self.budget_bytes = bytes;
        Ok(())
    }

    /// Converts, bands and packs a typical glyph on its first use.
    pub fn prepare(
        &mut self,
        key: ShapeKey,
        path: &BezPath,
        rule: FillRule,
        frame: u64,
    ) -> Result<ShapeHandle, AtlasError> {
        if let Some(handle) = self.get(key) {
            self.mark_used(key, frame);
            return Ok(handle);
        }
        let shape = QuadShape::from_path(path, crate::shape::DEFAULT_TOLERANCE)
            .map_err(AtlasError::Shape)?;
        let bands = BandBuilder.build(&shape, 16).map_err(AtlasError::Band)?;
        self.prepare_quadratics(key, shape, bands, rule, frame)
    }

    /// Packs already bounded overlay quadratics and their selected band lists.
    /// This shares the glyph allocator and its generation/upload ownership rules.
    pub fn prepare_quadratics(
        &mut self,
        key: ShapeKey,
        shape: QuadShape,
        bands: BandSet,
        rule: FillRule,
        frame: u64,
    ) -> Result<ShapeHandle, AtlasError> {
        self.prepare_geometry(key, &shape, &bands, rule, frame)
    }

    /// Packs borrowed cold geometry, allowing bounded scratch retry without cloning curves.
    pub fn prepare_geometry(
        &mut self,
        key: ShapeKey,
        shape: &QuadShape,
        bands: &BandSet,
        rule: FillRule,
        frame: u64,
    ) -> Result<ShapeHandle, AtlasError> {
        if let Some(handle) = self.get(key) {
            self.mark_used(key, frame);
            return Ok(handle);
        }
        // Relocation here would corrupt addresses already captured by earlier rows.
        // Request a safe boundary instead; the text layer supplies bitmap overflow.
        let result = self.insert_prepared(key, shape, bands, rule, frame);
        self.pressure |= result == Err(AtlasError::Capacity);
        result
    }

    /// Names this atlas incarnation for cached overlay and effect variants.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Looks up only handles from the current atlas generation.
    #[must_use]
    pub fn get(&self, key: ShapeKey) -> Option<ShapeHandle> {
        // The map, rather than a cached render instance, owns current offsets.
        self.entries.get(&key).map(|stored| stored.handle)
    }

    /// Updates recency when an explicitly addressed shape is drawn.
    ///
    /// Preparation alone does not keep an unused glyph alive indefinitely;
    /// only a frame that actually draws it advances its retention deadline.
    pub fn mark_used(&mut self, key: ShapeKey, frame: u64) {
        if let Some(stored) = self.entries.get_mut(&key) {
            stored.last_used = stored.last_used.max(frame);
            let page = usize::from(stored.handle.glyph_loc[1]) / PAGE_ROWS;
            self.pages[page].last_used = self.pages[page].last_used.max(frame);
        }
    }

    /// Returns whether a retained handle still points at current texels.
    #[must_use]
    pub fn contains_handle(&self, handle: ShapeHandle) -> bool {
        let page = usize::from(handle.glyph_loc[1]) / PAGE_ROWS;
        self.pages
            .get(page)
            .is_some_and(|page| page.generation == handle.generation)
    }

    /// Exposes curve texels through the furthest written row for M3 upload.
    #[must_use]
    pub fn curve_texels(&self) -> &[[f32; 4]] {
        &self.curves.texels[..self.curves.high_water]
    }

    /// Exposes band texels through the furthest written row for M3 upload.
    #[must_use]
    pub fn band_texels(&self) -> &[[u16; 2]] {
        &self.bands.texels[..self.bands.high_water]
    }

    /// Takes incremental dirty spans after a consumer captures their bytes.
    ///
    /// M3 uploads only changed ranges after an atlas insertion. Draining
    /// these spans transfers responsibility for that version of the mirrors
    /// to the renderer without copying the texture contents here.
    pub fn take_dirty(&mut self) -> (Vec<DirtySpan>, Vec<DirtySpan>) {
        self.upload_spans[0] = self.curves.dirty.len() + self.bands.dirty.len();
        // Coalesce at publication, after insertion checkpoints can no longer roll back.
        self.curves.coalesce_dirty();
        self.bands.coalesce_dirty();
        self.upload_spans[1] = self.curves.dirty.len() + self.bands.dirty.len();
        (
            std::mem::take(&mut self.curves.dirty),
            std::mem::take(&mut self.bands.dirty),
        )
    }

    /// Reports write-call reduction without changing any upload address or bytes.
    pub fn upload_spans(&self) -> [usize; 2] {
        self.upload_spans
    }

    /// Restores every occupied CPU texel without repeating outline or band work.
    pub fn restore(&mut self) {
        self.curves.restore();
        self.bands.restore();
    }

    /// Clears logical contents for a fresh benchmark or a full replacement.
    pub fn reset(&mut self) -> Result<(), AtlasError> {
        // The benchmark must redo full preparation while reusing the large
        // backing buffers a long-running terminal would already own.
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(AtlasError::GenerationExhausted)?;
        self.entries.clear();
        self.pinned.clear();
        self.locations.clear();
        self.batches.clear();
        self.pressure = false;
        for (index, page) in self.pages.iter_mut().enumerate() {
            *page = pages::Page::new(index, self.generation);
        }
        self.active_page = 0;
        self.curves.reset();
        self.bands.reset();
        self.curves.limit = PAGE_TEXELS;
        self.bands.limit = PAGE_TEXELS;
        Ok(())
    }

    /// Writes a converted shape, making its handle visible only on success.
    ///
    /// B2 may fail after some curves are stored. Restoring cursors and dirty
    /// spans makes the abandoned block invisible to both lookup and upload.
    fn insert_prepared(
        &mut self,
        key: ShapeKey,
        shape: &QuadShape,
        bands: &BandSet,
        rule: FillRule,
        frame: u64,
    ) -> Result<ShapeHandle, AtlasError> {
        let glyph_loc = self.pack_in_pages(shape, bands)?;
        let horizontal = (bands.horizontal.len() - 1) as u8;
        let vertical = (bands.vertical.len() - 1) as u8;
        // E occupies bit 28 of the packed word, matching the reference's
        // bit 12 after the vertex shader shifts its high half into glyph.w.
        let even_odd = if rule == FillRule::EvenOdd {
            crate::encoding::EVEN_ODD
        } else {
            0
        };
        let handle = ShapeHandle {
            generation: self.pages[self.active_page].generation,
            glyph_loc,
            band_info: crate::encoding::encode_band_info(vertical, horizontal, even_odd),
            bbox: shape.bbox,
        };
        // Cached rows resolve this location once when their usage batch is registered.
        self.locations
            .insert(crate::encoding::encode_glyph_loc(glyph_loc), key);
        self.entries.insert(
            key,
            StoredShape {
                handle,
                last_used: frame,
            },
        );
        let page = &mut self.pages[self.active_page];
        page.keys.push(key);
        page.last_used = page.last_used.max(frame);
        Ok(handle)
    }
}

#[cfg(test)]
/// Atlas tests cover cross-row addresses, B1/B2 and stale generations.
mod tests;

#[cfg(test)]
#[path = "atlas/four_edge/tests.rs"]
/// Four-edge acceleration must preserve both selected lists and packed bytes.
mod four_edge_tests;
