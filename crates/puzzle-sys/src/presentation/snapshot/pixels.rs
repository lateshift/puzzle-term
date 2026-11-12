//! Top-left, crop-aware thumbnail sampling in encoded premultiplied colour space.
//!
//! Sample centres map over the whole presented rectangle, not allocation padding.
//! Bilinear interpolation keeps premultiplied edges free of transparent colour
//! fringes. No transfer function is applied: source and result are encoded sRGB.
use super::SnapshotError;

/// Preserve aspect ratio, never enlarge, and retain at least one row of a thin pane.
pub(super) fn size(source: [u32; 2], max_width: u32) -> Result<[u32; 2], SnapshotError> {
    if max_width == 0 {
        return Err(SnapshotError::InvalidWidth);
    }
    let width = source[0].min(max_width);
    // The retained native descriptor is nonzero and device-limited. Widen before
    // multiplying so even tall source images cannot overflow aspect arithmetic.
    let height = (u64::from(source[1]) * u64::from(width) / u64::from(source[0])).max(1) as u32;
    Ok([width, height])
}
/// Read only the four neighbouring source pixels for each destination sample.
pub(super) fn copy(data: &[u8], stride: usize, source: [u32; 2], size: [u32; 2], out: &mut [u8]) {
    for y in 0..size[1] {
        // One source-row pair serves the entire output scanline; native stride
        // includes allocation padding, while the crop dimensions exclude it.
        let (top, bottom, fy) = sample(y, source[1], size[1]);
        for x in 0..size[0] {
            let (left, right, fx) = sample(x, source[0], size[0]);
            // BGRA native storage becomes portable RGBA; alpha stays premultiplied.
            let offset = (y as usize * size[0] as usize + x as usize) * 4;
            for (dst, channel) in [2, 1, 0, 3].into_iter().enumerate() {
                let a = f32::from(data[top * stride + left * 4 + channel]);
                let b = f32::from(data[top * stride + right * 4 + channel]);
                let c = f32::from(data[bottom * stride + left * 4 + channel]);
                let d = f32::from(data[bottom * stride + right * 4 + channel]);
                // Interpolate horizontally before vertically. The four samples
                // remain premultiplied, so transparent pixels contribute no hue.
                // Identical source/target sizes select exact pixel centres.
                let upper = a + (b - a) * fx;
                let lower = c + (d - c) * fx;
                out[offset + dst] = (upper + (lower - upper) * fy).round() as u8;
            }
        }
    }
}
/// Clamp edge samples to the presented crop so unused allocation pixels never bleed.
fn sample(index: u32, source: u32, target: u32) -> (usize, usize, f32) {
    // The half-pixel offset pairs pixel centres, rather than grid-line edges;
    // clamping duplicates edge texels instead of sampling pool padding.
    let position =
        ((index as f32 + 0.5) * source as f32 / target as f32 - 0.5).clamp(0., (source - 1) as f32);
    let first = position as usize;
    (
        first,
        (first + 1).min(source as usize - 1),
        position - first as f32,
    )
}

#[cfg(test)]
/// Cropping, channel order, premultiplication and immutable source byte contracts.
mod tests;
