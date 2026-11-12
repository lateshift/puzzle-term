//! Shareable font, shape and atlas state for one text configuration (D-30).
//!
//! Sessions retain their own VT rows and row caches, while a host keeps
//! exactly one context for each font settings, DPR and hinting choice. The
//! context is touched only on the frame-building thread and is passed by
//! mutable reference; parser workers need only copied cell snapshots.
//!
//! A generation names the atlas incarnation used by cached row handles.
//! Replacing the atlas after eviction or device loss increments it, so a
//! second session cannot draw a stale handle merely because its VT row
//! identity and style epoch stayed unchanged.

use super::geometry::measure;
use puzzle_text::{
    font_set::FontSet, glyph::slug::SlugBackend, layout::RowLayout, metrics::CellMetrics,
    shape::TextShaper,
};

/// Mutable text resources shared by every session with one display key.
pub struct TextContext {
    /// Loaded faces and fallback decisions outlive individual terminals.
    pub(super) fonts: Option<FontSet>,
    /// Startup cmap and source identity before the full font is available.
    pub(super) pack: Option<puzzle_text::pack::FontPack>,
    /// Shaping plans and run-cache entries serve all changed rows.
    pub(super) shaper: TextShaper,
    /// Slug atlas and CPU mirror receive each prepared outline once.
    pub(super) backend: SlugBackend,
    /// Size-specific source pixels and bounded page mirrors shared across sessions.
    pub(super) raster: puzzle_text::raster::RasterAtlas,
    /// Placement scratch is reused sequentially on the build thread.
    pub(super) layout: RowLayout,
    /// Natural annotation layout reuses grapheme/placement scratch.
    pub(super) free_layout: puzzle_text::layout::free::Layouter,
    /// Sized annotation ink is retained across dirty provider rebuilds.
    pub(super) free_text: super::overlays::text::Cache,
    /// Homogeneous run text is reused rather than allocated per cell.
    pub(super) run_text: String,
    /// Authoritative cluster byte ranges and physical columns, reused between runs.
    pub(super) run_cells: Vec<puzzle_text::layout::GridCell>,
    /// Explicit row probes retain scalar timings only; ordinary contexts have no clock.
    pub(super) row_metrics: Option<[u64; 4]>,
    /// Primary-face physical cell size for this font and monitor scale.
    metrics: CellMetrics,
    /// Logical size is part of the host's context lookup key.
    font_size: f32,
    /// Physical scale determines D-27's hinted-outline path.
    dpr: f32,
    /// Every atlas reset invalidates all sessions' row-cache handles.
    generation: u64,
    /// Per-atlas allowance is part of the host resource identity.
    glyph_budget_bytes: usize,
    /// Explicit diagnostics sample completion cost; ordinary contexts never read a timer here.
    completion_metrics: Option<completion::CompletionMetrics>,
    /// Frame-local publication admission is supplied by the host refresh clock.
    publication: publication::Publication,
}

impl TextContext {
    /// Validates font geometry before admitting a context to the host map.
    pub fn new(fonts: FontSet, font_size: f32, dpr: f32) -> Result<Self, String> {
        let metrics = measure(&fonts, font_size, dpr)?;
        Ok(Self {
            fonts: Some(fonts),
            pack: None,
            shaper: TextShaper::default(),
            backend: SlugBackend::default(),
            raster: puzzle_text::raster::RasterAtlas::default(),
            layout: RowLayout::default(),
            free_layout: puzzle_text::layout::free::Layouter::default(),
            free_text: super::overlays::text::Cache::default(),
            run_text: String::new(),
            run_cells: Vec::new(),
            metrics,
            font_size,
            dpr,
            generation: 1,
            glyph_budget_bytes: puzzle_slug::atlas::DEFAULT_BUDGET_BYTES,
            completion_metrics: None,
            row_metrics: None,
            publication: publication::Publication::default(),
        })
    }

