//! Re-prepares affected neutral placements and applies ink-only maps and color.
//!
//! Every layer of a cluster receives the same cell-origin transform. Color
//! gradients retain their authored stops unless an explicit recolor is given.
//! Effect compilation retains original placement keys until a range selects them.
//! Hinted and unhinted variants never alias because hint mode and ppem are keyed.
//! A color-only provider effect also selects unhinted ink under the backend contract.
//! Source clusters stay complete; combining marks keep their natural baseline offset.
//! The fallback fitting stage precedes the affine effect and remains part of the map.
//! Authored palette solids preserve their RGB when no explicit recolor is requested.
//! The common paint mirror resolves gradient foreground references for color variants.
//! No modified color row relocates source addresses already captured in this frame.
//! Opacity applies to every layer of a color glyph rather than only its outer shape.
//! A transparent hidden effect contributes neither an outline nor a bitmap draw.
//! Selection and cursor geometry continue to cover ordinary terminal cell rectangles.
//! Their copied ink streams can use transformed source glyphs without moving cells.
//! Error handling leaves the ordinary cache intact for the next provider build.
//! Warm variants reuse all retained instance arrays and source ownership.
//!
//! OSC 66 sources retain their natural placements beside one whole-block map.
//! Rebuilding from already scaled instances would apply sizing twice after
//! an animation or colour-only change. Instead, select a block from any of its
//! retained fragment identities and compose the provider map after sizing.
//! The variant key includes the sealed block signature, so scrolling a clipped
//! root into view cannot reuse the preceding owner row or baseline offset.
//! Ordinary rows have no block maps; their source selection stays cell-local.
//! Startup-pack and full-font sources share this same composition order.
use super::{CachedRow, Request, Source, TextContext};
use crate::frame_builder::row::glyphs::append_glyphs;
use kurbo::Affine;
use puzzle_overlay::{draw::Effect, geometry::Range};
use puzzle_text::glyph::HintMode;
use puzzle_vt::tracked::LineId;
/// Tests an inclusive source interval without looking up any off-screen row.
/// Endpoints are inclusive and either orientation names the same interval.
/// A wide source or ligature is one ink request even if only one cell is selected.
/// The provider's visible snapshot supplies cells; this check performs no buffer read.
/// Returns whether the source span intersects the retained descriptor.
pub(super) fn touches(range: Range, line: LineId, column: usize, span: usize) -> bool {
    let start = (range.start.line.0, range.start.column);
    let end = (range.end.line.0, range.end.column);
    let (lo, hi) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    // Ligatures and wide clusters are indivisible ink, but their cells stay neutral.
    (line.0, column) <= hi && (line.0, column.saturating_add(span.saturating_sub(1))) >= lo
}
/// Builds a cold variant while keeping paper, decoration and neutral metadata intact.
/// `source` holds ordinary paper, underline geometry and original source requests.
/// `requests` are already sampled and ordered by provider submission.
/// Every overlapping transform multiplies the preceding map on the left.
/// Opacity multiplies across providers and the last explicit color wins.
/// Hidden ink is omitted without changing the original cell text or advance.
/// Returns a local preparation error before a variant is published in the cache.
pub(super) fn rebuild(
    context: &mut TextContext,
    source: &CachedRow,
    out: &mut CachedRow,
    requests: &[Request],
    dpr: f32,
    cell_width: u32,
) -> Result<(), String> {
    // Retain all array capacities and paper data rather than replacing the row owner.
    out.clone_from(source);
    // Ordinary caches are untouched; only this alternate ink stream is replaced.
    clear_ink(out);
    let line = source.id.ok_or("effect row identity absent")?;
    for (index, request) in source.sources.iter().enumerate() {
        // Ordinary rows bypass sparse sizing lookup at the row boundary.
        let sizing = if source.block_transforms.is_empty() {
            None
        } else {
            source
                .block_transforms
                .iter()
                .find(|b| b.sources.contains(&index))
        };
        let [column, span] = sizing.map_or(
            [
                request.placement.cell_column as usize,
                request.placement.cell_span as usize,
            ],
            |b| b.cells,
        );
        // Start each cluster independently; effects cannot leak into the following cells.
        let mut effect = Effect::default();
        let mut active = false;
        for item in requests {
            if sizing.map_or_else(
                || touches(item.range, line, column, span),
                |block| {
                    block
                        .lines
                        .iter()
                        .flatten()
                        .any(|line| touches(item.range, *line, column, span))
                },
            ) {
                // Later transforms multiply earlier maps; alpha multipliers compose.
                effect.transform = item.effect.transform * effect.transform;
                effect.color = item.effect.color.or(effect.color);
                effect.opacity *= item.effect.opacity.clamp(0.0, 1.0);
                effect.hidden |= item.effect.hidden;
                active = true;
            }
        }
        // Omitting ink does not erase the cell or change the source used by selection.
        if effect.hidden {
            continue;
        }
        emit(
            context,
            out,
            request,
            sizing,
            active.then_some(effect),
            dpr,
            cell_width,
            source.cursor_column,
        )?;
    }
    Ok(())
}
/// Emits one original request with an unhinted key only when an effect selects it.
/// A copied placement gets an unhinted source key only when a provider selects it.
/// The original request remains available for geometry and effect removal.
/// Pixel-only preparation receives the strongest device magnification of its map.
/// `first` indices isolate every newly emitted color layer and bitmap crop.
/// The cursor copy inherits that same prepared ink without moving the caret cells.
/// Unaffected sources retain ordinary hinting and font fitting behavior.
#[allow(
    clippy::too_many_arguments,
    reason = "one source uses optional sparse sizing beside existing effect owners"
)]
fn emit(
    context: &mut TextContext,
    out: &mut CachedRow,
    source: &Source,
    sizing: Option<&crate::frame_builder::multicell::SourceMap>,
    effect: Option<Effect>,
    dpr: f32,
    cell_width: u32,
    cursor: Option<usize>,
) -> Result<(), String> {
    // Only the temporary request changes backend; neutral geometry keeps this source.
    let mut p = source.placement;
    if effect.is_some() {
        // Effects must never transform a ppem-hinted grid outline, including color-only effects.
        p.key.hint_ppem = 0;
        p.key.hint_mode = HintMode::None;
        p.effect = true;
    }
    // Effective magnification is needed before source dispatch can choose a bitmap strike.
    let column = sizing.map_or(p.cell_column as usize, |b| b.cells[0]);
    let x = column as f64 * f64::from(cell_width);
    // Provider maps act on already sized grid ink, around its physical cell origin.
    let provider = effect.map_or(Affine::IDENTITY, |e| {
        Affine::translate((x, 0.0))
            * super::super::ink::device_map(e.transform, [0.0; 2], dpr)
            * Affine::translate((-x, 0.0))
    });
    let map = provider * sizing.map_or(Affine::IDENTITY, |b| b.map);
    let scale = super::super::ink::raster_scale(map, p.size_px)?;
    // Saved boundaries limit transforms to this cluster, including its COLR layers.
    let first = out.glyphs.len();
    let first_raster = out.rasters.len();
    // The common emitter applies fitting before the provider map modifies ink.
    prepare_source(context, out, source, &p, dpr, scale)?;
    if effect.is_some() || sizing.is_some() {
        transform(
            context,
            out,
            first,
            first_raster,
            map,
            effect.unwrap_or_default(),
            scale,
        )?;
    }
    if cursor == Some(column) {
        // Isolation comes from ordinary shaping; the caret still covers its neutral cells.
        out.cursor_glyphs.extend_from_slice(&out.glyphs[first..]);
        out.cursor_palette
            .extend_from_slice(&out.glyph_palette[first..]);
        out.cursor_rasters
            .extend_from_slice(&out.rasters[first_raster..]);
    }
    Ok(())
}
/// Applies one cell-relative affine to every prepared layer and size-appropriate bitmap.
#[allow(
    clippy::too_many_arguments,
    reason = "effect application borrows distinct ink coordinate and cache owners"
)]
/// The pivot is the source cell's row-relative top-left in physical pixels.
/// Offsets in the effect remain logical until the DPR-conjugated map is formed.
/// Each color layer retains its union-based native fit under that common map.
/// Gradient variants alter colors and opacity while retaining clip geometry.
/// Selected-mask metadata stores a paint index, preserving the forty-byte ABI.
/// A source bitmap has already been rasterized at `scale` times its neutral size.
fn transform(
    context: &mut TextContext,
    out: &mut CachedRow,
    first: usize,
    first_raster: usize,
    map: Affine,
    effect: Effect,
    scale: f32,
) -> Result<(), String> {
    // Parallel color metadata follows each layer rather than the cluster as a whole.
    for index in first..out.glyphs.len() {
        let draw = &mut out.glyphs[index];
        super::super::ink::vector(draw, map);
        if draw.paint != 0 && (effect.color.is_some() || effect.opacity != 1.0) {
            // Resolve stops in a shared variant row; the Slug shader math stays unchanged.
            draw.paint = context.backend.prepare_paint_effect(
                draw.paint,
                draw.color,
                effect.color,
                effect.opacity,
            )?;
        }
        // Solid layers carry alpha in their instance instead of a gradient row.
        draw.color = tint(draw.color, effect);
        out.glyph_selection_ink[index] = tint(out.glyph_selection_ink[index], effect);
        // An explicit provider color takes precedence over selection/cursor tint.
        out.glyph_palette[index] |= effect.color.is_some();
    }
    // Pixel-only ink uses the same affine pivot and independent selected tint.
    for index in first_raster..out.rasters.len() {
        let draw = &mut out.rasters[index];
        let selected = tint(out.raster_selection_ink[index], effect);
        let color = tint(draw.tint, effect);
        super::super::ink::bitmap(
            &mut context.backend,
            draw,
            map,
            scale,
            color,
            effect.color.is_some(),
        )?;
        // Selection copies store a paint index, never reinterpret that index as RGBA.
        // Translation already lives in the quad origin; the paint stores its linear map.
        let c = map.as_coeffs();
        out.raster_selection_ink[index] = context.backend.prepare_raster_effect(
            [c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32],
            selected,
        )?;
    }
    Ok(())
}
/// Packed straight-alpha override preserves authored layer alpha before opacity.
/// The input and override use straight-alpha RGBA8 words at the frame boundary.
/// An authored layer's alpha is multiplied rather than replaced by the override.
/// Alpha rounding occurs once after the complete opacity product is known.
/// Without recoloring, RGB stays byte-identical to the original layer.
/// The raster shader premultiplies this tint independently of filtered samples.
fn tint(original: u32, effect: Effect) -> u32 {
    // Packed little-endian RGB occupies the low bytes, independent of alpha.
    let replacement = effect.color.unwrap_or(original);
    let alpha = (original >> 24) as f32 * effect.opacity;
    // A replacement alpha is an additional multiplier, never an authored-alpha replacement.
    let alpha = if effect.color.is_some() {
        alpha * (replacement >> 24) as f32 / 255.0
    } else {
        alpha
    };
    (replacement & 0x00ff_ffff) | ((alpha.round() as u32) << 24)
}

