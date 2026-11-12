//! Complete negative and asynchronous pending boxes keep their identities separate.
//!
//! Source requests carry a font baseline and style class. Procedural diagnostics
//! instead carry packed cell geometry and a bottom baseline. A final negative
//! converts that metadata before drawing; a pending browser request retains its
//! source identity so completion can later replace the diagnostic on that row.
use super::{CachedRow, FontSet, GlyphPlacement, RasterAtlas, RunStyle, SlugBackend};

/// Complete unsupported lookup uses an ordinary builtin placement, not a pending source key.
///
/// `request` carries source geometry, its scalar label, style and DPR bits.
/// A negative result changes the retained identity to TOFU_FACE, which lets
/// readiness diagnostics exclude true unsupported scalars without excluding
/// a deferred covered request. Invalid coordinate ranges return an error.
/// No platform source is retried here: its candidate traversal has finished.
pub(super) fn missing(
    fonts: &mut FontSet,
    backend: &mut SlugBackend,
    raster: &mut RasterAtlas,
    cache: &mut CachedRow,
    request: (&GlyphPlacement, u32, RunStyle, u32),
) -> Result<(), String> {
    let (source, code, style, dpr) = request;
    // A procedural key packs dimensions, unlike the original font variation key.
    let cell =
        puzzle_text::builtin::BuiltinCell::new(source.ink_limit_px, source.cell_height_px, 1)?;
    let mut placement = *source;
    // Procedural coordinates use the cell bottom while source coordinates use the font baseline.
    let offset = i32::from(cell.height)
        .checked_sub(placement.baseline_px.round() as i32)
        .ok_or("tofu baseline exceeds device coordinates")?;
    // Both baseline subtraction and origin addition reject extreme host geometry.
    placement.origin_px[1] = placement.origin_px[1]
        .checked_add(offset)
        .ok_or("tofu origin exceeds device coordinates")?;
    // Bottom-baseline geometry uses the physical cell height as its shape scale.
    placement.size_px = f32::from(cell.height);
    placement.key.face = puzzle_text::builtin::TOFU_FACE;
    // Preserve the scalar label while discarding source-specific variation metadata.
    placement.key.glyph_id = code;
    placement.key.instance = cell.pack()?;
    if let Some(retained) = cache.sources.last_mut() {
        // Diagnostics must distinguish a complete negative from deferred covered ink.
        retained.placement = placement;
    }
    // Ordinary builtin preparation can produce vectors or bounded bitmap overflow.
    // The false flag means this complete negative is never a provisional source.
    super::super::tofu::append(
        fonts,
        backend,
        raster,
        cache,
        &placement,
        code,
        false,
        style,
        f32::from_bits(dpr),
    )
}

/// Pending requests draw shared box components while their font work stays asynchronous.
/// The request tuple retains the original cell geometry, scalar label and appearance.
pub(super) fn pending(
    fonts: &mut FontSet,
    backend: &mut SlugBackend,
    raster: &mut RasterAtlas,
    cache: &mut CachedRow,
    request: (&GlyphPlacement, u32, RunStyle, u32),
) -> Result<(), String> {
    let (p, code, style, dpr) = request;
    // The key stores DPR bits, not a numeric integer scale.
    super::super::tofu::append(
        fonts,
        backend,
        raster,
        cache,
        p,
        code,
        // Pending is the asynchronous browser path and always retains provisional identity.
        true,
        style,
        f32::from_bits(dpr),
    )
}
