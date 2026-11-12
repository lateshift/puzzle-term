//! Startup pack handles remain pinned while the full font is unavailable (§8.8).
//!
//! Page eviction never relocates ink, so registration needs no decoded copy
//! of the contours or bands. Structural address checks establish that every
//! referenced texel belongs to one page pair. Validated takeover releases its
//! pins; those pages then participate in ordinary least-recent-use eviction.
use super::{
    AtlasError, PAGE_ROWS, PAGE_TEXELS, ShapeHandle, ShapeKey, SlugAtlas, StoredShape,
    TEXTURE_WIDTH,
};

/// Prebuilt handles share ordinary identity, page generation and pin lifetimes.
impl SlugAtlas {
    /// Registers imported ink and pins its page until full-font takeover.
    pub fn register_packed(
        &mut self,
        key: ShapeKey,
        mut handle: ShapeHandle,
        frame: u64,
    ) -> Result<ShapeHandle, AtlasError> {
        // Band location determines the only page allowed to own all dependent data.
        let page = usize::from(handle.glyph_loc[1]) / PAGE_ROWS;
        // Reject cross-page pack addresses before they acquire pinned lifetime metadata.
        self.validate_packed_page(handle, page)?;
        // Additional style packs use ordinary allocation before registration.
        // An existing identity must still acquire startup protection; otherwise
        // a style placed on its own page could be cleared before fonts arrive.
        if let Some(stored) = self.entries.get(&key) {
            let handle = stored.handle;
            self.pages[usize::from(handle.glyph_loc[1]) / PAGE_ROWS].pinned = true;
            self.pinned.insert(key);
            return Ok(handle);
        }
        // All dependent curves and lists passed the same-page structural check.
        // No outline extraction, band building or extra source cache is needed.
        handle.generation = self.pages[page].generation;
        // Row batches resolve these shader locations to the same registered shape identity.
        self.locations
            .insert(crate::encoding::encode_glyph_loc(handle.glyph_loc), key);
        // The atlas owns generation validation for imported and runtime outlines alike.
        self.entries.insert(
            key,
            StoredShape {
                handle,
                last_used: frame,
            },
        );
        // Pinning protects the entire page pair, including currently unused pack glyphs.
        let page = &mut self.pages[page];
        page.keys.push(key);
        page.last_used = page.last_used.max(frame);
        page.pinned = true;
        self.pinned.insert(key);
        Ok(handle)
    }

    /// Reports whether startup still relies exclusively on pinned pack geometry.
    pub fn has_pack_pins(&self) -> bool {
        !self.pinned.is_empty()
    }

    /// Releases protection only after the replacement tables and metrics validate.
    pub fn release_pack_pins(&mut self) {
        // Retained validated pack bytes reconstruct subsequently evicted pack shapes.
        self.pinned.clear();
        // Clearing protection does not move addresses or require a GPU upload.
        for page in &mut self.pages {
            page.pinned = false;
        }
    }

    /// Checks B1/B3 and proves that one page owns all of this pack glyph's data.
    fn validate_packed_page(&self, handle: ShapeHandle, page: usize) -> Result<(), AtlasError> {
        // Instance fields encode maximum indices, so both axes need one extra header.
        let (max_x, max_y, _) = crate::encoding::decode_band_info(handle.band_info);
        let count = usize::from(max_x) + usize::from(max_y) + 2;
        // Linear addressing is checked against imported extents before taking slices.
        let base =
            usize::from(handle.glyph_loc[0]) + usize::from(handle.glyph_loc[1]) * TEXTURE_WIDTH;
        // Texture capacity can exceed used pack data and cannot validate missing texels.
        let end = (page + 1) * PAGE_TEXELS;
        // Imported extents, rather than allocated zero-filled reserve, are authoritative.
        if usize::from(handle.glyph_loc[0]) + count > TEXTURE_WIDTH
            || base + count > self.bands.high_water.min(end)
        {
            return Err(AtlasError::InvalidCurveIndex);
        }
        // Each header references one B3 list whose start can wrap but entries cannot.
        for &[len, offset] in &self.bands.texels[base..base + count] {
            // List offsets are relative to glyphLoc rather than to their header.
            let start = base + usize::from(offset);
            let stop = start + usize::from(len);
            if stop > self.bands.high_water.min(end)
                || start % TEXTURE_WIDTH + usize::from(len) > TEXTURE_WIDTH
            {
                return Err(AtlasError::InvalidCurveIndex);
            }
            // All list entries must point to complete curve pairs in this same page.
            for &[x, y] in &self.bands.texels[start..stop] {
                // The second endpoint texel must exist before pinning a curve reference.
                let curve = usize::from(y) * TEXTURE_WIDTH + usize::from(x);
                // C1's second texel cannot wrap, and curves cannot belong to a
                // different page that a later eviction could clear independently.
                if usize::from(x) >= TEXTURE_WIDTH - 1
                    || usize::from(y) / PAGE_ROWS != page
                    || curve + 1 >= self.curves.high_water
                {
                    return Err(AtlasError::InvalidCurveIndex);
                }
            }
        }
        Ok(())
    }
}
