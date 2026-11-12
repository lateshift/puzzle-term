//! Cold spare-bank growth belongs to grid warming, not the first exposed history row.
//!
//! Each bank remains bounded by the grid height plus its fractional edge (P10).
//! Reserving scratch copies no text, placements or handles: spare entries stay
//! invalid until a real row rebuild fills them. Their atlas usage token is warmed
//! with familiar dependencies so registration can also reuse its key allocation.
//! No glyph is prepared and no texture is uploaded by this storage reservation.
//! Larger new content can still grow a cache; familiar scrolling reuses the bank.
//!
//! Row identity and scratch lifetime differ: swapping a live cache into the
//! spare bank preserves its LineId, but a never-used spare needs capacity before
//! its first rebuild. Reserving from a visible row keeps that spare invalid.
//! Parallel flags/tints must grow with ink arrays, including cursor copies and
//! underline waves; reserving only the glyph Vec would leave hot allocations.
//! The atlas token similarly owns a retained dependency-key Vec. Its initial
//! registration may conservatively stamp familiar live shapes once; subsequent
//! recency stamps belong only to real rows drawn in later frames.
use super::{CachedRow, FrameBuilder, context::TextContext};

/// Empty spare entries acquire storage without becoming valid copies of live rows.
impl FrameBuilder {
    /// Warms new spare slots against the largest current row, once per slot.
    pub(super) fn warm_spares(&mut self, context: &mut TextContext) {
        let (visible, spare) = self.rows.split_at_mut(self.visible_rows);
        // Empty screens need no glyph storage; later real output supplies its size.
        let Some(source) = visible.iter().max_by_key(|row| row.placements.len()) else {
            return;
        };
        if source.placements.is_empty() {
            return;
        }
        for row in spare {
            if row.id.is_some() || row.shape_batch != 0 {
                continue;
            }
            // Familiar ink reserves parallel arrays without cloning cached geometry.
            row.reserve_like(source);
            // Registering a new token grows its key Vec/map now, outside warm scrolling.
            context.backend.use_row_batch(
                &mut row.shape_batch,
                source
                    .glyphs
                    .iter()
                    .chain(&source.waves)
                    .map(|draw| (draw.glyph_loc, draw.paint)),
                true,
            );
        }
    }
}

/// Cache arrays grow together because one placement can expand into several ink layers.
impl CachedRow {
    /// Reserves the live row's warmed high-water capacities without publishing its identity.
    fn reserve_like(&mut self, source: &Self) {
        // This local expansion covers every parallel vector and preserves empty lengths.
        // Capacity is bounded by already-built visible ink, rather than scrollback size.
        /// Applies identical capacity reservation to all parallel cached-ink arrays.
        macro_rules! reserve {
            ($($field:ident),+ $(,)?) => {$(
                self.$field.reserve(source.$field.capacity().saturating_sub(self.$field.len()));
            )+};
        }
        reserve!(
            placements,
            sources,
            backgrounds,
            glyphs,
            rasters,
            raster_blink,
            raster_selection_ink,
            cursor_rasters,
            glyph_blink,
            glyph_palette,
            glyph_selection_ink,
            waves,
            wave_selection_ink,
            cursor_glyphs,
            cursor_palette,
            decorations
        );
    }
}