    /// Starts a fresh opt-in row measurement without changing cache or glyph policy.
    pub fn measure_rows(&mut self) {
        // Reset elapsed row stages, while each cache keeps its cumulative lookup history.
        self.row_metrics = Some([0; 4]);
        // Independent counters distinguish fallback work from a stale downstream handle.
        self.shaper.measure_cache();
        self.raster.measure_cache();
        self.backend.measure_cache();
        if let Some(fonts) = &mut self.fonts {
            fonts.measure_caches();
        }
    }

    /// Metrics, clusters/fallback, shaping/placement and glyph preparation in nanoseconds.
    /// The last stage includes glyph-handle lookup and row instance emission;
    /// whole-frame assembly, uploads and surface acquisition are measured elsewhere.
    #[must_use]
    pub fn row_metrics(&self) -> Option<[u64; 4]> {
        self.row_metrics
    }

    /// Loads a user/test fallback and returns its explicit face ID for free text.
    /// Existing grid and annotation layouts rebuild before using the new chain.
    pub fn add_fallback(&mut self, data: puzzle_text::FontData) -> Result<u32, String> {
        let fonts = self.fonts.as_mut().ok_or("font tables not loaded")?;
        let id = fonts.add_fallback(data)?;
        // Adding a face changes fallback selection but preserves existing atlas shapes.
        self.generation = self.generation.wrapping_add(1);
        Ok(id)
    }

    /// Returns physical grid geometry without loading a second font copy.
    #[must_use]
    pub fn metrics(&self) -> CellMetrics {
        self.metrics
    }

    /// Non-frame diagnostics can await accepted fallback jobs without synchronous discovery.
    #[must_use]
    pub fn sources_pending(&self) -> bool {
        self.publication.ready() || self.fonts.as_ref().is_some_and(FontSet::sources_pending)
    }

    /// Names the current atlas incarnation for each session's row cache.
    #[must_use]
    pub fn atlas_generation(&self) -> u64 {
        self.generation
    }

    /// Evicts cold paint rows before damage capture decides which cells to copy.
    pub fn begin_frame(&mut self) -> Result<(), String> {
        let started = web_time::Instant::now();
        // Counter differences identify boundary reclamation without per-shape logs.
        let before = self.backend.atlas().evictions();
        let paints_changed = self.backend.begin_color_frame()?;
        if self.backend.atlas().evictions() != before {
            // Rare pressure boundaries are measured separately from ordinary row assembly.
            tracing::debug!(
                event = "text.atlas_page_evicted",
                evictions = self.backend.atlas().evictions(),
                shapes = self.backend.entry_count(),
                bytes = self.backend.atlas().allocated_bytes(),
                elapsed_ms = started.elapsed().as_secs_f64() * 1000.0
            );
        }
        // Raster pages have their own generation and follow the same frame boundary.
        let rasters_changed = self.raster.begin_frame();
        // Publish ownership before any row collects shader addresses for this frame.
        self.process_source_completions();
        if let Some(fonts) = self.fonts.as_mut() {
            // Publication, page eviction and cold draws share one refresh allowance.
            // Elapsed wall time also charges atlas preparation between source calls.
            fonts.begin_source_frame(self.publication.budget.saturating_sub(started.elapsed()));
        }
        if paints_changed || rasters_changed {
            // Every session observes page eviction through its existing epoch check.
            self.generation = self.generation.wrapping_add(1);
        }
        Ok(())
    }

    /// Counts prepared glyph keys across all sessions sharing this context.
    #[must_use]
    pub fn prepared_glyphs(&self) -> usize {
        self.backend.entry_count() + self.raster.entry_count()
    }

    /// Reports allocated raster CPU pages; GPU capacity follows the same lazy pages.
    #[must_use]
    pub fn raster_bytes(&self) -> usize {
        self.raster.allocated_bytes()
    }

    /// Reports successful between-frame page evictions and charged vector memory.
    /// Development observers compare these counters across a complete flood interval.
    #[must_use]
    pub fn vector_pressure(&self) -> (u64, usize) {
        (
            self.backend.atlas().evictions(),
            self.backend.atlas().allocated_bytes(),
        )
    }

    /// Last vector publication's contiguous write counts before and after merging.
    pub fn vector_upload_spans(&self) -> [usize; 2] {
        self.backend.atlas().upload_spans()
    }

