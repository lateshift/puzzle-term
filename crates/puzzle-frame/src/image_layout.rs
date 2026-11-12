//! Shared exact image tiling geometry for protocol admission and GPU upload.
//! The native owner supplies its device limit; this module owns no GPU handles.
//! Both sides charge the same filtering borders, preventing an accepted image
//! from later failing frame construction merely because its tiles need halos.
/// Unsplit axes need no halo; split axes leave room for both neighboring texels.
pub fn image_tile_step(size: [u32; 2], limit: u32) -> [u32; 2] {
    size.map(|extent| {
        // Ordinary images retain exact dimensions and require no split overhead.
        if extent <= limit {
            extent.max(1)
        } else {
            // Two reserved columns/rows supply the left/right or top/bottom neighbors.
            limit.saturating_sub(2).max(1)
        }
    })
}
/// Only internal edges receive borders; image edges retain clamp-to-edge semantics.
pub fn image_tile_border(rect: [u32; 4], size: [u32; 2]) -> [u32; 4] {
    let [x, y, width, height] = rect;
    // Saturation clips the halo at original image edges instead of duplicating padding.
    let left = x.saturating_sub(1);
    let top = y.saturating_sub(1);
    // Public callers may supply an empty intersection outside the source.
    // Such rectangles produce zero extent instead of unsigned subtraction overflow.
    [
        left,
        top,
        x.saturating_add(width)
            .saturating_add(1)
            .min(size[0])
            .saturating_sub(left),
        y.saturating_add(height)
            .saturating_add(1)
            .min(size[1])
            .saturating_sub(top),
    ]
}
/// Exact allocated RGBA bytes include two shared border texels per internal seam.
pub fn image_resident_bytes(size: [u32; 2], limit: u32) -> usize {
    // Invalid device/source geometry must fail admission, never divide by zero.
    if limit < 3 || size.contains(&0) {
        return usize::MAX;
    }
    // This uses the same structural tile partition as upload, with no pixel traversal.
    let step = image_tile_step(size, limit);
    // Summing overlapped axes gives exactly the product of all allocated tile areas.
    // Saturation makes an unrepresentable public source fail residency admission.
    // Each shared seam contributes two texels, one in either neighboring texture.
    // Axis additions must saturate too, because shared code also runs on wasm32.
    let width = (size[0] as usize)
        .saturating_add(((size[0].div_ceil(step[0]) - 1) as usize).saturating_mul(2));
    let height = (size[1] as usize)
        .saturating_add(((size[1].div_ceil(step[1]) - 1) as usize).saturating_mul(2));
    width.saturating_mul(height).saturating_mul(4)
}
