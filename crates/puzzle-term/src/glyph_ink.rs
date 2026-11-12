//! Converts backend-neutral glyph placements into Slug ink instances.
//!
//! Fallback fitting changes only the drawn outline; grid origins and
//! advances stay fixed by the text layout seam (design §8.4, D-28).
//!
//! Slug instances carry an em-to-device affine transform and an atlas
//! address. Keeping fitting here lets both the live frame builder and
//! milestone fixtures use identical ink transforms without involving
//! the font shaper in renderer-specific decisions.
use puzzle_frame::slug::SlugInstance;
use puzzle_text::builtin::TOFU_FACE;

/// Emits the original ink and an optional fixed-font bold offset (§8.3).
pub(crate) fn append_ink(
    output: &mut Vec<SlugInstance>,
    placement: &puzzle_text::glyph::GlyphPlacement,
    handle: puzzle_slug::atlas::ShapeHandle,
    shear: bool,
    synthetic_bold: bool,
    fallback: bool,
) {
    // The first draw represents the face's actual outline. A synthetic
    // bold offset is an extra paint operation, not a second grid glyph.
    let first = fitted_instance(placement, handle, shear, fallback);
    output.push(first);
    if synthetic_bold {
        // The minimum one-device-pixel offset is needed where a fixed
        // weight face has no bold counterpart; advances remain unchanged.
        // The second draw thickens a fixed face without touching either
        // its shaped advance or the next cell's integer origin.
        let mut second = first;
        second.t[0] += (placement.size_px / 24.0).round().max(1.0);
        output.push(second);
    }
}

/// Encodes a prepared outline with the placement's stable device origin.
fn instance(
    placement: &puzzle_text::glyph::GlyphPlacement,
    handle: puzzle_slug::atlas::ShapeHandle,
    shear: bool,
) -> SlugInstance {
    // The glyph key identifies prepared curves, while placement supplies
    // the snapped origin; the atlas never chooses terminal cell geometry.
    // Font outlines use font ppem; procedural paths use cell height as
    // their em scale. Both arrive here in the placement's size field.
    let size = placement.size_px;
    let slant = if shear {
        // Twelve degrees matches the fallback italic policy in §8.3.
        // Shear belongs in the affine map, leaving stored curves reusable.
        size * 12.0_f32.to_radians().tan()
    } else {
        0.0
    };
    // M maps em x-right/y-up to device x-right/y-down. The platform has
    // already rounded the origin; an italic shear changes ink, not cells.
    // The two packed texture coordinates come from the current atlas
    // handle, so later GPU uploads refer to exactly this CPU preparation.
    SlugInstance {
        m: [size, 0.0, slant, -size],
        t: [placement.origin_px[0] as f32, placement.origin_px[1] as f32],
        bbox: handle.bbox,
        glyph_loc: puzzle_slug::encoding::encode_glyph_loc(handle.glyph_loc),
        band_info: handle.band_info,
        color: placement.color,
        paint: 0,
    }
}

/// Fits fallback ink to the grid without changing neutral cell placement.
pub(crate) fn fitted_instance(
    placement: &puzzle_text::glyph::GlyphPlacement,
    handle: puzzle_slug::atlas::ShapeHandle,
    shear: bool,
    fallback: bool,
) -> SlugInstance {
    // Layout has already assigned this glyph to its cells. Fitting may
    // alter only ink scale and translation inside that fixed allocation.
    let mut draw = instance(placement, handle, shear);
    // Builtins are already constructed on exact device-cell edges, so
    // rescaling them would break box joins and block continuity (§8.6).
    if !fallback || placement.key.face >= TOFU_FACE {
        return draw;
    }
    // Shared ink fitting applies the same uniform geometry to every backend.
    // Ordinary fallback pivots around its existing baseline; Emoji source
    // placements use an inset row rectangle instead of a foreign font ascent.
    let bounds = handle.bbox.map(|value| value * placement.size_px);
    let fit = puzzle_text::fitting::fit(
        placement,
        bounds,
        placement.color_glyph,
        placement.origin_px[1] as f32,
    );
    for value in &mut draw.m {
        *value *= fit.scale;
    }
    draw.t[0] += fit.offset[0];
    draw.t[1] += fit.offset[1];
    draw
}
