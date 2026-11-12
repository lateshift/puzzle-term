//! Re-imports validated pack ink after page eviction without preparing its font.
//!
//! The directory's band lists already contain the selected, sorted memberships.
//! Recover their unique curve addresses and translate those indices into an owned
//! prepared shape. Coordinates and band order are copied exactly, never searched
//! or approximated. The path exists only for the ordinary CPU-oracle overflow.
//! Band-list positions and contour positions serve different purposes: the lists
//! retain descending ray order, while geometry retains directed contour order.
//! Both orders must survive recovery to preserve coverage and early ray exits.
use super::{FontPack, PackGlyph};
use crate::preparation::Outline;
use puzzle_slug::{
    bands::BandSet,
    shape::{QuadCurve, QuadShape},
};
use std::collections::BTreeMap;

/// Decodes only already validated texture membership for address-stable pack reuse.
impl FontPack {
    /// Transfers one glyph's precomputed geometry, preserving exact winding and bands.
    pub(crate) fn prepared(&self, glyph: &PackGlyph) -> Result<Option<Outline>, String> {
        // Advance-only cmap entries (spaces) have no band header to recover.
        let Some(handle) = glyph.handle else {
            return Ok(None);
        };
        // Pack addresses use the fixed 4096-wide texture stride (§9.2).
        let base = usize::from(handle.glyph_loc[1]) * 4096 + usize::from(handle.glyph_loc[0]);
        // The high half stores the horizontal maximum; the low half is vertical.
        // Headers themselves remain horizontal first, as required by pack_bands.
        let (max_x, max_y, _) = puzzle_slug::encoding::decode_band_info(handle.band_info);
        let horizontal = usize::from(max_y) + 1;
        let count = horizontal + usize::from(max_x) + 1;
        // Header blocks never wrap; checked slicing rejects corrupt directories.
        let headers = self
            .bands
            .get(base..base + count)
            .ok_or("pack headers absent")?;
        // A curve can appear in many bands, but its geometry is copied only once.
        // Texture-address ordering preserves the packer's original contour sequence.
        let mut addresses = BTreeMap::new();
        // Preserve each sorted band membership; rebuilding bands would discard pack work.
        let mut lists = Vec::with_capacity(count);
        for &[len, offset] in headers {
            // Offsets are relative to this glyph header, with ordinary row wrapping.
            let start = base + usize::from(offset);
            // List lengths are validated independently of the directory/header bounds.
            let members = self
                .bands
                .get(start..start + usize::from(len))
                .ok_or("pack list absent")?;
            let mut list = Vec::with_capacity(members.len());
            for &[x, y] in members {
                // Sorted texture addresses retain the original directed contour order.
                let address = usize::from(y) * 4096 + usize::from(x);
                addresses.insert(address, 0);
                list.push(address);
            }
            lists.push(list);
        }
        // Reconstruct both the exact Slug shape and the overflow oracle's path.
        // Neither curve approximation nor expensive candidate-band selection runs.
        let (shape, path) = geometry(self, &mut addresses, handle.bbox)?;
        // Remap texture locations to dense curve indices for the ordinary page allocator.
        for list in &mut lists {
            for address in list {
                // INVARIANT: every member was inserted into addresses in the loop above.
                *address = addresses[address];
            }
        }
        // The pack concatenates horizontal headers first, then vertical headers.
        let vertical = lists.split_off(horizontal);
        Ok(Some(Outline {
            path,
            shape,
            bands: BandSet {
                horizontal: lists,
                vertical,
            },
        }))
    }
}

/// Copies f32 curves and reconstructs contour boundaries from shared endpoints.
fn geometry(
    pack: &FontPack,
    addresses: &mut BTreeMap<usize, usize>,
    bbox: [f32; 4],
) -> Result<(QuadShape, kurbo::BezPath), String> {
    // Allocate only on eviction or first style import; warm draws borrow atlas handles.
    let mut curves = Vec::with_capacity(addresses.len());
    let mut contours = Vec::new();
    // The oracle accepts a path when the protected frame exhausts vector capacity.
    let mut path = kurbo::BezPath::new();
    // Record the dense start of the contour that is currently being reconstructed.
    let mut first = 0;
    // An absent start means the next directed curve begins a new closed contour.
    let mut start = None;
    for (&address, index) in addresses.iter_mut() {
        // C1 guarantees the endpoint texel follows its curve without row wrapping.
        // Still check both texels: decoded pack data remains an untrusted input surface.
        let pair = pack
            .curves
            .get(address..address + 2)
            .ok_or("pack curve absent")?;
        // Each first texel stores p1.xy/p2.xy; the next texel stores p3.xy.
        let curve = QuadCurve {
            p1: [pair[0][0], pair[0][1]],
            p2: [pair[0][2], pair[0][3]],
            p3: [pair[1][0], pair[1][1]],
        };
        // Fonts and packs close contours explicitly; no heuristic winding reversal is needed.
        if start.is_none() {
            start = Some(curve.p1);
            first = curves.len();
            path.move_to(point(curve.p1));
        }
        // The membership remap refers to the curve index assigned in this same pass.
        *index = curves.len();
        // f32 em coordinates survive conversion to kurbo's f64 without rounding.
        path.quad_to(point(curve.p2), point(curve.p3));
        curves.push(curve);
        if start == Some(curve.p3) {
            // The source closes each contour explicitly before starting another.
            contours.push(first..curves.len());
            path.close_path();
            start = None;
        }
    }
    // A partial contour would silently corrupt winding; reject it before publishing ink.
    if start.is_some() {
        return Err("pack contour did not close".into());
    }
    // Keep the directory bounds unchanged, so recovery has the identical instance quad.
    Ok((
        QuadShape {
            curves,
            contours,
            bbox,
        },
        path,
    ))
}

/// Conversion is exact because every f32 coordinate is representable as f64.
fn point(at: [f32; 2]) -> kurbo::Point {
    kurbo::Point::new(f64::from(at[0]), f64::from(at[1]))
}
