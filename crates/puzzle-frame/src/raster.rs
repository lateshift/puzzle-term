//! Atlas instance shared by bitmap fallback and later image producers (§10.2).
//!
//! The atlas is only a texture source: glyph origins and cell advances come
//! from the backend-neutral placement above it (design §8.9, D-28).
use bytemuck::{Pod, Zeroable};

/// Forty-byte textured quad for a mask or a color atlas page.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct RasterInstance {
    /// Left/top and width/height in device pixels. Effects transform the origin
    /// on the CPU and relative corners through the paint-row matrix on the GPU.
    pub rect: [f32; 4],
    /// Minimum and maximum normalized atlas texture coordinates.
    pub uv: [f32; 4],
    /// RGBA8 tint normally; with EFFECT, a shared paint-table first-texel-plus-one.
    pub tint: u32,
    /// Bit zero selects color, bit one affine/bilinear sampling, bit two recoloring.
    /// Bit three selects independent image sampling without a glyph effect matrix.
    /// Bits 8–15 select the atlas array layer; bits 16–31 select its draw incarnation.
    pub flags: u32,
}

#[cfg(test)]
/// A mask producer can rely on one fixed vertex stride.
mod tests;

/// Affine bitmap-only ink fetches its forward matrix and premultiplied tint row.
pub const RASTER_EFFECT: u32 = 2;
/// Explicit color replacement uses filtered coverage even for authored RGBA.
pub const RASTER_RECOLOR: u32 = 4;
