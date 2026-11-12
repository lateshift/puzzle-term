//! Transaction boundary for one shape's two texture planes (design §9.8).
//!
//! Band headers depend on curve addresses. If curve or band packing fails
//! halfway through, a later shape must not inherit consumed space or upload
//! spans from the abandoned attempt. The texel bytes themselves need no
//! clearing: restored cursors make them logically absent.
//! A dirty span can be extended in place by a new adjacent write. Saving
//! only the Vec length would leave that failed extension observable, so the
//! checkpoint retains the previous tail span as well.
use super::{AtlasError, SlugAtlas};
use crate::{bands::BandSet, shape::QuadShape};

/// Saves a dirty list's length and last span, which may be coalesced in place.
type DirtyCheckpoint = (usize, Option<super::DirtySpan>);

impl SlugAtlas {
    /// Packs curve addresses and dependent band lists as one logical write.
    pub(super) fn pack_transaction(
        &mut self,
        shape: &QuadShape,
        bands: &BandSet,
    ) -> Result<[u16; 2], AtlasError> {
        let curve_start = self.curves.cursor;
        let curve_high_water = self.curves.high_water;
        // Both planes have independent cursors and upload lists; neither
        // may advance if the other rejects the shape.
        let curve_dirty = (self.curves.dirty.len(), self.curves.dirty.last().copied());
        let band_start = self.bands.cursor;
        let band_high_water = self.bands.high_water;
        let band_dirty = (self.bands.dirty.len(), self.bands.dirty.last().copied());
        let packed = self
            .pack_curves(shape)
            .and_then(|locations| self.pack_bands(bands, &locations));
        if packed.is_err() {
            // A mark adjacent to an existing span can lengthen that span
            // rather than append one, so both its length and tail are saved.
            self.curves.cursor = curve_start;
            self.curves.high_water = curve_high_water;
            restore_dirty(&mut self.curves.dirty, curve_dirty);
            self.bands.cursor = band_start;
            self.bands.high_water = band_high_water;
            restore_dirty(&mut self.bands.dirty, band_dirty);
        }
        packed
    }
}

/// Restores the dirty spans a future GPU upload is allowed to observe.
fn restore_dirty(spans: &mut Vec<super::DirtySpan>, checkpoint: DirtyCheckpoint) {
    spans.truncate(checkpoint.0);
    if let (Some(saved), Some(last)) = (checkpoint.1, spans.last_mut()) {
        *last = saved;
    }
}
