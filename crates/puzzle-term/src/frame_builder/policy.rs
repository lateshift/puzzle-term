//! Copy-only drawing policy sealed with a published Session snapshot (D-50).
//!
//! Preferences contain neither row caches nor resources. A renderer applies the
//! published value before shaping, so later Session input cannot change the ink
//! of an older frame. Local cache epochs remain monotonic across publications;
//! adopting an older policy may invalidate ink, never roll the epoch backward.
//! Font and palette changes invalidate colored geometry; focus and blink only
//! affect assembly. No layout or model callback runs while copying preferences.
use super::{FrameBuilder, colors::ColorPolicy};
use crate::session::cursor_settings::CursorSettings;
use puzzle_text::metrics::CellMetrics;

/// Immutable frame preferences, separate from rebuildable drawing caches.
#[derive(Clone)]
pub(crate) struct FramePolicy {
    /// Source policy revision is independent of the renderer's cache history.
    pub(crate) revision: u64,
    /// Integer device measurements sampled by the Session owner.
    pub(crate) metrics: CellMetrics,
    /// Logical em size selected by the host's font settings.
    pub(crate) font_size: f32,
    /// Device-to-logical scale shared by text and overlay placement.
    pub(crate) dpr: f32,
    /// Resolved colors carry no reference to a mutable theme registry.
    colors: ColorPolicy,
    /// Host caret preferences independent of VT's requested style.
    cursor: CursorSettings,
    /// Timed ink visibility is assembly state, not a row content revision.
    pub(crate) blink: bool,
    /// Unfocused caret outlines use this captured host state.
    pub(crate) focused: bool,
    /// A drag may change the backing before the coalesced grid resize.
    pub(crate) surface: Option<[u32; 2]>,
    /// Physical inset used by every grid layer and hit region.
    pub(crate) padding: u32,
    /// Host-reserved left gutter in whole cells, independent of outer padding.
    pub(crate) gutter_columns: u8,
}

/// Publication reads small values; the rendering owner keeps all cache storage.
impl FrameBuilder {
    /// Installs a sealed policy while preserving the renderer's own cache history.
    pub(crate) fn apply_published_policy(&mut self, policy: &FramePolicy) {
        // Only preferences that change retained row ink invalidate the cache epoch.
        if self.metrics != policy.metrics
            || self.font_size != policy.font_size
            || self.dpr != policy.dpr
            || self.colors != policy.colors
            || self.padding != policy.padding
            || self.gutter_columns != policy.gutter_columns
        {
            self.style_epoch = self.style_epoch.wrapping_add(1);
        }
        self.metrics = policy.metrics;
        self.font_size = policy.font_size;
        self.dpr = policy.dpr;
        self.colors.clone_from(&policy.colors);
        // Cursor and timer state are sampled anew even when retained row ink is valid.
        self.cursor_settings = policy.cursor;
        self.blink_on = policy.blink;
        self.focused = policy.focused;
        self.surface_size = policy.surface;
        self.padding = policy.padding;
        // UI cells belong to this surface, never to a shared font context.
        self.gutter_columns = policy.gutter_columns;
    }
}

/// Session-owned preferences mutate without a builder or glyph resource.
impl FramePolicy {
    /// Initializes the same baseline used by direct VT renderers.
    pub(crate) fn new(font: crate::FontMetrics, cursor: CursorSettings) -> Self {
        Self {
            revision: 0,
            metrics: font.cell,
            font_size: font.font_size,
            dpr: font.dpr,
            colors: ColorPolicy::default(),
            cursor,
            blink: true,
            focused: true,
            surface: None,
            padding: 16,
            gutter_columns: 0,
        }
    }
    /// Font loading supplies only measurements, never shaping or cache state.
    pub(crate) fn set_font_metrics(&mut self, font: crate::FontMetrics) {
        self.metrics = font.cell;
        self.font_size = font.font_size;
        self.dpr = font.dpr;
        // Source copy history changes independently of any render-context generation.
        self.revision = self.revision.wrapping_add(1);
    }
    /// A resolved theme recolors palette tags without rewriting VT cells.
    pub(crate) fn set_theme(&mut self, theme: &crate::theme::Theme) {
        self.colors.set_theme(theme);
        self.revision = self.revision.wrapping_add(1);
    }
    /// Contrast and bold-bright are source policy, separate from terminal SGR.
    pub(crate) fn set_color_options(&mut self, bright: bool, contrast: f32) {
        self.colors.set_options(bright, contrast);
        self.revision = self.revision.wrapping_add(1);
    }
    /// Caret settings do not require a borrowed drawing owner.
    pub(crate) fn set_cursor_settings(&mut self, cursor: CursorSettings) {
        if self.cursor != cursor {
            self.cursor = cursor;
            self.revision = self.revision.wrapping_add(1);
        }
    }
    /// Pointer models derive their track from the same sealed grid geometry as drawing.
    pub(crate) fn scrollbar_track(&self, columns: usize, rows: usize) -> [f64; 4] {
        let mut track = super::padding::scrollbar_track(self.metrics, self.padding, columns, rows);
        track[0] += f64::from(self.gutter_columns) * f64::from(self.metrics.width_px);
        track
    }
}
