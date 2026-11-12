//! Slug preparation behind the backend-neutral text seam (design §8.9).
//!
//! Each `GlyphKey` receives a stable atlas shape identity. Unhinted keys
//! can serve many sizes; hinted keys include ppem and therefore occupy
//! distinct atlas entries. The caller's placement is never rewritten.
//!
//! This adapter owns the atlas, while an outline source owns font parsing.
//! A failed or malformed font outline returns a report before GPU upload.
use super::{GlyphBackend, GlyphKey, GlyphOutlineSource, GlyphPlacement, PreparedGlyph};
use puzzle_slug::{
    ShapeKey, SlugAtlas,
    atlas::{FillRule, ShapeHandle},
};
// Keys are fixed validated glyph/size words in bounded stores; Fx avoids
// string-oriented randomized hashing on every warm cell lookup (design §2.6).
use rustc_hash::FxHashMap as HashMap;
/// Cold-path budget replacement preserves immutable font-pack sources.
mod budget;
/// COLR layer preparation shares the same curve and band atlas.
mod color;
/// Overlay path partitions and brushes share the glyph atlas and identity allocator.
mod overlays;
/// Pack registration and the full-font takeover lifetime.
mod packed;
pub use color::{ColorDraw, PreparedColor};
pub use overlays::OverlayChunk;

/// Size-independent or ppem-specific outlines prepared for Slug draws.
#[derive(Default)]
pub struct SlugBackend {
    /// CPU mirrors and packed curve/band texture rows.
    atlas: SlugAtlas,
    /// Collision-free shape IDs assigned to complete glyph keys.
    shapes: HashMap<GlyphKey, ShapeKey>,
    /// Cold packed-byte descriptors let a cached large vector later choose a smaller mask.
    vector_sizes: HashMap<GlyphKey, usize>,
    /// Next shape ID; zero is reserved for non-font fixtures.
    next_shape: u64,
    /// Ordered COLR layers and their bounded paint mirror.
    colors: color::ColorCache,
    /// Cold native outline matching records only missing-style synthesis flags.
    source_styles: HashMap<GlyphKey, (bool, bool)>,
    /// Validated pack owners recover evicted startup ink without font preparation.
    packs: HashMap<u32, packed::Source>,
    /// Device-linear path variants, bounded and released with their source owners.
    overlays: overlays::Cache,
    /// Forward affine bitmap metadata shares the bounded paint mirror.
    raster_effects: raster_effects::Cache,
    /// Desktop host selection; web contexts keep the previous vector policy.
    grid: grid::Policy,
    /// Lazily allocated CPU scratch exists only after covered vector overflow.
    oracle: Option<super::oracle::OracleRasterizer>,
    /// Opt-in handle lookups count valid placement-specific vector reuse.
    metrics: Option<crate::cache_metrics::CacheMetrics>,
}

/// Exposes the atlas for upload after placements have been prepared.
impl SlugBackend {
    /// Transfers the prepared CPU mirror to an isolated test scene.
    pub fn into_atlas(self) -> SlugAtlas {
        self.atlas
    }
    /// Borrows the CPU texture mirrors for the rendering layer.
    pub fn atlas(&self) -> &SlugAtlas {
        &self.atlas
    }

    /// Borrows the atlas mutably so the platform renderer can drain uploads.
    pub fn atlas_mut(&mut self) -> &mut SlugAtlas {
        &mut self.atlas
    }

    /// Looks up a packed handle without drawing or re-preparing the glyph.
    pub fn get(&self, key: GlyphKey) -> Option<ShapeHandle> {
        // Pack and runtime outlines share stable IDs. The atlas resolves each
        // ID to its current generation/address after between-frame eviction.
        let key = self.pack_key(key);
        self.shapes
            .get(&key)
            .and_then(|shape| self.atlas.get(*shape))
    }

    /// Counts distinct prepared glyph keys, including all shared sessions.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.shapes.len()
    }
}

/// Prepares through the shared Slug packer without affecting grid layout.
impl GlyphBackend for SlugBackend {
    /// Gets an existing handle or validates and packs a new outline.
    fn prepare(
        &mut self,
        placement: &GlyphPlacement,
        source: &mut dyn GlyphOutlineSource,
    ) -> Result<PreparedGlyph, String> {
        self.prepare_fitted(placement, source, false, false, false)
    }
}

