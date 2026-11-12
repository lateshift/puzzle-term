//! Complete host sources retain index and mapping ownership across policy changes.
//!
//! Hosts supply a complete preparation owner through the neutral text seam.
//! Core knows neither native catalogs nor threads. Updating source policy leaves
//! bundled faces loaded, but source-derived handles and row epochs must change.
//! Desktop grid coverage is an explicit host choice over the D-28 seam.
//! The GPU producer is owned beside the shared raster atlas, never by Session.
//! Family-policy changes drop masks; geometry-only changes reuse keyed variants.
//! The host then reinstalls its current device-bound producer on the render thread.
//! Neutral clients can select the CPU oracle without creating any GPU object.
//! Font renderer changes advance the same row-cache epoch as other ink changes.
use super::TextContext;
use puzzle_text::cluster_source::synchronous::SynchronousSources;

/// Owns invalidation when a host selects complete preparation instead of continuations.
impl TextContext {
    /// Installs one complete source without introducing platform knowledge in core.
    pub fn set_synchronous_sources(
        &mut self,
        source: Box<dyn SynchronousSources>,
    ) -> Result<(), String> {
        // Installation needs the primary metrics established by the loaded font set.
        self.fonts
            .as_mut()
            .ok_or("font tables not loaded")?
            .set_synchronous_sources(source);
        // Existing rows may still contain asynchronous source handles.
        self.invalidate_source_policy()?;
        Ok(())
    }
    /// Reuses the source while applying a new family order or physical cell geometry.
    pub fn configure_synchronous_sources(&mut self, families: &[String]) -> Result<(), String> {
        let changed = self
            .fonts
            .as_mut()
            .ok_or("font tables not loaded")?
            .configure_synchronous_sources(families, self.metrics);
        // Window extents and equal-policy monitor moves cannot retire ready ink (D-30).
        if changed {
            self.invalidate_source_policy()?;
        }
        Ok(())
    }
    /// Invalidates future snapshots only after this build has finished collecting addresses.
    pub(in crate::frame_builder) fn finish_raster_build(&mut self, previous: u64) {
        if self.raster.generation() != previous {
            self.generation = self.generation.wrapping_add(1);
        }
    }
    /// Source policy replacement invalidates ink identities while preserving bundled glyphs.
    fn invalidate_source_policy(&mut self) -> Result<(), String> {
        tracing::debug!(event="text.source_policy_reset", generation=self.generation,
            cache=?self.grid_cache_metrics(), size=self.font_size, dpr=self.dpr);
        // Existing source keys may select another family; loaded primary faces stay valid.
        self.backend.invalidate_sources();
        self.backend
            .set_grid_renderer(puzzle_text::glyph::RendererChoice::Auto);
        // Family identity changes retire raster entries; size variants otherwise coexist.
        // Native grid masks share 32 MiB; one 16-MiB emoji page keeps the total at 48.
        self.raster = puzzle_text::raster::RasterAtlas::with_budget(self.glyph_budget_bytes, true);
        self.raster.set_synchronous();
        // D-47's batch packer owns one additional vector page, inside this allowance.
        self.backend
            .restrict_budget(self.glyph_budget_bytes - puzzle_slug::atlas::MIN_BUDGET_BYTES)?;
        // Per-session rows sharing this context reject all old source handles.
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }
}

impl TextContext {
    /// Installs the native GPU coverage producer without giving layout GPU knowledge.
    pub fn set_grid_rasterizer(
        &mut self,
        rasterizer: Box<dyn puzzle_text::raster::GridRasterizer>,
    ) -> Result<(), String> {
        self.raster.set_grid_rasterizer(rasterizer)
    }
    /// The desktop host applies the validated renderer setting and invalidates row handles.
    pub fn set_grid_renderer(&mut self, choice: puzzle_text::glyph::RendererChoice) {
        self.backend.set_grid_renderer(choice);
        self.generation = self.generation.wrapping_add(1);
    }
}

impl TextContext {
    /// Reports bitmap population, physical atlas capacity and evictions for the D-47 curve.
    pub fn grid_cache_metrics(&self) -> [u64; 3] {
        [
            self.raster.entry_count() as u64,
            self.raster.allocated_bytes() as u64,
            self.raster.evictions(),
        ]
    }
}
