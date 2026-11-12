//! Startup OSC 66 text uses the same imported outlines as ordinary pack text.
//!
//! The pack has design advances, not shaping tables. Its supported scalars
//! have first-frame ink; full-font takeover invalidates these row caches and
//! enables complete grapheme shaping. No separate drawing pipeline is used.
//!
//! Natural positions are temporary: the caller applies one uniform OSC 66 map
//! after the whole run is emitted. Scaling each scalar independently would fit
//! an explicit-width run repeatedly and make its advances inconsistent.
//! The directory's design advance retains spaces even without an ink handle.
//! Styled packs already contain their weight and slant, so this path does not
//! add synthetic bold or italic on top. A missing styled scalar can use regular.
//! Each request also retains its natural key for later overlay effect rebuilds.
//! Source maps and cursor copies are attached by the caller after fitting.
//! Imported handles are looked up again rather than trusting directory addresses:
//! an atlas reincarnation can invalidate a formerly valid packed address.
//! Full-font takeover rebuilds this cache and supplies proper shaping/fallback.
//! This temporary startup path never changes VT occupancy or stored text.
use super::{RunStyle, TextContext};
use crate::frame_builder::{CachedRow, overlays::effects::Source};
use puzzle_text::{
    glyph::{GlyphKey, GlyphPlacement, HintMode},
    metrics::CellMetrics,
};

/// Emits natural pack ink and returns its full design advance in device pixels.
pub(super) fn append(
    context: &mut TextContext,
    cache: &mut CachedRow,
    text: &str,
    style: RunStyle,
    metrics: CellMetrics,
) -> Result<f32, String> {
    // Accumulate in floating point; round only the origins written into placements.
    let mut width = 0.0;
    for scalar in text.chars() {
        // Variation selectors and joiners have no standalone startup advance.
        if puzzle_text::font_set::is_default_ignorable(scalar) {
            continue;
        }
        // Pack face slots follow the same four style classes as full-font sources.
        let face = style.face as u32;
        let (face, glyph) = match context.backend.packed_glyph(face, scalar) {
            Some(glyph) => (face, Some(glyph)),
            None => (0, context.backend.packed_glyph(0, scalar)),
        };
        let Some(glyph) = glyph else {
            // A missing startup scalar follows the existing pack takeover policy.
            width += metrics.width_px as f32;
            continue;
        };
        // Design units are normalized to em by the pack loader.
        let advance = glyph.advance_em * metrics.device_size;
        // Contourless space entries contribute advance but must never enter Slug.
        if glyph.handle.is_some() {
            // Save the natural request before any integer/fractional sizing is applied.
            let placement = placement(metrics, face, glyph.glyph_id, width, advance, style);
            let handle = context
                .backend
                .get(placement.key)
                .ok_or("sized startup pack was evicted")?;
            // Geometry retention must see only the instances emitted by this scalar.
            let first = cache.glyphs.len();
            crate::glyph_ink::append_ink(
                &mut cache.glyphs,
                &placement,
                handle,
                false,
                false,
                false,
            );
            // An effect rebuild needs the original key, not an already scaled quad.
            cache.sources.push(Source {
                placement,
                style,
                italic: false,
                empty: false,
            });
            // Shared geometry extraction reads the same imported shape bounds.
            super::super::row::glyphs::geometry::retain(
                cache,
                &placement,
                first,
                cache.rasters.len(),
            );
            // Pack ink is monochrome and follows selection/cursor foreground tint.
            cache.glyph_palette.resize(cache.glyphs.len(), false);
            // Every emitted layer keeps its block-level SGR blink state.
            cache.glyph_blink.resize(cache.glyphs.len(), style.blink);
            cache
                .glyph_selection_ink
                .resize(cache.glyphs.len(), style.selection_ink);
        }
        // Whitespace advances even though its contour is deliberately absent.
        width += advance;
    }
    Ok(width)
}

/// A natural source has no physical cell until its whole-block map is applied.
fn placement(
    metrics: CellMetrics,
    face: u32,
    glyph_id: u32,
    pen: f32,
    advance: f32,
    style: RunStyle,
) -> GlyphPlacement {
    // This is a neutral natural line; the caller replaces its cell hit extent.
    GlyphPlacement {
        // OSC 66 transforms must select unhinted outlines, even at DPR one.
        key: GlyphKey {
            face,
            glyph_id,
            instance: face,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        // Row-relative natural baseline is mapped with the rest of the block.
        origin_px: [pen.round() as i32, metrics.baseline_px],
        baseline_px: metrics.baseline_px as f32,
        size_px: metrics.device_size,
        // The common emitter uses unsigned pixel bounds for its neutral source.
        advance_px: advance.ceil() as u32,
        ink_limit_px: advance.ceil() as u32,
        cell_height_px: metrics.height_px,
        // Source coordinates are temporary; block occupancy is recorded separately.
        cell_column: 0,
        cell_span: 1,
        color: super::super::colors::rgba(style.ink),
        // This prevents the untransformed grid bitmap selector from taking over.
        effect: true,
        color_glyph: false,
    }
}
