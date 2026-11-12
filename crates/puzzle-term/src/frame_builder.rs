//! Damage-driven terminal frame construction without a GPU dependency (§10.4).
//!
//! A row cache holds device-pixel instances relative to the row's top edge.
//! Its line identity and version protect against both edits and scrolling;
//! the style epoch invalidates resolved colors when a theme or font changes.
//! Assembly shifts cached ink by the visible row's y offset, so scrolling
//! does not require shaping an otherwise unchanged logical line again.
//! The host's shared text context owns the Slug atlas: a glyph is prepared
//! once and its dirty texels enter `RenderFrame::uploads` before first draw.
//!
//! Row content and its screen position have different lifetimes. A logical
//! line can scroll upward without any of its cells changing; binding cached
//! Slug transforms to an absolute y coordinate would throw away that work.
//! The cache instead stores y=0 and assembly supplies the current top edge.
//! Its x coordinate is also relative to the grid padding, which lets the
//! surface resize without rebuilding every prepared glyph.
//!
//! A style epoch is separate from `Row::version` because palette and theme
//! changes do not edit the terminal's text. They still change GPU instance
//! colors, so a row must rebuild its colored geometry after such a change.
//! Fonts and DPR can additionally change the outline key and cell metrics.
//!
//! The frame arrays are sorted by z-slot rather than interleaved by row.
//! This is necessary for one background draw and one Slug draw independent
//! of terminal height. The renderer may therefore submit a small fixed
//! number of GPU draws even when thousands of cells are visible (P5).
use crate::frame_snapshot::FrameSnapshot;
use crate::session::cursor_settings::CursorSettings;
use crate::theme::Theme;
use context::TextContext;
use puzzle_frame::{RasterInstance, RectInstance, RenderFrame, SlugInstance};
use puzzle_text::metrics::CellMetrics;
use puzzle_vt::{terminal::Terminal, tracked::LineId};

/// Painter layers assemble cached ink and current selection geometry.
mod assembly;
mod colors;
/// Font, shaping and atlas resources shared by sessions with one text key.
pub mod context;
mod cursor;
/// Read-only retained-ink evidence keeps diagnostics off the assembly hot path.
mod diagnostics;
/// Immutable image placement compilation, separate from glyph caches.
mod images;
/// Sparse rectangular text uses ordinary cached glyph streams.
mod multicell;
/// Opt-in observers identify neutral source requests without preparing new ink.
pub use diagnostics::InkSample;
/// Shared physical-cell geometry used by native startup and frame assembly.
mod geometry;
/// Placement queries and retained provider painter output.
mod overlays;
/// Padding changes assembly origins without changing prepared glyphs.
mod padding;
/// Copy-only drawing preferences accompany caller-owned publications.
pub(crate) mod policy;
/// Raster assembly, selection partitioning and upload ownership.
mod raster;
mod row;
/// Match paper is assembled beneath selection and text.
mod search;
/// Selection clipping uses a bounded number of text draws.
mod selection;
/// Copy-only cache selection and frame construction for desktop snapshots.
mod snapshot;
/// Spare row scratch is warmed with the live grid, before scrolling exposes history.
mod storage;
mod uploads;

/// Completed phase boundaries let an opt-in observer attribute slow frame builds.
#[derive(Clone, Copy)]
#[repr(usize)]
pub enum BuildPhase {
    /// Dimension reset, cache matching and pinning cached raster dependencies.
    CacheOrder,
    /// Changed-row shaping, glyph lookup and atlas row-batch registration.
    Rows,
    /// Copies retained instances and adds selection, search and cursor geometry.
    Assembly,
    /// Publishes neutral placement queries for overlays and pointer handling.
    Geometry,
    /// Runs dirty providers and compiles their retained painter commands.
    Overlays,
    /// Collects ordered glyph, paint and raster writes into the pending frame.
    Uploads,
}

