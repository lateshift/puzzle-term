//! Band headers and deduplicated curve lists obey B1 and B2 (§9.2–§9.3).
//!
//! Headers and each individual list stay inside one row because the
//! reference shader adds an index to x without wrapping. Only the offset
//! from the glyph header to a list is linear and may cross rows.
//!
//! The first `h + v` texels are headers. Each holds a 16-bit curve count and
//! a 16-bit offset from the shape's first header to its curve-location list.
//! Header access uses `glyph.x + bandIndex`, so those texels must fit on one
//! row (B1). `CalcBandLoc` can wrap the offset to the list's start; after
//! that, the shader again adds indices to x without wrapping. The complete
//! list must also fit inside one row, even though successive lists may be
//! on later rows.
//!
//! One shape's offset is limited to 65535 texels (B2), independent of the
//! total atlas size. Large overlay outlines are split in their own later
//! milestone instead of truncating an index here. Identical lists can share
//! one location, reducing both memory and future GPU upload volume. This
//! deduplication does not alter the order the shader visits curves.
use super::{AtlasError, SlugAtlas, TEXTURE_WIDTH};
use crate::bands::BandSet;

/// Packs both axes after their referenced curve locations exist.
impl SlugAtlas {
    /// Returns the header origin used as the instance's `glyph_loc`.
    pub(super) fn pack_bands(
        &mut self,
        bands: &BandSet,
        curves: &[[u16; 2]],
    ) -> Result<[u16; 2], AtlasError> {
        let headers = bands.horizontal.len() + bands.vertical.len();
        // The header count itself must fit a row, even with a fresh origin.
        if headers > TEXTURE_WIDTH {
            return Err(AtlasError::BandBlockTooLarge);
        }
        // B1: the shader reads headers as glyph.x + band, without wrapping.
        if self.bands.cursor % TEXTURE_WIDTH + headers > TEXTURE_WIDTH {
            self.bands.align_row();
        }
        let start = self.bands.cursor;
        self.bands
            .ensure(start + headers, self.curves.bytes(), self.budget_bytes)?;
        self.bands.cursor += headers;
        let mut unique: Vec<(&[usize], u16)> = Vec::new();
        // Store an offset for every band header, but write each distinct
        // location list only once within this shape's B2 address window.
        for (index, list) in bands.horizontal.iter().chain(&bands.vertical).enumerate() {
            let offset = if list.is_empty() {
                0
            } else if let Some((_, offset)) = unique
                .iter()
                .find(|(previous, _)| *previous == list.as_slice())
            {
                *offset
            } else {
                let offset = self.write_list(start, list, curves)?;
                unique.push((list, offset));
                offset
            };
            self.bands.texels[start + index] = [list.len() as u16, offset];
            self.bands.mark(start + index);
        }
        Ok([
            (start % TEXTURE_WIDTH) as u16,
            (start / TEXTURE_WIDTH) as u16,
        ])
    }

    /// Writes one unique list in a row and returns its 16-bit B2 offset.
    fn write_list(
        &mut self,
        glyph_start: usize,
        list: &[usize],
        curves: &[[u16; 2]],
    ) -> Result<u16, AtlasError> {
        if list.len() > u16::MAX as usize || list.len() > TEXTURE_WIDTH {
            return Err(AtlasError::BandBlockTooLarge);
        }
        // Reference loops use `listBase.x + curveIndex`, so even a list
        // whose base wrapped via CalcBandLoc cannot cross a row itself.
        if self.bands.cursor % TEXTURE_WIDTH + list.len() > TEXTURE_WIDTH {
            self.bands.align_row();
        }
        let start = self.bands.cursor;
        let end = start
            .checked_add(list.len())
            .ok_or(AtlasError::BandBlockTooLarge)?;
        // B2 constrains the last addressed list texel, not just its start.
        if end - glyph_start > u16::MAX as usize {
            return Err(AtlasError::BandBlockTooLarge);
        }
        self.bands
            .ensure(end, self.curves.bytes(), self.budget_bytes)?;
        for (at, &curve) in list.iter().enumerate() {
            let location = curves.get(curve).ok_or(AtlasError::InvalidCurveIndex)?;
            self.bands.texels[start + at] = *location;
            self.bands.mark(start + at);
        }
        self.bands.cursor = end;
        Ok((start - glyph_start) as u16)
    }
}
