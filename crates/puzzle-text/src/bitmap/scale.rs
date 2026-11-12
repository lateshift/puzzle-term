//! Box-area integration of straight RGBA samples (M11.5).
//!
//! Averaging RGB before multiplying alpha creates dark/colored halos around
//! emoji. Each sample contributes alpha-weighted color instead. Only the final
//! average is unpremultiplied, since the raster shader expects straight RGBA.
//! Fractional source boundaries retain their exact overlap weights: a 3→2
//! reduction includes portions of adjacent source texels, not nearest pixels.
use super::{DecodedBitmap, validate};

/// Resizes bounded samples by integrating source rectangles in premultiplied alpha.
pub fn area_resize(image: &DecodedBitmap, width: u32, height: u32) -> Result<Vec<u8>, String> {
    validate(image)?;
    // The host never needs a larger page; reject before allocating a destination.
    if width == 0 || height == 0 || width > 2048 || height > 2048 {
        return Err("invalid area-filter destination".into());
    }
    // Validated dimensions make the RGBA byte product fit u32 and usize.
    // A fully transparent box stays transparent black rather than retaining hidden RGB.
    let mut output = vec![0; (width * height * 4) as usize];
    // Work in source-pixel coordinates; f64 avoids drift across fractional boxes.
    let sx = f64::from(image.width) / f64::from(width);
    let sy = f64::from(image.height) / f64::from(height);
    // Every destination receives one integrated area, including enlargement.
    for y in 0..height {
        for x in 0..width {
            // Destination centers do not choose one source sample: its full box does.
            let box_ = [
                f64::from(x) * sx,
                f64::from(y) * sy,
                f64::from(x + 1) * sx,
                f64::from(y + 1) * sy,
            ];
            let average = integrate(image, box_);
            // Output is tightly packed row-major RGBA, without an atlas gutter yet.
            let at = ((y * width + x) * 4) as usize;
            output[at..at + 4].copy_from_slice(&average);
        }
    }
    Ok(output)
}

/// Measures every source texel's overlap with one destination pixel rectangle.
fn integrate(image: &DecodedBitmap, box_: [f64; 4]) -> [u8; 4] {
    let [left, top, right, bottom] = box_;
    // Accumulators retain byte-valued alpha; no early rounding loses faint edges.
    let mut sum = [0.0; 4];
    // Floor/ceil cover partial boundary texels; upper clamps protect source indexing.
    for y in top.floor() as u32..(bottom.ceil() as u32).min(image.height) {
        // Intersect each unit texel interval with the destination box on each axis.
        let dy = (bottom.min(f64::from(y + 1)) - top.max(f64::from(y))).max(0.0);
        for x in left.floor() as u32..(right.ceil() as u32).min(image.width) {
            let dx = (right.min(f64::from(x + 1)) - left.max(f64::from(x))).max(0.0);
            let at = ((y * image.width + x) * 4) as usize;
            let pixel = &image.pixels[at..at + 4];
            // The product of interval overlaps is this texel’s fractional area.
            let weight = dx * dy;
            // Alpha weights RGB before integration, hiding arbitrary transparent colors.
            let alpha = f64::from(pixel[3]) * weight;
            for channel in 0..3 {
                sum[channel] += f64::from(pixel[channel]) * alpha;
            }
            sum[3] += alpha;
        }
    }
    // A fully transparent box stays transparent black rather than retaining hidden RGB.
    let mut out = [0; 4];
    // Unpremultiply once, after integration, to satisfy the raster sampler contract.
    if sum[3] > 0.0 {
        for channel in 0..3 {
            out[channel] = (sum[channel] / sum[3]).round().clamp(0.0, 255.0) as u8;
        }
    }
    // Divide alpha by source area, while RGB divides by integrated alpha above.
    out[3] = (sum[3] / ((right - left) * (bottom - top)))
        .round()
        .clamp(0.0, 255.0) as u8;
    out
}
