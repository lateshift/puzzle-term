//! Reference horizontal and vertical ray loops over packed CPU texels.
//!
//! The parent module carries the Slug author's copyright and SPDX credit.
//! Every data fetch mirrors SlugPixelShader.hlsl. Packing keeps headers and
//! each list within a row because shader increments x without wrapping.
//!
//! Slug estimates coverage by casting two rays through the pixel centre.
//! The horizontal ray solves where a curve crosses y=0 in sample-relative
//! em space and accumulates signed x-side coverage. The vertical ray swaps
//! axes and uses the opposite sign convention, as in the HLSL reference.
//! A curve may contribute zero, one or two eligible roots; `CalcRootCode`
//! determines which roots represent directed crossings before solving.
//!
//! Each band header gives a list of curve-texture coordinates. Its offset
//! can wrap rows through `CalcBandLoc`, but the header and the list's own
//! indexed reads cannot. Address checks here turn a packing bug into a
//! report rather than an out-of-bounds panic. The first curve texel carries
//! p1 and p2; its immediate neighbour carries p3. C1 guarantees that
//! neighbour stays on the same row.
//!
//! Band lists are sorted by descending maximum coordinate along the ray.
//! Once a curve's entire control hull is farther than half a pixel behind
//! the sample, no later curve can contribute. The early exit is part of
//! the reference's runtime behavior and depends on the packer's sort order.
//! Weights peak when a crossing is near the pixel centre; `CalcCoverage`
//! uses them to blend the two directional estimates.
use super::{
    ReferenceError,
    poly::{calc_root_code, solve_horiz_poly, solve_vert_poly},
};
use crate::{
    atlas::ShapeHandle,
    atlas::{SlugAtlas, TEXTURE_WIDTH},
};

/// Horizontal or vertical contribution and its edge proximity weight.
pub(super) type RayResult = (f32, f32);

/// If a list offset crosses a row, wrap its base like CalcBandLoc.
/// The offset is relative to the shape's header origin, not the texture row.
fn calc_band_loc(glyph: [u16; 2], offset: u16) -> (usize, usize) {
    let x = usize::from(glyph[0]) + usize::from(offset);
    let y = usize::from(glyph[1]) + (x >> 12);
    (x & (TEXTURE_WIDTH - 1), y)
}

/// Fetches one packed curve and subtracts the render coordinate.
/// The resulting points are centred at the pixel being shaded, so solving
/// for x=0 or y=0 casts a ray through that pixel without extra translation.
fn curve_relative(
    atlas: &SlugAtlas,
    location: [u16; 2],
    at: [f32; 2],
) -> Result<([f32; 4], [f32; 2]), ReferenceError> {
    let x = usize::from(location[0]);
    let row = usize::from(location[1]);
    // The reference reads p3 at x+1 with no row carry (packing rule C1).
    if x + 1 >= TEXTURE_WIDTH {
        return Err(ReferenceError::PackedAddress);
    }
    let first = atlas
        .curve_texels()
        .get(row * TEXTURE_WIDTH + x)
        .ok_or(ReferenceError::PackedAddress)?;
    let next = atlas
        .curve_texels()
        .get(row * TEXTURE_WIDTH + x + 1)
        .ok_or(ReferenceError::PackedAddress)?;
    Ok((
        [
            first[0] - at[0],
            first[1] - at[1],
            first[2] - at[0],
            first[3] - at[1],
        ],
        [next[0] - at[0], next[1] - at[1]],
    ))
}

/// Reads a band's header and list location as in the reference shader.
/// Header x is advanced directly, whereas its list offset uses CalcBandLoc.
fn band_header(
    atlas: &SlugAtlas,
    handle: ShapeHandle,
    index: usize,
) -> Result<([u16; 2], usize, usize), ReferenceError> {
    let x = usize::from(handle.glyph_loc[0]) + index;
    // B1 protects this direct header index from crossing the row edge.
    if x >= TEXTURE_WIDTH {
        return Err(ReferenceError::PackedAddress);
    }
    let header = *atlas
        .band_texels()
        .get(usize::from(handle.glyph_loc[1]) * TEXTURE_WIDTH + x)
        .ok_or(ReferenceError::PackedAddress)?;
    let (base_x, base_y) = calc_band_loc(handle.glyph_loc, header[1]);
    Ok((header, base_x, base_y))
}

