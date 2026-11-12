//! Per-outline data consumed by the M3 instanced Slug vertex shader (§9.5).
//!
//! The first ten floats map em coordinates to logical pixels and bound the
//! original outline quad. Four packed words carry atlas locations, flags,
//! color and a future paint-table entry. Keeping exactly 56 bytes makes
//! instance upload independent of the shape's number of curves.
use bytemuck::{Pod, Zeroable};

/// The 56-byte Slug instance described in design §9.5.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SlugInstance {
    /// Column-major 2×2 em-to-object linear map (scale, shear and rotation).
    pub m: [f32; 4],
    /// Object-space position of the em origin in logical pixels.
    pub t: [f32; 2],
    /// Em-space bounds: minimum x/y followed by maximum x/y.
    pub bbox: [f32; 4],
    /// Band header location: x in low 16 bits, y in high 16 bits.
    pub glyph_loc: u32,
    /// Band maxima and E/W flags in the reference's bit positions.
    pub band_info: u32,
    /// Straight, sRGB-encoded RGBA8 color, premultiplied by the shader.
    pub color: u32,
    /// Zero selects the solid color; other entries select a later paint row.
    pub paint: u32,
}

#[cfg(test)]
/// Layout tests protect the shader's byte-for-byte vertex attribute offsets.
mod tests;
