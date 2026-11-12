//! Placement of device-cell glyphs and missing-codepoint boxes (§8.6).
//!
//! A box line must reach precisely the device-pixel edge of its source
//! cell. It therefore uses a procedural path sized from integral metrics,
//! and a bottom-left origin rather than the font baseline. This occurs in
//! the neutral layout layer; the same Slug backend prepares the path later.
//!
//! The key contains width, height and stroke thickness because those
//! physical measurements determine the outline itself. An ordinary font
//! glyph key can reuse one em-space outline across sizes, but a builtin
//! box line must be regenerated when the cell becomes wider or the DPR
//! changes. Keeping that distinction in the key prevents stale geometry.
//!
//! Shades deliberately use alpha rather than a checkerboard of tiny
//! contours. That keeps the three shade levels stable under zoom and
//! avoids a moiré pattern on displays with fractional pixel ratios.
use super::{Cluster, RunContext};
use crate::builtin::TOFU_FACE;
use crate::{
    builtin::BuiltinCell,
    font_set::is_default_ignorable,
    glyph::{GlyphKey, GlyphPlacement, HintMode},
};
use unicode_width::UnicodeWidthChar;

/// Places a builtin or missing-codepoint outline on exact cell edges.
pub(super) fn placement(
    cluster: &Cluster,
    text: &str,
    context: &RunContext<'_>,
) -> Result<GlyphPlacement, String> {
    let metrics = context.metrics;
    let options = context.options;
    let origin = procedural_origin(cluster, context)?;
    let grapheme = &text[cluster.byte..cluster.end];
    let ch = grapheme.chars().next().ok_or("empty builtin cluster")?;
    // A missing pictograph may become two cells only because VS16 joined
    // it. Keep its original one-cell tofu outline and leave the second
    // cell blank; VS16 changes placement, never draws its own missing box.
    let base_span = UnicodeWidthChar::width(ch).unwrap_or(1).max(1) as u32;
    let ignorable_tail = grapheme.chars().skip(1).all(is_default_ignorable);
    let draw_span = if cluster.face == TOFU_FACE && ignorable_tail {
        base_span.min(cluster.span.max(1))
    } else {
        cluster.span.max(1)
    };
    let draw_width = metrics
        .width_px
        .checked_mul(draw_span)
        .ok_or("builtin width overflow")?;
    let advance = metrics
        .width_px
        .checked_mul(cluster.span.max(1))
        .ok_or("builtin advance overflow")?;
    // Packing the snapped measurements into the key distinguishes this
    // glyph from the same Unicode scalar at another DPR or font size.
    let cell = BuiltinCell::new(draw_width, metrics.height_px, metrics.underline_px)?;
    // Shades use a solid path with 25/50/75% opacity. The remaining
    // procedural shapes keep the caller's original text alpha.
    let shade = match ch {
        '░' => 64,
        '▒' => 128,
        '▓' => 191,
        _ => 255_u32,
    };
    let alpha = ((options.color & 0xff) * shade + 127) / 255;
    // Position and advance still belong to layout, while the source
    // supplies only the path when the glyph backend later asks for it.
    Ok(GlyphPlacement {
        cell_column: cluster.cell + (options.origin_px[0].max(0) as u32 / metrics.width_px.max(1)),
        key: GlyphKey {
            face: cluster.face,
            glyph_id: ch as u32,
            instance: cell.pack()?,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        origin_px: origin,
        baseline_px: metrics.baseline_px as f32,
        size_px: f32::from(cell.height),
        // Grid advance still uses the full Unicode cluster span. The
        // outline can be narrower when VS16 added only an empty spacer.
        advance_px: advance,
        ink_limit_px: draw_width,
        cell_height_px: metrics.height_px,
        cell_span: u8::try_from(cluster.span).map_err(|_| "builtin spans too many cells")?,
        color: (options.color & !0xff) | alpha,
        // Effects still select the vector backend at the D-28 seam.
        effect: options.transformed,
        color_glyph: false,
    })
}

/// Places a cell-filling procedural path at its bottom-left device edge.
fn procedural_origin(cluster: &Cluster, context: &RunContext<'_>) -> Result<[i32; 2], String> {
    let cell_x = cluster
        .cell
        .checked_mul(context.metrics.width_px)
        .ok_or("builtin x overflow")?;
    let x = context.options.origin_px[0]
        .checked_add(i32::try_from(cell_x).map_err(|_| "builtin x exceeds device range")?)
        .ok_or("builtin x overflow")?;
    // The Slug path's y-up em origin is at the bottom of the source cell;
    // font glyphs instead use their metric baseline for this translation.
    let height = i32::try_from(context.metrics.height_px).map_err(|_| "builtin row too tall")?;
    let y = context.options.origin_px[1]
        .checked_add(height)
        .ok_or("builtin baseline overflow")?;
    Ok([x, y])
}
