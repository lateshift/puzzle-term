//! Explicit probes use primary cell geometry without extracting coverage ink.
//! Live sources receive actual placements and validate on the background worker.
use super::NativeFonts;
use puzzle_term::text::glyph::{GlyphKey, GlyphPlacement, HintMode};

/// Probe geometry does not establish a coverage fact or own prepared ink.
impl NativeFonts {
    /// A neutral two-cell diagnostic placement never changes actual D-31 layout.
    pub fn probe_placement(&self) -> GlyphPlacement {
        // Cold witnesses inspect the returned ink directly, with no boolean prepass.
        // Production requests carry the actual draw placement supplied by layout.
        // Two cells are enough for any cluster accepted by D-31; no advance is inferred.
        let m = self.metrics;
        // The probe's source column is fixed independently of fitted baseline ink.
        GlyphPlacement {
            // Coverage probing has no grid offset; the source cell begins at zero.
            cell_column: 0, // This validation key is local and never inserted into a shared glyph atlas.
            key: GlyphKey {
                face: 0,
                glyph_id: 0,
                instance: 0,
                hint_ppem: 0,
                hint_mode: HintMode::None,
            },
            // A zero-x primary baseline makes bitmap fitting independent of row position.
            origin_px: [0, m.baseline_px],
            baseline_px: m.baseline_px as f32,
            size_px: m.device_size,
            advance_px: 2 * m.width_px,
            ink_limit_px: 2 * m.width_px,
            cell_height_px: m.height_px,
            cell_span: 2,
            // White exposes authored color without depending on the current theme.
            color: 0xffffffff,
            effect: false,
            color_glyph: self.emoji_only,
        }
    }
}