/// Cached geometry for one visible logical line, in row-relative pixels.
#[derive(Clone, Default)]
struct CachedRow {
    /// Stable line identity; a rotated grid row may retain its ink.
    id: Option<LineId>,
    /// Content revision from the VT row.
    version: u64,
    /// Block revisions and owner-role changes survive viewport rotation.
    block_signature: u64,
    /// Appearance revision that controls resolved colors and glyph size.
    style_epoch: u64,
    /// Atlas reset forces a new handle even for unchanged VT cells.
    atlas_generation: u64,
    /// One atlas recency stamp protects all shapes of an unchanged cached row (D-39).
    shape_batch: u64,
    /// Cursor column breaks ligatures even when the row's cells did not edit.
    cursor_column: Option<usize>,
    /// Neutral cluster metadata stays independent of the selected ink backend.
    placements: Vec<puzzle_overlay::geometry::Glyph>,
    /// Neutral requests survive effects so hinted grid ink can be re-prepared unhinted.
    sources: Vec<overlays::effects::Source>,
    /// Sparse sizing maps, absent on ordinary rows; no extra per-source storage.
    block_transforms: Vec<multicell::SourceMap>,
    /// Equal-background cell runs painted below glyphs.
    backgrounds: Vec<RectInstance>,
    /// Prepared Slug glyphs and procedural joining characters.
    glyphs: Vec<SlugInstance>,
    /// Cropped bitmap quads retain the same row-relative origin as vector ink.
    rasters: Vec<RasterInstance>,
    /// Blink policy follows each bitmap through cache-hit assembly.
    raster_blink: Vec<bool>,
    /// Selected mask tint, resolved against selection paper.
    raster_selection_ink: Vec<u32>,
    /// Bitmap copies isolated beneath a filled block cursor.
    cursor_rasters: Vec<RasterInstance>,
    /// Whether each matching glyph uses SGR 5 blinking ink.
    glyph_blink: Vec<bool>,
    /// Solid palette layers retain authored colors under selection and cursor tint.
    glyph_palette: Vec<bool>,
    /// Rule-6 ink is resolved before minimum contrast against selection paper.
    glyph_selection_ink: Vec<u32>,
    /// Curly underline shapes prepared once for each device-cell geometry.
    waves: Vec<SlugInstance>,
    /// Curly decorations share selection colors without changing their shapes.
    wave_selection_ink: Vec<u32>,
    /// Cursor-cell ink redrawn with cursorText above a block cursor.
    cursor_glyphs: Vec<SlugInstance>,
    /// Parallel palette mask for isolated cursor-cell color layers.
    cursor_palette: Vec<bool>,
    /// Underline, strike and overline geometry above backgrounds.
    decorations: Vec<RectInstance>,
}

/// Frame dimensions, colors and per-session reusable row geometry.
pub struct FrameBuilder {
    /// Per-frame logical queries reuse cached neutral placements.
    geometry: puzzle_overlay::geometry::TextGeometry,
    /// Callbacks and frozen data temporarily travel with external frame ownership.
    pub(crate) overlays: overlays::OverlayFrame,
    /// One cache entry per visible row; keys include line identity.
    rows: Vec<CachedRow>,
    /// Only this prefix participates in the current document slice.
    visible_rows: usize,
    /// Fractional movement is rounded once, then added to every grid layer.
    row_shift: i32,
    /// Device-pixel cell geometry shared by ink and backgrounds.
    metrics: CellMetrics,
    /// Logical font size before display scaling.
    font_size: f32,
    /// Physical pixels per logical pixel.
    dpr: f32,
    /// Increments when resolved appearance or metrics change.
    style_epoch: u64,
    /// Atlas incarnation last admitted into this terminal's row caches.
    context_generation: u64,
    /// Separate VT screen incarnations may contain the same numeric LineId.
    screen_epoch: u64,
    /// Direct VT convenience calls receive the Session's reconciled range.
    selection: Option<crate::selection::SelectionRange>,
    /// Direct-frame query intervals retain capacity across display turns.
    search: Vec<crate::search::SearchFill>,
    /// Assembly-order ink parallels copied glyphs and retains capacity (P1).
    selection_colors: Vec<u32>,
    /// Transient selected mask colors parallel the assembled raster stream.
    raster_selection_colors: Vec<u32>,
    /// Current color policy; settings will own it in M6.5.
    colors: colors::ColorPolicy,
    /// Drawn cursor defaults remain separate from VT's DECSCUSR request.
    cursor_settings: CursorSettings,
    /// How many rows were reshaped in the latest frame, for P4 evidence.
    rebuilt_rows: usize,
    /// Current xterm blink phase; only assembly changes on a timer tick.
    blink_on: bool,
    /// Platform focus controls hollow rather than filled block cursors.
    focused: bool,
    /// Physical surface extent supplied by a live host, when present.
    surface_size: Option<[u32; 2]>,
    /// Inset in physical pixels; harness defaults preserve reference overhang.
    padding: u32,
    /// Host-reserved left gutter in whole cells, independent of outer padding.
    gutter_columns: u8,
    /// Reused copy for callers that still supply a direct VT reference.
    snapshot: FrameSnapshot,
    /// Optional measured contract; presenters consume it only after actual submission.
    statistics: Option<crate::messages::FrameStats>,
}

