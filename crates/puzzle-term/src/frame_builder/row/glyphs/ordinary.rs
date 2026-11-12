//! Ordinary faces preserve grid placement when D-39 chooses bitmap overflow.
//!
//! Layout has already chosen a face, glyph, style and fixed cell span. This
//! emitter chooses only its ink store, keeping origin and advance unchanged.
//! The vector atlas cannot evict pages while these instances are being collected.
//! A capacity rejection therefore samples the covered outline through the CPU
//! oracle and publishes it into the ordinary bounded raster atlas for this frame.
//! Exact size, fitting span, baseline and DPR identify the resulting device crop.
//! Warm mask hits skip both outline extraction and CPU pixel sampling.
//! Slug handles stay generation-checked; row batches stamp actual draws separately.
//! A COLR graph must remain complete and ordered when the backend changes.
//! Mixing vector and bitmap layers would change its source-over ordering relative
//! to neighboring text, so the overflow branch rasterizes the entire graph.
//! Monochrome outlines become tintable masks; multicolor graphs become RGBA crops.
//! Both use the same row-relative raster emitter as platform fallback glyphs.
//! That emitter preserves blink, selection tint and isolated block-cursor copies.
//! Empty contextual glyphs remain occupied layout components without visible ink.
//! Their explicit Empty result is recorded on the retained source for diagnostics.
//! Missing atlas handles alone never imply an empty glyph: eviction can remove ink.
//! This metadata changes neither shaping nor the draw instances used by any host.
//! Malformed font failures retain their reports rather than being called capacity.
//! Native Auto glyphs may transfer prepared outlines into the smaller bitmap store.
//! The chosen mask already contains fitting and synthesis, applied exactly once.
//! Changing to Slug or requesting an effect must bypass retained outline masks.
use super::{CachedRow, FontSet, GlyphPlacement, RunStyle, SlugBackend};
use puzzle_text::{
    glyph::PreparedGlyph,
    raster::{RasterAtlas, RasterKey},
};

/// Loaded-face dispatch keeps diagnostics, complete color graphs and ordinary outlines distinct.
/// This function sees only faces already selected by the layout's fallback chain.
/// It never initiates platform discovery or changes the character's cell width.
/// A color-font probe returning None permits the ordinary outline path.
/// Complete graphs and retained masks return early so no glyph is emitted twice.
/// Appearance metadata is appended by the ink-specific emitter below this seam.
#[allow(
    clippy::too_many_arguments,
    reason = "loaded ink dispatch borrows independent stores"
)]
pub(super) fn glyph(
    fonts: &mut FontSet,
    backend: &mut SlugBackend,
    raster: &mut RasterAtlas,
    cache: &mut CachedRow,
    p: &GlyphPlacement,
    style: RunStyle,
    italic: bool,
    dpr: f32,
) -> Result<(), String> {
    // Unsupported scalars keep their diagnostic; capacity never sends covered ink here.
    if p.key.face == puzzle_text::builtin::TOFU_FACE {
        super::tofu::append(
            fonts,
            backend,
            raster,
            cache,
            p,
            p.key.glyph_id,
            false,
            style,
            dpr,
        )?;
    } else if !retained(backend, raster, cache, p, style, italic, dpr)
        && !color(fonts, backend, raster, cache, p, style, italic, dpr)?
    {
        // Both color and monochrome paths share the single retained-mask decision.
        append(fonts, backend, raster, cache, p, style, italic, dpr)?;
    }
    Ok(())
}

/// Reuses authored or pressure ink once, before choosing color versus monochrome.
/// An empty atlas cannot contain this placement, so vector-only contexts need no
/// raster key construction or hash lookup. Retired keys still count as populated:
/// an earlier draw batch may own their pixels after the physical page is reused.
fn retained(
    backend: &SlugBackend,
    raster: &mut RasterAtlas,
    cache: &mut CachedRow,
    p: &GlyphPlacement,
    style: RunStyle,
    italic: bool,
    dpr: f32,
) -> bool {
    if raster.is_empty() {
        return false;
    }
    // Outline masks cannot supply transformed ink; image-only glyphs still can.
    let effect = p.effect || italic || !backend.grid_masks_enabled();
    if let Some(handle) = raster.get_for_placement(raster_key(p, dpr), effect) {
        super::super::super::raster::append(cache, p, handle, style.blink, style.selection_ink);
        return true;
    }
    false
}