/// Fetches one curve location from a non-wrapping list.
/// The packer aligns a list to the next row if it would cross one.
fn listed_curve(
    atlas: &SlugAtlas,
    base_x: usize,
    base_y: usize,
    index: usize,
) -> Result<[u16; 2], ReferenceError> {
    // HLSL increments listBase.x directly; wrapping here would conceal B1.
    if base_x + index >= TEXTURE_WIDTH {
        return Err(ReferenceError::PackedAddress);
    }
    atlas
        .band_texels()
        .get(base_y * TEXTURE_WIDTH + base_x + index)
        .copied()
        .ok_or(ReferenceError::PackedAddress)
}

/// SlugRender's horizontal loop: crossings of y = 0 contribute to xcov.
/// The returned weight records the closest root within one pixel of centre.
pub(super) fn horizontal(
    atlas: &SlugAtlas,
    handle: ShapeHandle,
    band_index: usize,
    at: [f32; 2],
    pixels_per_em: [f32; 2],
) -> Result<RayResult, ReferenceError> {
    let (header, base_x, base_y) = band_header(atlas, handle, band_index)?;
    // Winding starts outside the outline. Directed crossings add or
    // subtract fractional coverage as the ray enters and leaves it.
    let mut xcov = 0.0;
    let mut xwgt: f32 = 0.0;
    for curve_index in 0..usize::from(header[0]) {
        let curve_loc = listed_curve(atlas, base_x, base_y, curve_index)?;
        let (p12, p3) = curve_relative(atlas, curve_loc, at)?;
        // Descending maximum x means no remaining curve can affect this
        // pixel after the first one entirely left of its −0.5 window.
        if p12[0].max(p12[2]).max(p3[0]) * pixels_per_em[0] < -0.5 {
            break;
        }
        let code = calc_root_code(p12[1], p12[3], p3[1]);
        if code != 0 {
            // Scale root positions from ems to pixel widths before the
            // reference's ±0.5 coverage window and edge-proximity weight.
            let r = solve_horiz_poly(p12, p3).map(|v| v * pixels_per_em[0]);
            if code & 1 != 0 {
                // The first eligible directed crossing enters the fill.
                xcov += (r[0] + 0.5).clamp(0.0, 1.0);
                xwgt = xwgt.max((1.0 - r[0].abs() * 2.0).clamp(0.0, 1.0));
            }
            if code > 1 {
                // The second root exits it; its direction reverses sign.
                xcov -= (r[1] + 0.5).clamp(0.0, 1.0);
                xwgt = xwgt.max((1.0 - r[1].abs() * 2.0).clamp(0.0, 1.0));
            }
        }
    }
    Ok((xcov, xwgt))
}

/// SlugRender's vertical loop: crossings of x = 0 contribute to ycov.
/// Its sign convention is reversed because the ray direction is rotated.
pub(super) fn vertical(
    atlas: &SlugAtlas,
    handle: ShapeHandle,
    band_index: usize,
    at: [f32; 2],
    pixels_per_em: [f32; 2],
) -> Result<RayResult, ReferenceError> {
    // Vertical headers follow all horizontal headers in the same block.
    let (_, max_y, _) = crate::encoding::decode_band_info(handle.band_info);
    let horizontal_count = usize::from(max_y) + 1;
    let (header, base_x, base_y) = band_header(atlas, handle, horizontal_count + band_index)?;
    let mut ycov = 0.0;
    let mut ywgt: f32 = 0.0;
    for curve_index in 0..usize::from(header[0]) {
        let curve_loc = listed_curve(atlas, base_x, base_y, curve_index)?;
        let (p12, p3) = curve_relative(atlas, curve_loc, at)?;
        // The vertical list is sorted by descending maximum y, so the
        // same half-pixel early exit is valid with axes exchanged.
        if p12[1].max(p12[3]).max(p3[1]) * pixels_per_em[1] < -0.5 {
            break;
        }
        let code = calc_root_code(p12[0], p12[2], p3[0]);
        if code != 0 {
            // This is the same quadratic solver with axes exchanged; its
            // results are vertical displacements in pixel widths.
            let r = solve_vert_poly(p12, p3).map(|v| v * pixels_per_em[1]);
            if code & 1 != 0 {
                // Rotating the ray flips the signed first-root contribution.
                ycov -= (r[0] + 0.5).clamp(0.0, 1.0);
                ywgt = ywgt.max((1.0 - r[0].abs() * 2.0).clamp(0.0, 1.0));
            }
            if code > 1 {
                // The paired root restores winding after leaving the fill.
                ycov += (r[1] + 0.5).clamp(0.0, 1.0);
                ywgt = ywgt.max((1.0 - r[1].abs() * 2.0).clamp(0.0, 1.0));
            }
        }
    }
    Ok((ycov, ywgt))
}
