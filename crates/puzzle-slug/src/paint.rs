//! COLR/overlay brush records and their CPU color oracle (design §9.5–§9.6).
//!
//! One 256-texel row describes a brush in root em space and any enclosing
//! Slug clips. Stop colors are premultiplied sRGB, as OpenType COLR requires.
//! Solid unconstrained fills bypass this table and use the instance color.
use crate::ShapeHandle;
/// Color-line extension outside its normalized interval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Extend {
    /// Clamp at the first and last colors.
    Pad,
    /// Repeat the color interval with period one.
    Repeat,
    /// Alternate forward and backward intervals with period two.
    Reflect,
}
/// A normalized color-line position and premultiplied sRGB color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    /// Position supplied by skrifa after color-line normalization.
    pub offset: f32,
    /// Premultiplied sRGB with independently interpolated alpha.
    pub color: [f32; 4],
    /// Only foreground-index stops inherit the instance text color.
    pub foreground: bool,
}
/// Geometry in brush-local em space; affine maps are stored separately.
#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    /// Constant premultiplied sRGB color.
    Solid([f32; 4]),
    /// Effective two-point gradient after skrifa resolves the third point.
    Linear {
        /// Start point of the normalized color line.
        p0: [f32; 2],
        /// End point of the normalized color line.
        p1: [f32; 2],
    },
    /// Moving circles whose envelope defines the radial color line.
    Radial {
        /// Circle at offset zero.
        c0: [f32; 2],
        /// Circle at offset one.
        c1: [f32; 2],
        /// Radii at zero and one; negative radii have no ink.
        radii: [f32; 2],
    },
    /// Angular color line, in clockwise degrees from the positive x axis.
    Sweep {
        /// Center of rotation.
        center: [f32; 2],
        /// Start and end angles, possibly outside one turn.
        angles: [f32; 2],
    },
}
/// A brush plus its coordinate map and enclosing clip scopes.
#[derive(Clone, Debug, PartialEq)]
pub struct Paint {
    /// Closed brush geometry variant used by CPU and shader.
    pub brush: Brush,
    /// A solid foreground brush samples the instance color dynamically.
    pub foreground: bool,
    /// Root em coordinates to brush-local em coordinates, column-major affine.
    pub inverse: [f32; 6],
    /// Outside-interval behavior, used only by gradient brushes.
    pub extend: Extend,
    /// Sorted stops, limited to sixteen before a record can be allocated.
    pub stops: Vec<Stop>,
    /// Additional root-em outlines; the instance draws the innermost clip.
    pub clips: Vec<ShapeHandle>,
}
/// Row encoding shared with `shaders/paint.wgsl`.
mod encoding;
/// Scalar geometry and premultiplied color-line evaluation.
mod sample;
/// Bounded generational CPU mirror with dirty rows and recent-use eviction.
pub mod table;
impl Paint {
    /// Evaluates premultiplied color at root em coordinates, excluding clips.
    pub fn sample(&self, at: [f32; 2]) -> [f32; 4] {
        sample::evaluate(self, at)
    }
    /// Serializes a validated paint into its fixed 256-texel GPU row.
    pub fn texels(&self) -> Result<[[f32; 4]; 256], String> {
        encoding::encode(self)
    }
}
#[cfg(test)]
mod tests;
