//! Connects retained row draws to D-39's batched shape recency.
//!
//! The text layer owns paint records, while the geometry atlas owns shape keys.
//! Registering a rebuilt row resolves both its visible quads and outer clips.
//! Subsequent unchanged draws stamp only the token; the lazy iterator is untouched.
use super::SlugBackend;

/// Usage registration stays independent of the GPU's instance type.
impl SlugBackend {
    /// Stamps one row batch, enumerating locations only after a row rebuild.
    /// Paint clips are dependencies too: their curves are sampled without a draw quad.
    pub fn use_row_batch(
        &mut self,
        token: &mut u64,
        draws: impl Iterator<Item = (u32, u32)>,
        changed: bool,
    ) {
        // Clip records can reference geometry not represented by the visible quad.
        let paints = &self.colors.paints;
        // This iterator is evaluated only when the row batch needs registration.
        let locations =
            draws.flat_map(|(loc, paint)| std::iter::once(loc).chain(paints.clip_locations(paint)));
        if changed {
            // refresh_batch reuses the old key Vec on a warmed changed row.
            self.atlas
                .refresh_batch(token, locations, self.colors.frame);
        } else {
            // A cache hit updates the batch clock without traversing glyph instances.
            self.atlas.use_batch(token, locations, self.colors.frame);
        }
    }
}
