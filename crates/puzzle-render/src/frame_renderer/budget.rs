//! CPU and GPU sampled stores share the text context's budget lifetime.
//! Shrinking retires old textures, which geometric growth cannot do in place.
use super::{FrameRenderer, Renderer};
impl FrameRenderer {
    /// Reports sampled raster allocation independently of transient ordered copies.
    pub fn raster_bytes(&self) -> usize {
        self.raster.allocated_bytes()
    }

    /// Recreates sampled stores only when policy changes, before fresh uploads.
    pub fn set_glyph_budget(&mut self, renderer: &Renderer, bytes: usize) -> bool {
        if self.glyph_budget_bytes == bytes {
            return false;
        }
        // Existing submissions own their textures until the GPU completes them.
        // The host discards obsolete pending spans and recaptures every row.
        let images = self.images.take();
        *self = Self::new(renderer, self.format);
        // Glyph policy must never discard unrelated image residency (D-30).
        self.images = images;
        self.glyph_budget_bytes = bytes;
        self.raster.set_budget(bytes);
        true
    }
}