/// A complete color graph either stays vector or retains all layers as oracle pixels.
#[allow(
    clippy::too_many_arguments,
    reason = "color overflow needs both independent stores"
)]
pub(super) fn color(
    fonts: &FontSet,
    backend: &mut SlugBackend,
    raster: &mut RasterAtlas,
    cache: &mut CachedRow,
    p: &GlyphPlacement,
    style: RunStyle,
    italic: bool,
    dpr: f32,
) -> Result<bool, String> {
    match backend.prepare_color(p, fonts) {
        Ok(Some(color)) => {
            // Expand complete layers consecutively, using one shared union for fitting.
            super::append_color(cache, p, color, style, italic);
            return Ok(true);
        }
        Ok(None) => return Ok(false),
        Err(error) => {
            // Malformed font reports must not accidentally select the capacity fallback.
            if !backend.atlas().needs_eviction() {
                return Err(error);
            }
        }
    }
    // Traversal is repeated only after an exceptional capacity rejection, never on a hit.
    let layers = fonts
        // Re-extract only this glyph; the font owner keeps its parsed tables reusable.
        .face(p.key.face)
        .ok_or("color face absent")?
        .color_layers(p.key.glyph_id, p.color)?
        .ok_or("color graph absent")?;
    // CPU sampling keeps all layers and gradients in their source-over order.
    let samples = backend.rasterize_color(p, &layers.layers)?;
    // One RGBA crop keeps source-over ordering intact across the backend boundary.
    let handle = raster.prepare(raster_key(p, dpr), samples)?;
    super::super::super::raster::append(cache, p, handle, style.blink, style.selection_ink);
    Ok(true)
}

/// Warm bitmap hits bypass outline extraction and oracle sampling like vector hits.
#[allow(
    clippy::too_many_arguments,
    reason = "ordinary dispatch crosses separate retained owners"
)]
pub(super) fn append(
    fonts: &mut FontSet,
    backend: &mut SlugBackend,
    raster: &mut RasterAtlas,
    cache: &mut CachedRow,
    p: &GlyphPlacement,
    style: RunStyle,
    italic: bool,
    dpr: f32,
) -> Result<(), String> {
    // Synthesis is determined by selected face capabilities, not by backend pressure.
    let bold = fonts.needs_synthetic_bold(style.face, p.key.face);
    // A real fallback italic face needs no shear; missing bold remains synthesis.
    let fallback = p.key.face != fonts.primary_face(style.face);
    // Fallback bounds are fitted inside the same cell span on either ink backend.
    match backend.prepare_fitted(p, fonts, fallback, italic, bold)? {
        PreparedGlyph::Pending => {
            // Layout remains unchanged while the one accepted job owns real ink.
            super::tofu::append(fonts, backend, raster, cache, p, 0xfffd, true, style, dpr)?;
        }
        PreparedGlyph::Vector(handle) => {
            // Only vector instances receive the retained affine and duplicate-bold policy.
            crate::glyph_ink::append_ink(&mut cache.glyphs, p, handle, italic, bold, fallback);
        }
        PreparedGlyph::GridOutline {
            ink,
            placement,
            style: ink_style,
        } => {
            let handle = raster.prepare_grid(raster_key(p, dpr), &placement, *ink, ink_style)?;
            super::super::super::raster::append(cache, p, handle, style.blink, style.selection_ink);
        }
        PreparedGlyph::Rasterized(samples) => {
            // The oracle has already applied fit/style to these device-sized samples.
            // The common raster emitter adds blink and selection metadata exactly once.
            let handle = raster.prepare_outline_mask(raster_key(p, dpr), samples)?;
            super::super::super::raster::append(cache, p, handle, style.blink, style.selection_ink);
        }
        // Space-like contextual components have no drawable ink.
        PreparedGlyph::Empty => {
            // Harfrust can emit contourless components before a ligature's visible glyph.
            // Retain that explicit backend result so diagnostics do not call it missing ink.
            if let Some(source) = cache
                .sources
                .last_mut()
                .filter(|s| s.placement.key == p.key)
            {
                source.empty = true;
            }
        }
        // This path owns samples; an external raster rectangle would lack its page owner.
        PreparedGlyph::Bitmap(_, _) => return Err("bitmap result has no raster page".into()),
    }
    Ok(())
}

/// Bitmap identity covers every device-space fitting input, independently of ink color.
pub(super) fn raster_key(p: &GlyphPlacement, dpr: f32) -> RasterKey {
    // Size alone does not identify a crop: wide spans and row baselines can differ.
    // Masks remain independent of foreground tint, so selection reuses their coverage.
    RasterKey {
        // Face and hint configuration remain part of a mask's identity.
        glyph: p.key,
        size: p.size_px.to_bits(),
        fit: [p.ink_limit_px, p.cell_height_px, p.baseline_px.to_bits()],
        dpr: dpr.to_bits(),
    }
}
