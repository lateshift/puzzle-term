//! Cropped straight-RGBA readback becomes authored color or reusable coverage.
//!
//! Canvas ImageData is straight alpha. White monochrome edges therefore
//! have RGB=255 even when alpha is fractional. Comparing RGB directly to
//! alpha would misclassify every antialiased text edge as a color glyph.
//! In premultiplied coordinates white coverage has each RGB equal to alpha.
//! A non-white visible pixel selects color, including black emoji outlines.
//! Alpha-zero pixels are ignored when classifying color: Canvas may leave
//! arbitrary straight RGB in transparent samples without any visible ink.
//! The crop bounds include exactly the nonzero-alpha pixels, with an exclusive
//! right/bottom edge compatible with the shelf and normalized UV contract.
//! Coverage-only crops use one byte per pixel and retain fractional edge alpha.
//! Authored-color crops preserve four channels, including transparent interiors.
//! A hollow ideograph or emoji counter therefore stays transparent after packing.
//! The primary baseline offset is retained separately from cropped dimensions.
//! This prevents transparent top rows from changing a glyph's apparent baseline.
use puzzle_term::text::glyph::{BitmapKind, RasterGlyph};
/// Extracts only ink, retaining the crop position relative to the grid baseline.
pub(super) fn crop(
    data: &[u8],
    width: u32,
    height: u32,
    baseline: i32,
) -> Result<RasterGlyph, String> {
    // Start inverted so an entirely transparent result has no valid rectangle.
    let mut bounds = [width, height, 0, 0];
    let mut color = false;
    for (index, pixel) in data.chunks_exact(4).enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let x = index as u32 % width;
        let y = index as u32 / width;
        bounds[0] = bounds[0].min(x);
        bounds[1] = bounds[1].min(y);
        bounds[2] = bounds[2].max(x + 1);
        bounds[3] = bounds[3].max(y + 1);
        // White fill is coverage; any authored visible RGB requires RGBA storage.
        color |= pixel[..3].iter().any(|channel| *channel != 255);
    }
    let [left, top, right, bottom] = bounds;
    if right <= left || bottom <= top {
        return Err("browser font produced no ink".into());
    }
    // Preserve exact device samples; the fitting step already chose their size.
    let channels = if color { 4 } else { 1 };
    let mut pixels = Vec::with_capacity(((right - left) * (bottom - top) * channels) as usize);
    for y in top..bottom {
        for x in left..right {
            let index = ((y * width + x) * 4) as usize;
            if color {
                pixels.extend_from_slice(&data[index..index + 4]);
            } else {
                pixels.push(data[index + 3]);
            }
        }
    }
    Ok(RasterGlyph {
        offset_px: [left as i32, top as i32 - baseline],
        width: right - left,
        height: bottom - top,
        pixels,
        kind: if color {
            BitmapKind::Color
        } else {
            BitmapKind::Mask
        },
    })
}
