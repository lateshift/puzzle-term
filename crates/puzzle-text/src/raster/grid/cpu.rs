//! Outline-derived CPU crops share the disposable grid cache's ownership policy.
//!
//! This also covers D-39 vector-atlas overflow, not just the D-47 preferred mask
//! path. Fonts or procedural geometry can regenerate these samples after loss.
//! Only pending uploads own their bytes; image-only insertion keeps its mirror.
//! Web contexts retain their existing restoration contract until the wasm phase.
//! Reservation records a generated page incarnation, so capacity captures preserve
//! earlier ink even when CPU crops share a page with GPU coverage or authored ink.
use super::{BitmapKind, RasterAtlas, RasterHandle, RasterKey};
use crate::glyph::RasterGlyph;

/// The atlas publishes source-backed coverage with its ordinary ordered upload ownership.
impl RasterAtlas {
    /// Admits complete outline-derived samples without a native permanent CPU mirror.
    /// Non-synchronous clients retain their prior bitmap ownership and upload policy.
    /// Invalid byte counts are rejected before publishing shelf metadata or a handle.
    pub fn prepare_outline_mask(
        &mut self,
        key: RasterKey,
        glyph: RasterGlyph,
    ) -> Result<RasterHandle, String> {
        self.prepare_mask_crop(key, glyph, 1)
    }

    /// The explicit grid path never serves effects and may omit interpolation padding.
    pub(in crate::raster) fn prepare_grid_mask(
        &mut self,
        key: RasterKey,
        glyph: RasterGlyph,
    ) -> Result<RasterHandle, String> {
        self.prepare_mask_crop(key, glyph, 0)
    }

    /// Both CPU producers retain queue ordering; only their sampling contract differs.
    fn prepare_mask_crop(
        &mut self,
        key: RasterKey,
        glyph: RasterGlyph,
        gutter: u32,
    ) -> Result<RasterHandle, String> {
        if !self.synchronous {
            return self.prepare(key, glyph);
        }
        // Only monochrome analytic coverage has this reproducible-source contract.
        if glyph.kind != BitmapKind::Mask
            || glyph.width.checked_mul(glyph.height).map(|n| n as usize) != Some(glyph.pixels.len())
        {
            return Err("invalid outline-derived coverage samples".into());
        }
        if let Some(handle) = self.get(key) {
            return Ok(handle);
        }
        // This lazy neutral owner marks source reproducibility even without a GPU adapter.
        self.grid.oracle.get_or_insert_with(Default::default);
        // Bounds, page captures and in-build reuse are shared with the GPU reservation.
        let handle = self.reserve_grid(key, glyph.width, glyph.height, glyph.offset_px, gutter)?;
        // The owned crop survives until the executor acknowledges its ordered upload.
        self.dirty
            .last_mut()
            .ok_or("outline crop reservation absent")?
            .pixels = glyph.pixels;
        // Loss removes this key so its outline source supplies fresh samples on demand.
        self.grid.outlines.insert(key);
        Ok(handle)
    }
}
