//! Affine ink transforms preserve source cells and atlas sample quality (§11.2).
//!
//! Glyph placements stay neutral. Vector instances multiply their existing fit
//! by the final device map; bitmap instances use a forward matrix paint row.
//! Their quads include the atlas's transparent border before rotation or shear.
//! Neutral placements use device pixels; draw lists use logical pixels.
//! Conjugating the effect by DPR converts its translation once without moving cells.
//! The instance origin includes the anchor, while the linear matrix excludes it.
//! Outline curves and gradient coordinates stay in their original em root space.
//! Multiplication changes the existing fit matrix rather than replacing that fit.
//! Thus native fallback centering and proportional text mark offsets remain intact.
//! Raster sources have no curves and must be drawn again at the transformed size.
//! The largest singular value covers the strongest magnification under shear.
//! Taking only either diagonal would miss a rotated anisotropic scale.
//! Rounding the raster request upward prevents an undersized source bitmap.
//! The returned bitmap rectangle is divided back to neutral coordinates upstream.
//! Its atlas crop stays at the larger resolution and is sampled under the final map.
//! A one-source-texel gutter becomes a correspondingly smaller neutral border.
//! That border moves the quad beyond ink so every rotated edge has transparent taps.
//! UV bounds grow into the shelf packer's cleared border on all four sides.
//! Bilinear filtering premultiplies individual taps in the raster fragment shader.
use kurbo::{Affine, Point};
use puzzle_frame::{RasterInstance, SlugInstance};
use puzzle_text::glyph::slug::SlugBackend;

/// Largest singular value determines bitmap resolution under scale and shear.
pub(in crate::frame_builder) fn raster_scale(map: Affine, size: f32) -> Result<f32, String> {
    let c = map.as_coeffs();
    if !c.iter().all(|v| v.is_finite()) || map.determinant().abs() < 1e-12 {
        return Err("text effect transform is nonfinite or singular".into());
    }
    // Eigenvalues of M-transpose*M are squared singular values.
    let trace = c[..4].iter().map(|v| v * v).sum::<f64>();
    let det = map.determinant();
    let largest = ((trace + (trace * trace - 4.0 * det * det).max(0.0).sqrt()) * 0.5).sqrt();
    // Whole-device-pixel requests avoid nearly identical rotation-only raster variants.
    let scale = ((f64::from(size) * largest.max(1.0)).ceil() / f64::from(size)) as f32;
    // Outline zoom is independent of raster page capacity. A pixel-only source
    // validates its actual cropped sample allocation when it reaches the atlas.
    if !scale.is_finite() || !size.is_finite() || size <= 0.0 {
        return Err("effective text size is nonfinite or nonpositive".into());
    }
    Ok(scale)
}
/// Conjugates logical effects into physical coordinates, adding the sampled anchor.
pub(in crate::frame_builder) fn device_map(map: Affine, origin: [f32; 2], dpr: f32) -> Affine {
    Affine::translate((f64::from(origin[0] * dpr), f64::from(origin[1] * dpr)))
        * Affine::scale(f64::from(dpr))
        * map
        * Affine::scale(1.0 / f64::from(dpr))
}
/// Multiplies the ink fit only; bounds and band/paint addresses remain in root space.
pub(in crate::frame_builder) fn vector(draw: &mut SlugInstance, map: Affine) {
    let source = Affine::new([
        f64::from(draw.m[0]),
        f64::from(draw.m[1]),
        f64::from(draw.m[2]),
        f64::from(draw.m[3]),
        f64::from(draw.t[0]),
        f64::from(draw.t[1]),
    ]);
    let c = (map * source).as_coeffs();
    draw.m = [c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32];
    draw.t = [c[4] as f32, c[5] as f32];
}
/// Prepares bitmap-only forward metadata and includes one transparent source texel.
pub(in crate::frame_builder) fn bitmap(
    backend: &mut SlugBackend,
    draw: &mut RasterInstance,
    map: Affine,
    scale: f32,
    tint: u32,
    recolor: bool,
) -> Result<(), String> {
    let c = map.as_coeffs();
    let matrix = [c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32];
    let index = backend.prepare_raster_effect(matrix, tint)?;
    // Source samples were drawn at scale times the neutral size; their border is smaller.
    let border = 1.0 / scale;
    let point = map
        * Point::new(
            f64::from(draw.rect[0] - border),
            f64::from(draw.rect[1] - border),
        );
    draw.rect = [
        point.x as f32,
        point.y as f32,
        draw.rect[2] + 2.0 * border,
        draw.rect[3] + 2.0 * border,
    ];
    // The CPU shelf reserves and clears this border for every glyph.
    let uv_border = 1.0 / puzzle_text::raster::PAGE as f32;
    for axis in 0..2 {
        draw.uv[axis] -= uv_border;
        draw.uv[axis + 2] += uv_border;
    }
    draw.tint = index;
    draw.flags |= puzzle_frame::raster::RASTER_EFFECT;
    if recolor {
        draw.flags |= puzzle_frame::raster::RASTER_RECOLOR;
    }
    Ok(())
}
