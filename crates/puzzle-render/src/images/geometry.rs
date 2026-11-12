//! Image source crops intersect hardware tiles without changing cell-anchored geometry.
//!
//! Cropping happens in original pixel coordinates before tiling. Each intersection
//! maps to its corresponding fraction of the original destination rectangle; no
//! rounding to cells or device pixels is introduced by the GPU upload layout.
//! Logical tiles partition the image, so their draws do not overlap in coverage.
//! Uploaded halos extend beyond these logical rectangles only for sampling.
//! The primitive scissor remains on the painter batch and clips all expanded tiles.
use super::*;
/// Appends every intersecting tile in original source order, preserving the placement.
pub(super) fn expand(
    entry: &Entry,
    quad: RasterInstance,
    original: u32,
    quads: &mut Vec<RasterInstance>,
    draws: &mut Vec<Draw>,
) {
    // Convert normalized source edges once, before intersecting any texture tile.
    let source = [
        quad.uv[0] * entry.size[0] as f32,
        quad.uv[1] * entry.size[1] as f32,
        quad.uv[2] * entry.size[0] as f32,
        quad.uv[3] * entry.size[1] as f32,
    ];
    // Iteration follows source row/column order; z order remains the outer batch order.
    for tile in &entry.tiles {
        let [x, y, w, h] = tile.rect.map(|n| n as f32);
        // Intersect source edges, not destination cells, so arbitrary crops remain exact.
        let crop = [
            source[0].max(x),
            source[1].max(y),
            source[2].min(x + w),
            source[3].min(y + h),
        ];
        // A crop can touch a tile edge without covering any of that tile.
        if crop[0] >= crop[2] || crop[1] >= crop[3] {
            continue;
        }
        // The destination was aspect-fitted already; this only preserves that affine map.
        let scale = [
            quad.rect[2] / (source[2] - source[0]),
            quad.rect[3] / (source[3] - source[1]),
        ];
        // Position the subquad at the original source fraction inside the full placement.
        let rect = [
            quad.rect[0] + (crop[0] - source[0]) * scale[0],
            quad.rect[1] + (crop[1] - source[1]) * scale[1],
            (crop[2] - crop[0]) * scale[0],
            (crop[3] - crop[1]) * scale[1],
        ];
        // Each appended quad gets one contiguous instance in the reusable GPU stream.
        let instance = quads.len() as u32;
        // Sampling coordinates include the upload halo, not only its drawn interior.
        let [ux, uy, uw, uh] = tile.upload.map(|n| n as f32);
        // A one-tile ordinary image retains the source UVs exactly. Oversized
        // sources use normalized coordinates relative to each hardware texture.
        quads.push(RasterInstance {
            rect,
            uv: [
                (crop[0] - ux) / uw,
                (crop[1] - uy) / uh,
                (crop[2] - ux) / uw,
                (crop[3] - uy) / uh,
            ],
            // Preserve image sampling/tint flags from the original frame command.
            ..quad
        });
        // Preserve the original array offset so z-slot batches select their own quads.
        // A cloned bind group retains texture handles without copying source texels.
        draws.push(Draw {
            original,
            instance,
            binding: tile.binding.clone(),
        });
    }
}
