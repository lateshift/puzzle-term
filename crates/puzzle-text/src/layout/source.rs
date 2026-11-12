//! Cell placement for complete passed-in cluster sources (D-28, D-31).
//! Sources receive device size and immutable baseline/advance geometry.
use super::{Cluster, RunContext, grid_geometry};
use crate::{font_set::FontSet, glyph::GlyphPlacement};
/// Anchors a passed-in source to the same cell geometry used by Harfrust.
pub(super) fn source_placement(
    fonts: &mut FontSet,
    source: &Cluster,
    text: &str,
    context: &RunContext<'_>,
) -> Result<GlyphPlacement, String> {
    // Reuse the exact baseline and checked column arithmetic of shaped glyphs.
    // Zero offsets represent a whole-cluster source rather than a combining
    // component; a platform rasterizer can position ink inside this rectangle.
    let (origin, advance, ink_limit) = grid_geometry(
        source,
        crate::shape::ShapedGlyph {
            glyph_id: 0,
            cluster: 0,
            advance_em: None,
            x_offset_em: 0.0,
            y_offset_em: 0.0,
        },
        context,
    )?;
    // No shaping advance enters this record. The source receives its exact
    // requested device size only when the chosen backend prepares the ink.
    // The key owns selectors via FontSet's request registry, while the record
    // keeps only small numeric fields suitable for reusable row scratch.
    // Color-glyph policy is a source preference here; drawn pixels later
    // determine whether a bitmap is actually color or monochrome coverage.
    // Ink limits and height bound fallback fonts without changing advance.
    // Effects remain visible to the downstream vector/bitmap selector.
    let mut key = fonts.source_key(
        &text[source.byte..source.end],
        context.options.style,
        source.face,
    )?;
    // System outlines obey D-27 too. Bitmap caches already include device size,
    // and a hinted suffix prevents one prepared outline serving another ppem.
    // The suffix describes outline extraction; the registry still addresses UTF-8 by glyph ID.
    key.hint_ppem = context.hint.ppem;
    key.hint_mode = context.hint.mode;
    Ok(GlyphPlacement {
        // Source cells remain fixed even if the platform adjusts the ink baseline.
        cell_column: source.cell
            + (context.options.origin_px[0].max(0) as u32 / context.metrics.width_px.max(1)),
        key,
        origin_px: origin,
        baseline_px: context.metrics.baseline_px as f32,
        size_px: context.metrics.device_size,
        advance_px: advance,
        ink_limit_px: ink_limit,
        // Row height bounds ink from system fonts with different ascenders.
        cell_height_px: context.metrics.height_px,
        // This span came from D-31 before any source was consulted.
        cell_span: source.span as u8,
        // Coverage masks inherit SGR ink; color pixels retain their palette.
        color: context.options.color,
        effect: context.options.transformed,
        color_glyph: source.face == crate::cluster_source::EMOJI_FACE,
    })
}

/// Selects the two non-shaping branches while ordinary fonts retain Harfrust.
pub(super) fn special(
    fonts: &mut FontSet,
    cluster: &Cluster,
    text: &str,
    context: &RunContext<'_>,
) -> Result<Option<GlyphPlacement>, String> {
    use crate::{
        builtin::TOFU_FACE,
        cluster_source::{EMOJI_FACE, PLATFORM_FACE},
    };
    // Reserved source IDs are below procedural IDs, outside loaded font slots.
    if matches!(cluster.face, EMOJI_FACE | PLATFORM_FACE) {
        return source_placement(fonts, cluster, text, context).map(Some);
    }
    // Tofu and joining builtin shapes use device-cell geometry, not em metrics.
    if cluster.face >= TOFU_FACE {
        return super::procedural::placement(cluster, text, context).map(Some);
    }
    Ok(None)
}
