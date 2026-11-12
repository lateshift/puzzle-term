//! CPU oracle for SlugRender over the atlas mirrors (design §9.6).
//!
//! SPDX-License-Identifier: MIT OR Apache-2.0
//! Copyright 2017, by Eric Lengyel.
//! Ported from Eric Lengyel's SlugPixelShader.hlsl at commit
//! be3c13eb7d63f9e8aa5c583e42d92c374cb91d98. The HLSL comments and
//! formulas live beside their matching Rust functions in `poly`, `rays`
//! and `coverage`. Only fwidth changes: a known affine pixel-to-em map gives
//! its analytic derivatives without GPU derivative instructions.
//!
//! The oracle reads the exact CPU mirrors intended for M3's GPU upload.
//! It therefore tests geometry conversion, band assignment and packed
//! addressing together with the shader math, rather than substituting an
//! independent path rasterizer that could hide a shared packing error.
//! A stale handle or bad transform returns an error before texture access.
//! Device pixels are sampled at their centres; the known transform gives
//! the em width of that pixel for the reference's half-pixel windows.
/// Reference coverage combination and fill-rule flags.
mod coverage;
/// Reference sign-code and quadratic root solvers.
mod poly;
/// Reference texture fetches and horizontal/vertical ray loops.
mod rays;

use crate::atlas::{ShapeHandle, SlugAtlas};
use coverage::calc_coverage;
use rays::{horizontal, vertical};

/// A non-invertible transform or bad packed address fails without a panic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceError {
    /// The handle points at an atlas generation already rebuilt or reset.
    StaleHandle,
    /// Coordinates, scales or dimensions are invalid for a sample.
    InvalidSample,
    /// A stored texture address does not fit the CPU mirror.
    PackedAddress,
}

/// Reports an oracle error to tests or debugging tools.
impl std::fmt::Display for ReferenceError {
    /// Uses stable wording independent of any platform renderer.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::StaleHandle => "stale Slug shape handle",
                Self::InvalidSample => "invalid Slug sample or affine scale",
                Self::PackedAddress => "invalid Slug packed texture address",
            }
        )
    }
}

/// Standard error contract for CPU coverage queries.
impl std::error::Error for ReferenceError {}

/// Axis-aligned em-to-device transform for an oracle image.
#[derive(Clone, Copy, Debug)]
pub struct RasterTransform {
    /// Device pixels per em; a negative y scale flips font y-up to image y-down.
    pub scale: [f32; 2],
    /// Device-pixel position of the em-space origin.
    pub translate: [f32; 2],
}

/// Top-down opaque RGBA8 pixels with coverage shown as grayscale.
#[derive(Clone, Debug)]
pub struct OracleImage {
    /// Width in device pixels.
    pub width: u32,
    /// Height in device pixels.
    pub height: u32,
    /// Four bytes per pixel; white means full coverage and alpha is opaque.
    pub pixels: Vec<u8>,
}

/// Evaluates SlugRender at one em-space sample using analytic fwidth.
///
/// `ems_per_pixel` is the absolute derivative of sample coordinates with
/// respect to a one-pixel move along each axis. The reference multiplies
/// by its reciprocal before the ±0.5 pixel coverage windows.
pub fn sample(
    atlas: &SlugAtlas,
    handle: ShapeHandle,
    at: [f32; 2],
    ems_per_pixel: [f32; 2],
) -> Result<f32, ReferenceError> {
    if !atlas.contains_handle(handle) {
        return Err(ReferenceError::StaleHandle);
    }
    if at.into_iter().chain(ems_per_pixel).any(|v| !v.is_finite())
        || ems_per_pixel.into_iter().any(|v| v <= 0.0)
    {
        return Err(ReferenceError::InvalidSample);
    }
    // This is the analytic counterpart of fwidth(renderCoord), the first
    // statement of reference SlugRender before any divergent ray loop.
    let pixels_per_em = ems_per_pixel.map(|v| 1.0 / v);
    let bbox = handle.bbox;
    let spans = [bbox[2] - bbox[0], bbox[3] - bbox[1]];
    if spans.into_iter().any(|v| v <= 0.0 || !v.is_finite()) {
        return Err(ReferenceError::InvalidSample);
    }
    let (x, y, flags) = crate::encoding::decode_band_info(handle.band_info);
    let max_x = usize::from(x);
    let max_y = usize::from(y);
    // The vertex stage supplies band scale and offset from the em-space
    // bbox. Keep their multiplication order: `(at - min) * scale` is only
    // mathematically equivalent and can choose a different band at an edge.
    let band_scale = [(max_x + 1) as f32 / spans[0], (max_y + 1) as f32 / spans[1]];
    let band_offset = [-bbox[0] * band_scale[0], -bbox[1] * band_scale[1]];
    // HLSL truncates float to int before clamping to the packed maxima.
    let index_x =
        ((at[0] * band_scale[0] + band_offset[0]) as isize).clamp(0, max_x as isize) as usize;
    let index_y =
        ((at[1] * band_scale[1] + band_offset[1]) as isize).clamp(0, max_y as isize) as usize;
    let (xcov, xwgt) = horizontal(atlas, handle, index_y, at, pixels_per_em)?;
    let (ycov, ywgt) = vertical(atlas, handle, index_x, at, pixels_per_em)?;
    // The decoder supplies the same E/W flag byte that WGSL reads from glyph.w.
    Ok(calc_coverage(xcov, ycov, xwgt, ywgt, flags))
}

/// Produces a grayscale oracle PNG source without a GPU or font asset.
pub fn rasterize(
    atlas: &SlugAtlas,
    handle: ShapeHandle,
    width: u32,
    height: u32,
    transform: RasterTransform,
) -> Result<OracleImage, ReferenceError> {
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || transform
            .scale
            .into_iter()
            .any(|v| v == 0.0 || !v.is_finite())
        || transform.translate.into_iter().any(|v| !v.is_finite())
    {
        return Err(ReferenceError::InvalidSample);
    }
    let ems_per_pixel = transform.scale.map(|v| 1.0 / v.abs());
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            // Pixel centers map back through the known affine transform;
            // negative y scale produces a conventional top-down image.
            let at = [
                (x as f32 + 0.5 - transform.translate[0]) / transform.scale[0],
                (y as f32 + 0.5 - transform.translate[1]) / transform.scale[1],
            ];
            let coverage = sample(atlas, handle, at, ems_per_pixel)?;
            let byte = (coverage * 255.0).round().clamp(0.0, 255.0) as u8;
            pixels.extend_from_slice(&[byte, byte, byte, 255]);
        }
    }
    Ok(OracleImage {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
/// Analytic rectangle, rotational and winding-rule truth checks.
mod tests;