impl FrameBuilder {
    /// Starts row caches from an already validated host-owned text context.
    pub fn new(context: &TextContext) -> Self {
        Self::from_metrics(
            context.metrics(),
            context.font_size(),
            context.dpr(),
            context.atlas_generation(),
        )
    }

    /// Initializes render storage from neutral metrics, without borrowing fonts.
    /// Session messages can carry these values across the platform thread seam.
    pub(crate) fn from_metrics(
        metrics: CellMetrics,
        font_size: f32,
        dpr: f32,
        generation: u64,
    ) -> Self {
        Self {
            geometry: puzzle_overlay::geometry::TextGeometry::default(),
            overlays: overlays::OverlayFrame::default(),
            rows: Vec::new(),
            visible_rows: 0,
            row_shift: 0,
            metrics,
            font_size,
            dpr,
            style_epoch: 0,
            context_generation: generation,
            screen_epoch: 0,
            selection: None,
            search: Vec::new(),
            selection_colors: Vec::new(),
            raster_selection_colors: Vec::new(),
            colors: colors::ColorPolicy::default(),
            cursor_settings: CursorSettings::default(),
            rebuilt_rows: 0,
            blink_on: true,
            focused: true,
            surface_size: None,
            padding: 16,
            gutter_columns: 0,
            snapshot: FrameSnapshot::default(),
            statistics: None,
        }
    }

    /// Sets assembly-only range state; no row cache key changes.
    pub fn set_selection(&mut self, range: Option<crate::selection::SelectionRange>) {
        self.selection = range;
    }

    /// Copies query intervals without invalidating prepared glyph rows.
    pub fn set_search(&mut self, fills: &[crate::search::SearchFill]) {
        self.search.clear();
        self.search.extend_from_slice(fills);
    }

    /// Returns integer device-cell dimensions for surface and PTY sizing.
    #[must_use]
    pub fn metrics(&self) -> CellMetrics {
        self.metrics
    }

    /// Reports cache misses from the previous assembly for damage tests.
    #[must_use]
    pub fn rebuilt_rows(&self) -> usize {
        self.rebuilt_rows
    }

    /// Exposes appearance revision for a consistent snapshot capture.
    #[must_use]
    pub fn style_epoch(&self) -> u64 {
        self.style_epoch
    }

    /// Changes only assembly visibility; cached shaping remains valid.
    pub fn set_blink_phase(&mut self, on: bool) {
        self.blink_on = on;
    }

    /// Reads the current phase before the session advances its timer.
    #[must_use]
    pub fn blink_phase(&self) -> bool {
        self.blink_on
    }

    /// Detects any cached SGR-5 ink that shares the cursor blink timer.
    #[must_use]
    pub fn has_blinking_ink(&self) -> bool {
        self.rows
            .iter()
            .take(self.visible_rows)
            .any(|row| row.glyph_blink.contains(&true))
    }

    /// Shows a hollow block while the host window is unfocused (§10.5).
    pub fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Keeps live GPU clip-space coordinates tied to actual backing pixels.
    pub fn set_surface_size(&mut self, size: [u32; 2]) {
        self.surface_size = Some(size);
    }

