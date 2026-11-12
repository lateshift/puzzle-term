//! Atomic image admission charges decoded pixels and device filtering borders.
//! Only nonvisible images can be displaced by a new transmission. Feasibility
//! is checked before retiring any source, preserving old pixels on ENOSPC.
use super::{ImageError, Store};
use crate::vt::Terminal;

/// All source accounting lives on the same exclusive owner as placements.
impl Store {
    /// Device replacement recomputes charges before admitting subsequent output.
    pub fn set_texture_limit(&mut self, limit: u32, terminal: &mut Terminal) {
        // Real GPU limits leave room for a logical texel and both filtering borders.
        // Clamp malformed host values rather than allowing zero-step tile arithmetic.
        self.texture_limit = limit.max(3);
        self.charged = 0;
        // Device replacement can change tile borders without changing decoded pixels.
        // Recompute only on this control message, never once per source per frame.
        for image in &mut self.images {
            image.charge =
                puzzle_frame::image_resident_bytes(image.pixels.size, self.texture_limit)
                    .max(image.pixels.rgba.len());
            self.charged = self.charged.saturating_add(image.charge);
        }
        // A smaller replacement device is equivalent to a live allowance shrink:
        // sources that no longer fit are retired before the next sealed publication.
        self.set_limit(self.limit, terminal);
    }

    /// The shared partition includes exactly the texels uploaded by the renderer.
    pub(crate) fn charge(&self, size: [u32; 2]) -> Result<usize, ImageError> {
        // Validate dimensions and address-space multiplication before computing halos.
        // RGBA length alone would undercharge images split by the hardware limit.
        let decoded = super::pixel_bytes(size, 4)?;
        Ok(decoded.max(puzzle_frame::image_resident_bytes(size, self.texture_limit)))
    }

    /// New sources may replace their own ID, but cannot evict other visible ink.
    pub(crate) fn make_room(
        &mut self,
        charge: usize,
        id: u32,
        terminal: &mut Terminal,
    ) -> Result<(), ImageError> {
        // Replacement credit includes its old visible generation. Other candidates
        // must have no visible placement, exactly as the reviewer's admission rule.
        let eligible = |image: &super::Image| {
            id != 0 && image.id == id || !self.visible(image.generation, terminal)
        };
        // Also bound metadata: many tiny images must not create an unbounded table.
        let entries = (self.limit / std::mem::size_of::<super::Image>()).max(1);
        // Credit only sources that this admission is actually allowed to retire.
        // Counting all stored bytes here would falsely promise space in visible ink.
        let reclaim: usize = self
            .images
            .iter()
            .filter(|i| eligible(i))
            .map(|i| i.charge)
            .sum();
        let removable = self.images.iter().filter(|i| eligible(i)).count();
        // Test the complete candidate set before modifying any image or anchor.
        if self.charged - reclaim > self.limit - charge || self.images.len() - removable >= entries
        {
            return Err(ImageError::NoSpace);
        }
        if id != 0
            && let Some(index) = self.images.iter().position(|i| i.id == id)
        {
            // Sealed publications retain their old Arc independently of this replacement.
            self.remove_image(index, terminal);
        }
        while self.charged > self.limit - charge || self.images.len() >= entries {
            // Feasibility above guarantees a candidate; an invariant failure still
            // returns an ordinary error rather than panicking on untrusted output.
            let index = self
                .images
                .iter()
                .enumerate()
                .filter(|(_, i)| !self.visible(i.generation, terminal))
                .min_by_key(|(_, i)| i.used)
                .map(|(index, _)| index)
                .ok_or(ImageError::NoSpace)?;
            self.remove_image(index, terminal);
            // Retire only enough LRU candidates to fit, preserving other warm sources.
        }
        Ok(())
    }
}
