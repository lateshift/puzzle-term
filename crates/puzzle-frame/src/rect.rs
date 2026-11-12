//! Pixel-area instance for solid and rounded terminal rectangles (§10.2).
//!
//! One quad can cover a run of equal-background cells or a cursor shape.
//! Its geometry stays in device pixels; the renderer's orthographic matrix
//! maps those coordinates into clip space without per-cell CPU vertices.
use bytemuck::{Pod, Zeroable};

/// Thirty-two bytes consumed by the rect vertex shader.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct RectInstance {
    /// Left, top, width and height in device pixels.
    pub rect: [f32; 4],
    /// Straight sRGB RGBA8, with alpha in the high byte.
    pub color: u32,
    /// Radius in device pixels; zero gives a sharp pixel-aligned edge.
    pub corner_radius: f32,
    /// Inward stroke width; zero fills the whole rounded shape.
    pub stroke_width: f32,
    /// Reserved for per-rectangle rendering options.
    pub flags: u32,
}

#[cfg(test)]
/// The GPU reads these offsets directly, so size drift is a shader bug.
mod tests;
