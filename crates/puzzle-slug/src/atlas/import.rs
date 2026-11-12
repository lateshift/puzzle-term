//! Reserves prebuilt texels in the ordinary CPU atlas (§8.8).
//!
//! Font-pack handles already address these planes. Copying them at offset
//! zero and advancing the allocation cursors lets subsequent procedural
//! glyphs use the same atlas without overwriting the first-frame font.
//! The caller validates handle addresses before importing; this layer
//! checks the combined memory bound and emits row-local upload spans.
use super::{AtlasError, DirtySpan, SlugAtlas, TEXTURE_WIDTH};

/// Adds a pack before the atlas admits any runtime outline.
impl SlugAtlas {
    /// Imports curve and band planes into a new bounded atlas.
    pub fn from_texels(curves: &[[f32; 4]], bands: &[[u16; 2]]) -> Result<Self, AtlasError> {
        Self::from_texels_with_budget(curves, bands, super::DEFAULT_BUDGET_BYTES)
    }

    /// Imports a validated pack under the same caller budget as runtime outlines.
    /// Small-budget hosts/tests still reserve both reference planes' minimum rows.
    pub fn from_texels_with_budget(
        curves: &[[f32; 4]],
        bands: &[[u16; 2]],
        budget: usize,
    ) -> Result<Self, AtlasError> {
        let mut atlas = Self::with_budget(budget)?;
        // Grow both planes before copying, keeping the same shared budget
        // that runtime preparation observes. There are no address offsets.
        let page_count = curves
            .len()
            .max(bands.len())
            .div_ceil(super::PAGE_TEXELS)
            .max(1);
        while atlas.pages.len() < page_count {
            atlas.add_page()?;
        }
        atlas.curves.texels[..curves.len()].copy_from_slice(curves);
        atlas.bands.texels[..bands.len()].copy_from_slice(bands);
        atlas.curves.cursor = curves.len();
        atlas.bands.cursor = bands.len();
        atlas.curves.high_water = curves.len();
        atlas.bands.high_water = bands.len();
        // Unused tails remain available without changing imported addresses.
        for (index, page) in atlas.pages.iter_mut().enumerate() {
            let start = index * super::PAGE_TEXELS;
            let end = start + super::PAGE_TEXELS;
            page.curve_end = curves.len().clamp(start, end);
            page.band_end = bands.len().clamp(start, end);
        }
        // Imported rows are complete dirty intervals, including physical gaps.
        // mark() coalesces adjacent writes only: marking the two endpoints
        // would omit their interior and leave most pack glyphs blank on GPU.
        atlas.curves.dirty = imported_spans(curves.len());
        atlas.bands.dirty = imported_spans(bands.len());
        Ok(atlas)
    }
}

/// Each upload covers one occupied row, with an exact partial final extent.
fn imported_spans(len: usize) -> Vec<DirtySpan> {
    // from_texels already checked allocation bounds, so row indices fit u16.
    (0..len)
        .step_by(TEXTURE_WIDTH)
        .map(|start| DirtySpan {
            row: (start / TEXTURE_WIDTH) as u16,
            start: 0,
            end: (len - start).min(TEXTURE_WIDTH) as u16,
        })
        .collect()
}