/// Capacity overflow chooses the bitmap seam without changing layout (D-39).
impl SlugBackend {
    /// Prepares ink with the caller's existing fitting and synthetic style policy.
    pub fn prepare_fitted(
        &mut self,
        placement: &GlyphPlacement,
        source: &mut dyn GlyphOutlineSource,
        fitting: bool,
        italic: bool,
        bold: bool,
    ) -> Result<PreparedGlyph, String> {
        if let Some(handle) = self.get_for_placement(placement, [fitting, italic, bold]) {
            // Draw-time row stamps, rather than outline lookup alone, refresh recency.
            return Ok(PreparedGlyph::Vector(handle));
        }
        // §8.8 keeps covered glyphs on the pack path after takeover. A cleared
        // page is restored from its validated data, never queued for font work.
        if let Some(result) = self.recover_pack(placement, fitting, italic, bold) {
            return result;
        }
        // A new shape ID is allocated only after the outline succeeds, so
        // malformed fonts cannot consume the finite atlas identity space.
        let Some(ink) = source.prepared_outline(placement, [fitting, italic, bold]) else {
            return Ok(PreparedGlyph::Pending);
        };
        let Some(ink) = ink? else {
            return Ok(PreparedGlyph::Empty);
        };
        self.prepare_outline(placement, ink, fitting, italic, bold)
    }

    /// Packs owned geometry without repeating extraction, curve reduction or band building.
    pub fn prepare_outline(
        &mut self,
        placement: &GlyphPlacement,
        ink: crate::preparation::Outline,
        fitting: bool,
        italic: bool,
        bold: bool,
    ) -> Result<PreparedGlyph, String> {
        // A delayed job may arrive after another shared session already prepared this key.
        if let Some(handle) = self.get_for_placement(placement, [fitting, italic, bold]) {
            return Ok(PreparedGlyph::Vector(handle));
        }
        if self.grid.bitmap(placement, &ink, [fitting, italic, bold]) {
            // Transfer the prepared geometry, avoiding live vector pages for small grid ink.
            return Ok(PreparedGlyph::GridOutline {
                ink: Box::new(ink),
                placement: *placement,
                style: [fitting, italic, bold],
            });
        }
        let bytes = grid::vector_bytes(&ink);
        let crate::preparation::Outline {
            path,
            shape: geometry,
            bands,
        } = ink;
        // Prepared geometry is transferred to the persistent mirror. The original
        // path remains available only until packing chooses vector or oracle ink.
        let id = self
            .next_shape
            .checked_add(1)
            .ok_or("Slug glyph identity space exhausted")?;
        let shape = ShapeKey(id);
        // Both texture planes must fit in one page pair; insertion cannot reclaim
        // a page after any row of this frame has captured its shader addresses.
        let handle = match self.atlas.prepare_quadratics(
            shape,
            geometry,
            bands,
            FillRule::NonZero,
            self.colors.frame,
        ) {
            Ok(handle) => handle,
            Err(puzzle_slug::atlas::AtlasError::Capacity) => {
                // Current-frame handles remain stable; only the next boundary may evict.
                let samples = self
                    .oracle
                    .get_or_insert_with(Default::default)
                    .path(placement, &path, fitting, italic, bold)?;
                return Ok(PreparedGlyph::Rasterized(samples));
            }
            // OpenType contextual ligatures may substitute an empty glyph
            // before the component that paints the combined symbol.
            // Its source cell remains occupied, but an empty path has no
            // band header and must never become a GPU draw instance.
            Err(puzzle_slug::atlas::AtlasError::Shape(puzzle_slug::shape::ShapeError::Empty)) => {
                return Ok(PreparedGlyph::Empty);
            }
            Err(error) => {
                return Err(format!(
                    "face {} glyph {}: {error}",
                    placement.key.face, placement.key.glyph_id
                ));
            }
        };
        // Overflow did not publish a vector address; only successful packing owns this ID.
        self.next_shape = id;
        self.shapes.insert(placement.key, shape);
        self.vector_sizes.insert(placement.key, bytes);
        Ok(PreparedGlyph::Vector(handle))
    }
}

/// Bitmap-only affine transforms preserve the forty-byte raster instance format.
mod raster_effects;
/// Row batches carry one recency stamp for ordinary shapes and outer COLR clips.
mod retention;
/// Owned platform outlines enter the existing key/atlas cache without a font copy.
mod source;
#[cfg(test)]
/// Pressure must preserve covered ink through the existing backend seam.
mod tests;

/// Desktop per-glyph byte comparison stays independent of GPU ownership.
mod grid;

impl SlugBackend {
    /// Enables vector-handle observation without changing atlas generations.
    pub fn measure_cache(&mut self) {
        // Repeated per-frame activation preserves cumulative interval differences.
        self.metrics.get_or_insert_with(Default::default);
    }
    /// Copies cumulative valid-handle lookup counts.
    pub fn cache_metrics(&self) -> crate::cache_metrics::CacheMetrics {
        self.metrics.unwrap_or_default()
    }
}
