//! Kurbo-built outlines available before fonts are vendored (M2, §9.6).
//!
//! The same paths drive property tests, the oracle PNG and the native
//! preparation benchmark. They contain no third-party font or image asset.
//!
//! The fixtures isolate different geometry claims. The square and fourfold
//! circle expose any x/y or quarter-turn bias in the two ray directions.
//! Duplicate same-direction contours make winding two in their overlap,
//! distinguishing nonzero from even-odd fill without relying on rasterized
//! ground truth. The curved outline exercises kurbo cubic conversion.
//!
//! A path here is measured in em space, with y increasing upward. The real
//! font probe uses the same coordinate convention, so tests and specimens
//! exercise one packing and sampling path rather than parallel algorithms.
use kurbo::BezPath;

/// Named outlines accepted by the developer-only slug dump command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntheticShape {
    /// Straight-edged rectangle with fractional pixel tests.
    Rectangle,
    /// Four-quadratic circle for rotational-symmetry tests.
    Circle,
    /// Nested contours in the same winding direction.
    DoubleWinding,
    /// Curved, multi-contour outline representative of a text glyph.
    TypicalGlyph,
}

/// CLI name mapping remains outside the geometry hot path.
impl SyntheticShape {
    /// Parses the small known set of reproducible built-in outlines.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "rectangle" => Some(Self::Rectangle),
            "circle" => Some(Self::Circle),
            "double-winding" => Some(Self::DoubleWinding),
            "typical-glyph" => Some(Self::TypicalGlyph),
            _ => None,
        }
    }

    /// Returns a fresh kurbo path in em-space coordinates.
    #[must_use]
    pub fn path(self) -> BezPath {
        match self {
            Self::Rectangle => rectangle(0.12, 0.12, 0.88, 0.88),
            Self::Circle => symmetric_circle(),
            Self::DoubleWinding => {
                // Same-direction inner and outer contours have winding two
                // at the center, separating nonzero from even-odd coverage.
                // The outer-only ring stays filled under both rules, giving
                // the test a second control point outside the overlap.
                let mut path = rectangle(0.05, 0.05, 0.95, 0.95);
                path.extend(rectangle(0.28, 0.28, 0.72, 0.72).iter());
                path
            }
            Self::TypicalGlyph => typical_glyph(),
        }
    }
}

/// Uses kurbo's path representation for a closed, anticlockwise rectangle.
#[must_use]
pub fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((x0, y0));
    path.line_to((x1, y0));
    path.line_to((x1, y1));
    path.line_to((x0, y1));
    path.close_path();
    path
}

/// Draws a small curved glyph-like outline with an interior counter.
fn typical_glyph() -> BezPath {
    let mut path = BezPath::new();
    // Curved outside: cubic subdivision is exercised during preparation.
    // Unequal control points avoid accidentally passing a symmetry-specific
    // sampler while its general root solving is broken.
    path.move_to((0.10, 0.08));
    path.curve_to((0.05, 0.35), (0.06, 0.73), (0.30, 0.87));
    path.curve_to((0.52, 1.01), (0.82, 0.88), (0.90, 0.65));
    path.curve_to((0.98, 0.40), (0.88, 0.10), (0.66, 0.08));
    path.line_to((0.10, 0.08));
    path.close_path();
    // The inner contour runs in reverse so nonzero winding makes a hole.
    // Its tighter curves exercise bins where multiple arcs share a band.
    path.move_to((0.35, 0.31));
    path.curve_to((0.56, 0.25), (0.76, 0.40), (0.70, 0.61));
    path.curve_to((0.64, 0.78), (0.40, 0.78), (0.32, 0.62));
    path.curve_to((0.27, 0.51), (0.27, 0.37), (0.35, 0.31));
    path.close_path();
    path
}

/// Four rotated quadratic arcs keep the oracle symmetry fixture exact.
fn symmetric_circle() -> BezPath {
    let center = 0.5;
    let radius = 0.42;
    // This control distance places each quadratic's midpoint on the true
    // circle. Repeating it by quarter-turn removes orientation bias from
    // the fixture while the conversion tests exercise kurbo's cubic path.
    // Each arc has exactly the rotated control geometry of its neighbour;
    // unequal coverage after a quarter-turn therefore implicates packing
    // or ray combination rather than a different curve approximation.
    let control = radius * (2.0_f64.sqrt() - 0.5);
    let mut path = BezPath::new();
    path.move_to((center + radius, center));
    path.quad_to(
        (center + control, center + control),
        (center, center + radius),
    );
    path.quad_to(
        (center - control, center + control),
        (center - radius, center),
    );
    path.quad_to(
        (center - control, center - control),
        (center, center - radius),
    );
    path.quad_to(
        (center + control, center - control),
        (center + radius, center),
    );
    path.close_path();
    path
}
