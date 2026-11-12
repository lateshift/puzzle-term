//! Reuses diagnostic border/digits so absent Unicode floods cannot fill the shape atlas.
//!
//! All parts retain the complete cell's origin and scale. Individual digit bounds
//! must never trigger fallback fitting, which would center or enlarge the label.
//! Each component's identity contains its digit position and the cell geometry.
//! The full Unicode scalar is assembled from those few reusable identities.
//! A provisional source owns no final glyph key until its worker completes.
//! Its diagnostic uses this separate procedural face, preserving that distinction.
//! When the vector store is pinned, the same components may use oracle masks.
//! Masks carry the original cell transform and never fit a digit independently.
//! Ink color stays outside coverage, so the diagnostic can obey selection tint.
//! Raster shelf ownership is acquired before any component quad is published.
//! Earlier vector addresses remain stable through this entire row build.
//! No atlas page eviction or discovery occurs while a box's parts are collected.
//! A device-size change selects different mask crops through the placement key.
//! Whitespace-like parts do not manufacture an empty raster rectangle.
use super::{CachedRow, RunStyle, SlugBackend};
use puzzle_text::{
    builtin::BuiltinCell,
    glyph::{GlyphPlacement, PreparedGlyph},
    raster::RasterAtlas,
};

/// Emits a bounded set of shared component shapes for an ordinary or provisional box.
#[allow(
    clippy::too_many_arguments,
    reason = "diagnostic ink crosses both atlas owners"
)]
pub(super) fn append(
    fonts: &mut super::FontSet,
    backend: &mut SlugBackend,
    raster: &mut RasterAtlas,
    cache: &mut CachedRow,
    p: &GlyphPlacement,
    code: u32,
    provisional: bool,
    style: RunStyle,
    dpr: f32,
) -> Result<(), String> {
    // Ordinary builtin placements encode the complete cell geometry in their key.
    // Pending source placements instead carry the allocated span and row height.
    let cell = if provisional {
        BuiltinCell::new(p.ink_limit_px, p.cell_height_px, 1)?
    } else {
        BuiltinCell::unpack(p.key.instance)?
    };
    let mut part = *p;
    if provisional {
        // Source placements use the font baseline; procedural ink uses the cell bottom.
        // Keep baseline correction checked even for externally constructed placements.
        // Invalid coordinates reject preparation rather than panicking on integer overflow.
        let delta = i32::from(cell.height)
            .checked_sub(p.baseline_px.round() as i32)
            .ok_or("provisional tofu baseline exceeds device coordinates")?;
        part.origin_px[1] = part.origin_px[1]
            .checked_add(delta)
            .ok_or("provisional tofu origin exceeds device coordinates")?;
        part.size_px = f32::from(cell.height);
    }
    // Component identities occupy their own procedural face, separate from finished source ink.
    part.key.face = puzzle_text::builtin::TOFU_COMPONENT_FACE;
    // Geometry remains part of every component key: a DPR or font-size change cannot
    // accidentally reuse the border or digit of a differently sized diagnostic box.
    part.key.instance = cell.pack()?;
    // At most 129 shapes cover all codepoint labels at one cell geometry. Combining
    // digit positions in instances prevents a 46,607-scalar flood filling the atlas.
    parts(fonts, backend, raster, cache, part, cell, code, style, dpr)
}

/// A diagnostic remains visible even when covered current-frame ink pins the vector store.
#[allow(
    clippy::too_many_arguments,
    reason = "component ink preserves cell geometry and appearance"
)]
/// Fitting belongs to the whole box; independently centering digits would distort its label.
/// A cached pressure mask retains the original component offset rather than refitting a digit.
fn parts(
    fonts: &mut super::FontSet,
    backend: &mut SlugBackend,
    raster: &mut RasterAtlas,
    cache: &mut CachedRow,
    mut part: GlyphPlacement,
    cell: BuiltinCell,
    code: u32,
    style: RunStyle,
    dpr: f32,
) -> Result<(), String> {
    for id in cell.tofu_components(code) {
        part.key.glyph_id = id;
        // The normal diagnostic path already has a shared vector. Avoid probing
        // the bitmap map for every digit on each provisional row rebuild.
        if let Some(handle) = backend.get(part.key) {
            crate::glyph_ink::append_ink(&mut cache.glyphs, &part, handle, false, false, false);
            continue;
        }
        // The shared key describes this component's complete device crop.
        let key = super::ordinary::raster_key(&part, dpr);
        // A retained pressure mask keeps the border/digits at their original offsets.
        if let Some(handle) = raster.get(key) {
            super::super::super::raster::append(
                cache,
                &part,
                handle,
                style.blink,
                style.selection_ink,
            );
            continue;
        }
        // Warm boxes reuse packed shapes; only a previously unseen position/digit
        // requires a small procedural contour and Slug preparation.
        let prepared = backend.prepare_fitted(&part, fonts, false, false, false)?;
        append_prepared(raster, cache, &part, key, prepared, cell, style)?;
    }
    Ok(())
}

/// Publishes one prepared component without discarding a refused border's visible ink.
/// Vector and mask results retain the complete cell transform, so each digit
/// remains at its original label position instead of being fitted independently.
/// Pending digits can wait; a pending border must make the cell visible immediately.
fn append_prepared(
    raster: &mut RasterAtlas,
    cache: &mut CachedRow,
    part: &GlyphPlacement,
    key: puzzle_text::raster::RasterKey,
    prepared: PreparedGlyph,
    cell: BuiltinCell,
    style: RunStyle,
) -> Result<(), String> {
    match prepared {
        PreparedGlyph::Vector(handle) => {
            crate::glyph_ink::append_ink(&mut cache.glyphs, part, handle, false, false, false);
        }
        PreparedGlyph::GridOutline { .. } => {
            return Err("procedural parts must remain vector".into());
        }
        PreparedGlyph::Rasterized(samples) => {
            // A current-frame vector shortage changes only the component's ink owner.
            // The raster quad uses the same bottom baseline and unscaled digit bounds.
            let handle = raster.prepare_outline_mask(key, samples)?;
            super::super::super::raster::append(
                cache,
                part,
                handle,
                style.blink,
                style.selection_ink,
            );
        }
        PreparedGlyph::Pending if part.key.glyph_id == 0 => {
            pending_border(cache, part, cell);
        }
        PreparedGlyph::Empty | PreparedGlyph::Pending => {}
        PreparedGlyph::Bitmap(_, _) => {
            return Err("diagnostic bitmap has no raster page".into());
        }
    }
    Ok(())
}

/// A declined diagnostic border uses the existing rectangle pipeline without admission.
/// This runs only for the border component; digits retain their shared glyph keys.
fn pending_border(cache: &mut CachedRow, part: &GlyphPlacement, cell: BuiltinCell) {
    // A provisional diagnostic cannot itself wait for admission.
    // Its rectangular border needs no glyph preparation or atlas;
    // digits still use the shared bounded worker and publication path.
    // Row decorations already carry device coordinates into assembly,
    // so the one-pixel stroke uses the same cell crop at every DPR.
    // A completion invalidates this cached row and replaces the border
    // through normal glyph drawing, without a separate removal event.
    cache.decorations.push(puzzle_frame::RectInstance {
        rect: [
            part.origin_px[0] as f32,
            part.origin_px[1] as f32 - f32::from(cell.height),
            f32::from(cell.width),
            f32::from(cell.height),
        ],
        color: part.color,
        corner_radius: 0.0,
        stroke_width: 1.0,
        flags: 0,
    });
}
