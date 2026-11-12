//! First-frame font packs enter the same text context as loaded fonts.
//!
//! No TTF is needed for metrics, placement or atlas uploads. Takeover
//! preserves all packed unhinted style keys, while a context generation
//! invalidates rows so full-font shaping and non-pack D-27 hinting can begin.
//! Pack startup has no TTF owner; annotations use design advances until takeover.
use super::*;
use puzzle_text::{
    FontData,
    metrics::{CellOptions, MetricAdjustment},
    pack::FontPack,
};

/// Pack bootstrap preserves the host-owned D-30 resource boundary.
impl TextContext {
    /// Creates a usable context from validated pack bytes without a font.
    /// The directory and em-space metrics are the entire startup source.
    /// Shaping scratch starts empty because the pack maps scalars directly;
    /// complex substitutions wait for the full OpenType tables.
    pub fn from_pack(pack: FontPack, size: f32, dpr: f32) -> Result<Self, String> {
        let metrics = metrics(&pack, size, dpr)?;
        let backend = SlugBackend::from_pack(&pack)?;
        // The ordinary backend owns imported texels, so the first upload
        // follows the same dirty-span path as a runtime-prepared glyph.
        Ok(Self {
            fonts: None,
            pack: Some(pack),
            backend,
            shaper: TextShaper::default(),
            raster: puzzle_text::raster::RasterAtlas::default(),
            layout: RowLayout::default(),
            free_layout: puzzle_text::layout::free::Layouter::default(),
            free_text: super::super::overlays::text::Cache::default(),
            run_text: String::new(),
            run_cells: Vec::new(),
            metrics,
            font_size: size,
            dpr,
            generation: 1,
            glyph_budget_bytes: puzzle_slug::atlas::DEFAULT_BUDGET_BYTES,
            completion_metrics: None,
            row_metrics: None,
            publication: super::publication::Publication::default(),
        })
    }

    /// Rejects an unrelated fetched font before reusing its pack's glyph IDs.
    /// Validation is transactional: a failed download or bad font leaves
    /// the pack visible. A successful fetch invalidates cached rows because
    /// ligatures, fallback and hinted contours can change the visible ink,
    /// even though source metrics preserve origins and advances.
    pub fn load_fonts(
        &mut self,
        regular: FontData,
        italic: Option<FontData>,
        symbols: Option<FontData>,
    ) -> Result<(), String> {
        if self
            .pack
            .as_ref()
            .is_some_and(|pack| !pack.matches_source(&regular))
        {
            return Err("font does not match preloaded pack".into());
        }
        let fonts = FontSet::new(regular, italic, symbols)?;
        // Every style's source hash and weight must match before shared glyph IDs
        // may use full-font shaping; one mismatched face rejects the whole takeover.
        self.backend.validate_packs(&fonts)?;
        let metrics = measure(&fonts, self.font_size, self.dpr)?;
        // Commit only after font tables and metrics passed validation.
        // Existing pack texels remain in the shared backend (P6).
        // A failed takeover above keeps all startup glyphs pinned. Only now
        // can LRU eviction reclaim unused pack pages; retained pack bytes restore ink.
        self.backend.release_pack_pins();
        self.fonts = Some(fonts);
        self.metrics = metrics;
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }

    /// Imports an approved style pack while the context is still in startup.
    /// Face slots follow FontSet's four primary styles; later shaping reuses them.
    pub fn add_font_pack(
        &mut self,
        pack: FontPack,
        style: puzzle_text::font_set::FontStyle,
    ) -> Result<(), String> {
        // Startup-only import prevents late assets changing the primary face slots
        // after row caches and shaping plans have acquired their full-font meaning.
        if self.fonts.is_some() {
            return Err("style packs must precede font takeover".into());
        }
        self.backend.add_pack(style as u32, pack)
    }
}

/// Uses exactly the same device geometry before and after font takeover.
pub(super) fn metrics(pack: &FontPack, size: f32, dpr: f32) -> Result<CellMetrics, String> {
    // A low-DPR context initially displays unhinted pack ink, but uses
    // the geometry of its eventual hinted face. Otherwise takeover would
    // move the grid and send an unnecessary PTY resize after startup.
    // High-DPR contexts instead use the same cap-height snap as full fonts.
    CellMetrics::from_pack(
        pack.metrics,
        CellOptions {
            size,
            dpr,
            line_height: 1.0,
            width: MetricAdjustment::Px(0.0),
            height: MetricAdjustment::Px(0.0),
            baseline: MetricAdjustment::Px(0.0),
            snap_cap_height: dpr >= 1.5,
            hinted: dpr < 1.5,
        },
    )
}
