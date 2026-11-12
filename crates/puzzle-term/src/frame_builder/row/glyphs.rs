//! Converts row placements into ordinary or consecutive COLR Slug instances.
//!
//! Every color layer uses the whole glyph's fit; individually fitting each
//! layer would move eyes, outlines and gradients relative to one another.
//!
//! Layout owns cells and advances. This stage changes only ink transforms:
//! a foreign face or an emoji graph may have different bounds, but the next
//! ASCII glyph still starts at the column chosen by D-31 placement.
//! Each solid palette layer supplies its instance RGBA. A foreground layer
//! inherits the current text RGB with its paint alpha; gradient stop flags
//! preserve that same distinction inside the shader's color-line evaluation.
//! The row keeps a parallel palette mask for cursor redraw and resolved
//! selection colors for each instance. Neither policy may tint fixed CPAL
//! colors. Ordinary text and procedural paths extend the mask with false.
//! Cached COLR slices are borrowed: a warm rebuild must not clone a layer Vec.
//! Blinking is applied after layer expansion, so every fill of one cluster
//! disappears together rather than leaving part of a multicolor emoji.
//! Neutral source requests are retained before layer expansion for grid effects.
//! Only pixel-only preparation sees an effective raster-size multiplier.
use super::{CachedRow, FontSet, RunStyle, SlugBackend};
use puzzle_text::glyph::GlyphPlacement;
/// Prepares ink without rewriting a grid origin, advance or cluster span.
#[allow(
    clippy::too_many_arguments,
    reason = "the row crosses independent vector and bitmap cache owners"
)]
pub(in crate::frame_builder) fn append_glyphs(
    fonts: &mut FontSet,
    backend: &mut SlugBackend,
    raster: &mut puzzle_text::raster::RasterAtlas,
    cache: &mut CachedRow,
    glyphs: &[GlyphPlacement],
    style: RunStyle,
    transformed: bool,
    dpr: f32,
    raster_scale: f32,
) -> Result<(), String> {
    // Count neutral requests before backend lookup rather than only visible
    // instances: a repeated lookup that emits nothing is still repeated work.
    #[cfg(test)]
    crate::frame_builder::testing_work::charge(2, glyphs.len());
    // Reserved source faces carry retained cluster identities rather than OpenType IDs.
    for placement in glyphs {
        // Retain the neutral request, never reverse-engineer cells from backend ink.
        cache
            .sources
            .push(crate::frame_builder::overlays::effects::Source {
                placement: *placement,
                style,
                italic: transformed,
                empty: false,
            });
        let first = cache.glyphs.len();
        let first_raster = cache.rasters.len();
        if matches!(
            placement.key.face,
            puzzle_text::cluster_source::EMOJI_FACE | puzzle_text::cluster_source::PLATFORM_FACE
        ) {
            // Platform ink owns its store choice and appearance-array expansion.
            // A reserved face must never be interpreted as an index into bundled fonts.
            source::append(
                fonts,
                backend,
                raster,
                cache,
                placement,
                style,
                transformed,
                dpr,
                raster_scale,
            )?;
            geometry::retain(cache, placement, first, first_raster);
            continue;
        }
        // Shared face dispatch keeps source discovery separate from loaded font tables.
        ordinary::glyph(
            fonts,
            backend,
            raster,
            cache,
            placement,
            style,
            transformed,
            dpr,
        )?;
        geometry::retain(cache, placement, first, first_raster);
        // Paint-table palette entries ignore recoloring, while foreground
        // entries use the instance color, including selection/cursor tint.
        cache.glyph_palette.resize(cache.glyphs.len(), false);
        cache.glyph_blink.resize(cache.glyphs.len(), style.blink);
        cache
            .glyph_selection_ink
            .resize(cache.glyphs.len(), style.selection_ink);
    }
    Ok(())
}

/// Expands an ordered COLR graph while fitting all its layers as one glyph.
pub(super) fn append_color(
    cache: &mut CachedRow,
    placement: &GlyphPlacement,
    color: &puzzle_text::glyph::slug::PreparedColor,
    style: RunStyle,
    transformed: bool,
) {
    // The neutral placement remains immutable; only this ink view gets emoji fitting.
    let mut fitted = *placement;
    // A color font always uses its vector paint graph, regardless of DPR.
    fitted.color_glyph = true;
    for layer in &color.layers {
        // Every layer shares the graph union; eyes and outlines must scale together.
        let mut union = layer.shape;
        union.bbox = color.bounds;
        let mut draw = crate::glyph_ink::fitted_instance(&fitted, union, transformed, true);
        // Union bounds choose the fit only; the quad/bands use this layer.
        draw.bbox = layer.shape.bbox;
        // Zero denotes a fast solid; nonzero indexes the retained gradient/clip row.
        draw.paint = layer.paint.index;
        // Fixed CPAL solids preserve authored RGB while sentinel fills inherit foreground.
        if let Some(color) = layer.solid_color {
            draw.color = color;
        } else {
            // Foreground brush opacity multiplies text alpha without tinting palette layers.
            let alpha = ((draw.color >> 24) as f32 * layer.opacity).round() as u32;
            draw.color = (draw.color & 0x00ffffff) | (alpha << 24);
        }
        // The parallel flag also governs isolated cursor redraws from these instances.
        cache.glyph_palette.push(layer.solid_color.is_some());
        // Palette solids ignore selection; gradient foreground stops
        // still see the selection color through the instance tint.
        cache
            .glyph_selection_ink
            .push(if layer.solid_color.is_some() {
                draw.color
            } else {
                style.selection_ink
            });
        cache.glyphs.push(draw);
    }
}

/// Native source outlines and COLR layers share Slug; browser samples use raster.
mod source;

/// Backend-neutral metadata and union ink boxes for TextGeometry.
pub(in crate::frame_builder) mod geometry;

/// Ordinary face ink also uses device-sized oracle masks on vector overflow.
mod ordinary;
/// Shared diagnostic components preserve labels while bounding atlas growth.
mod tofu;