    /// Replaces all atlas handles after eviction or a lost GPU texture.
    pub fn reset_atlas(&mut self) {
        // The new generation forces row rebuilds before any old handle
        // can be submitted again, including in another idle terminal.
        self.backend.atlas_mut().restore();
        self.backend.restore_paints();
        self.raster.restore();
        self.generation = self.generation.wrapping_add(1);
    }

    /// Installs browser/native implementations after full font takeover.
    pub fn set_cluster_sources(
        &mut self,
        emoji: Box<dyn puzzle_text::cluster_source::EmojiSource>,
        platform: Box<dyn puzzle_text::cluster_source::PlatformFallback>,
    ) -> Result<(), String> {
        let fonts = self.fonts.as_mut().ok_or("font tables not loaded")?;
        fonts.set_emoji_source(emoji);
        fonts.set_platform_fallback(platform);
        // Only platform registry IDs can name different ink after replacement.
        // Pack and loaded-face outlines remain ready through takeover (P6, §8.8).
        // Source texels age out through boundary-only LRU; they never move here.
        self.backend.invalidate_sources();
        self.raster.replace_budget(self.glyph_budget_bytes);
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }

    /// Loaded fonts use the host's same cold preparation policy as platform fallback.
    pub fn set_preparation_driver(
        &mut self,
        driver: Box<dyn puzzle_text::preparation::Driver>,
    ) -> Result<(), String> {
        self.fonts
            .as_mut()
            .ok_or("font tables not loaded")?
            .set_preparation_driver(driver);
        Ok(())
    }

    /// Returns the logical size used to key hinted glyph outlines.
    #[must_use]
    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    /// Returns the physical scale used by this context's glyph cache.
    #[must_use]
    pub fn dpr(&self) -> f32 {
        self.dpr
    }

    /// Distinguishes pack-only startup from loaded shaping/fallback font tables.
    #[must_use]
    pub fn has_font_tables(&self) -> bool {
        self.fonts.is_some()
    }

    /// Revalidates a single host's context when its monitor scale changes.
    pub fn reconfigure(&mut self, font_size: f32, dpr: f32) -> Result<(), String> {
        // A failed scale leaves the old context usable. Hinted keys carry
        // ppem, so existing outlines may remain while new rows use this DPR.
        let metrics = match &self.fonts {
            Some(fonts) => measure(fonts, font_size, dpr)?,
            None => pack::metrics(self.pack.as_ref().ok_or("no font source")?, font_size, dpr)?,
        };
        self.metrics = metrics;
        self.font_size = font_size;
        self.dpr = dpr;
        Ok(())
    }
}

/// Completion timings are public data so platform observers can report frame work.
pub use completion::CompletionMetrics;
/// Host-derived elapsed-time admission for already prepared source results.
mod publication;

/// Pack-first construction and validated full-font takeover.
mod pack;

/// Validated memory policy shared by settings and platform resource owners.
pub mod budget;
/// Owned asynchronous ink enters caches without requiring its original visible row.
mod completion;
/// Owned source geometry publication preserves backend-neutral identities.
mod source_ink;
/// Lock-free complete environment preparation supplied by native hosts.
mod synchronous;

#[cfg(test)]
/// Startup and takeover regressions with platform-owned fixture files.
mod tests;

impl TextContext {
    /// Shape, selection, coverage, source-key, raster and vector counters.
    pub fn cache_metrics(&self) -> [[u64; 6]; 6] {
        // Pack-only startup has no font-selection owner; absent layers report zero.
        let fonts = self
            .fonts
            .as_ref()
            .map(FontSet::cache_metrics)
            .unwrap_or_default();
        // Coverage reports eligibility positives/negatives on immutable native tables.
        // Source-key counts separately expose string hashing versus reversible scalars.
        [
            self.shaper.cache_metrics(),
            fonts[0],
            fonts[1],
            fonts[2],
            self.raster.cache_metrics(),
            self.backend.cache_metrics(),
        ]
        .map(|metrics| metrics.values())
    }
}