/// Discards only variant ink and parallel appearance streams before rebuilding.
/// Ordinary background and underline arrays survive untouched in the clone.
/// Glyph appearance arrays must be cleared alongside the corresponding ink.
/// Source requests and geometry metadata are rebuilt only for the alternate row.
/// Isolated cursor streams are repopulated from their newly emitted cluster.
fn clear_ink(out: &mut CachedRow) {
    out.glyphs.clear();
    out.rasters.clear();
    out.sources.clear();
    out.placements.clear();
    out.glyph_blink.clear();
    out.glyph_palette.clear();
    out.glyph_selection_ink.clear();
    out.raster_blink.clear();
    out.raster_selection_ink.clear();
    out.cursor_glyphs.clear();
    out.cursor_palette.clear();
    out.cursor_rasters.clear();
}

/// Reuses common font/backend dispatch or an already imported startup outline.
/// Full-font mode shares fallback dispatch with the ordinary row builder.
/// Native outlines and COLR paint graphs remain vectors under every effect.
/// Browser and bitmap emoji sources redraw or select strikes at the sized request.
/// Startup packs already hold unhinted vectors and need no TTF extraction.
/// The matching appearance arrays stay parallel when one request emits many layers.
fn prepare_source(
    context: &mut TextContext,
    out: &mut CachedRow,
    source: &Source,
    p: &puzzle_text::glyph::GlyphPlacement,
    dpr: f32,
    scale: f32,
) -> Result<(), String> {
    if let Some(fonts) = context.fonts.as_mut() {
        // Shared source dispatch chooses Slug for outlines/COLR and sized pixel-only ink.
        append_glyphs(
            fonts,
            &mut context.backend,
            &mut context.raster,
            out,
            &[*p],
            source.style,
            source.italic,
            dpr,
            scale,
        )?;
    } else {
        // Startup pack outlines are already unhinted and can be instanced at any size.
        let handle = context
            .backend
            .get(p.key)
            .ok_or("effect pack glyph unavailable")?;
        crate::glyph_ink::append_ink(&mut out.glyphs, p, handle, source.italic, false, false);
        // Pack glyphs are monochrome: selection may recolor every emitted instance.
        out.glyph_palette.resize(out.glyphs.len(), false);
        out.glyph_blink.resize(out.glyphs.len(), source.style.blink);
        out.glyph_selection_ink
            .resize(out.glyphs.len(), source.style.selection_ink);
    }
    Ok(())
}