    /// Applies render-only theme options without rewriting VT cell colors.
    pub fn set_color_options(&mut self, bold_is_bright: bool, minimum_contrast: f32) {
        self.colors.set_options(bold_is_bright, minimum_contrast);
        // A row version does not change when theme policy changes, but its
        // instance colors must be rebuilt from the retained palette tags.
        self.style_epoch = self.style_epoch.wrapping_add(1);
    }

    /// Recolors cached rows on the next build without reshaping their text.
    pub fn set_theme(&mut self, theme: &Theme) {
        self.colors.set_theme(theme);
        // Palette indices remain in VT cells. Rebuilding instance colors
        // is enough to honor a new theme, including the surface clear.
        self.style_epoch = self.style_epoch.wrapping_add(1);
    }

    /// Recomputes style-sensitive geometry after a DPR or font-size change.
    pub fn set_context(&mut self, context: &TextContext) {
        // The host selects a validated context before a size or DPR change;
        // this session only replaces cell geometry and stale row handles.
        self.set_font_metrics(
            context.metrics(),
            context.font_size(),
            context.dpr(),
            context.atlas_generation(),
        );
    }

    /// Accepts copied measurements while font resources stay on the rendering owner.
    pub(crate) fn set_font_metrics(
        &mut self,
        metrics: CellMetrics,
        size: f32,
        dpr: f32,
        generation: u64,
    ) {
        self.metrics = metrics;
        self.font_size = size;
        self.dpr = dpr;
        self.context_generation = generation;
        // Geometry invalidates retained ink without changing any VT cell's version.
        self.style_epoch = self.style_epoch.wrapping_add(1);
    }

    /// Forces row copies before cached handles from an older atlas are used.
    pub fn sync_generation(&mut self, generation: u64) {
        if self.context_generation != generation {
            self.context_generation = generation;
            // Snapshot capture uses this epoch to copy even unchanged VT
            // rows, which both builders need after atlas eviction.
            self.style_epoch = self.style_epoch.wrapping_add(1);
        }
    }

    /// Converts currently visible VT lines into a complete ordered frame.
    pub fn build(
        &mut self,
        context: &mut TextContext,
        terminal: &Terminal,
        frame: &mut RenderFrame,
    ) -> Result<(), String> {
        self.build_viewport(
            context,
            terminal,
            terminal.screen().ring.scrollback_len(),
            frame,
        )
    }

    /// Builds a document slice selected by the Session viewport.
    pub fn build_viewport(
        &mut self,
        context: &mut TextContext,
        terminal: &Terminal,
        top: usize,
        frame: &mut RenderFrame,
    ) -> Result<(), String> {
        self.build_scrolled(context, terminal, top, 0, frame)
    }

    /// Builds both clipped edge rows at an integral device-pixel displacement.
    pub fn build_scrolled(
        &mut self,
        context: &mut TextContext,
        terminal: &Terminal,
        top: usize,
        shift: i32,
        frame: &mut RenderFrame,
    ) -> Result<(), String> {
        context.begin_frame()?;
        self.sync_generation(context.atlas_generation());
        // Harness and tests keep this convenience path. The desktop session
        // thread publishes an owned snapshot slot; the render thread calls
        // this builder with that sealed publication (D-50).
        let mut snapshot = std::mem::take(&mut self.snapshot);
        snapshot.capture_scrolled(terminal, self.style_epoch, top, shift)?;
        snapshot.set_selection(self.selection);
        snapshot.set_search(&self.search);
        let result = self.build_snapshot(context, &snapshot, frame);
        if result.is_err() {
            snapshot.invalidate();
        }
        self.snapshot = snapshot;
        result
    }

    /// Keeps cache row allocations after a viewport size change.
    fn ensure_rows(&mut self, count: usize) {
        self.rows
            .resize_with(count.saturating_mul(2), CachedRow::default);
    }
}

#[cfg(test)]
/// Frame assembly and row-cache behavior through real font outlines.
mod tests;

/// Optional coarse construction counters keep ordinary frames free of clocks.
mod statistics;

/// Test-only work observation, with no counters or branches in shipped code.
#[cfg(test)]
#[path = "frame_builder/tests/work.rs"]
pub(crate) mod testing_work;
