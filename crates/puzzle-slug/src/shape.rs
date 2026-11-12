//! Converts kurbo paths to closed quadratic contours (design §8.5, §9.3).
//!
//! All coordinates are em-space and y-up. Straight segments become
//! `{p1, p2, p2}`, as the Slug reference README prescribes. Kurbo handles
//! cubic subdivision; this module preserves contour endpoints and drops only
//! points that contribute no geometry.
//!
//! A TrueType outline already consists of quadratics; a CFF outline may
//! contain cubics. Slug's pixel shader solves only quadratic roots, so the
//! cubic approximation must happen before band selection. The 1/2048-em
//! tolerance is independent of font size and DPR, matching the reusable
//! atlas data in design §9.8. Kurbo owns the subdivision algorithm (D-2).
//!
//! Curves retain their directed contour order because winding depends on
//! direction. The endpoint of one curve is the start of the next. Separate
//! contour ranges let the packer share endpoints within a contour without
//! accidentally joining a glyph's outer boundary to a hole.
//!
//! The bbox uses actual quadratic extrema from kurbo, reducing the quad
//! drawn around a glyph. Bands instead use the conservative control hull:
//! excluding a curve that bends into a band would make the shader miss an
//! edge, while a few extra curves only cost traversal time.
//!
//! External fonts are untrusted. Nonfinite and out-of-f32-range coordinates
//! cannot be encoded in the curve texture, and invalid tolerance could make
//! approximation diverge. These cases return errors before packing.
use kurbo::{BezPath, CubicBez, ParamCurveExtrema, PathEl, Point, QuadBez};
use std::ops::Range;

/// Maximum em-space cubic approximation error used for font outlines.
pub const DEFAULT_TOLERANCE: f64 = 1.0 / 2048.0;

/// One Slug quadratic with coordinates rounded to texture f32 precision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadCurve {
    /// Starting point of the directed contour segment.
    pub p1: [f32; 2],
    /// Control point, or the endpoint for a straight line.
    pub p2: [f32; 2],
    /// Ending point; the next contour segment starts here.
    pub p3: [f32; 2],
}

/// Conservative control-point ranges used when assigning ray bands.
impl QuadCurve {
    /// Minimum and maximum control coordinates on the chosen axis.
    #[must_use]
    pub fn control_range(&self, axis: usize) -> (f32, f32) {
        let values = [self.p1[axis], self.p2[axis], self.p3[axis]];
        (
            values[0].min(values[1]).min(values[2]),
            values[0].max(values[1]).max(values[2]),
        )
    }

    /// Parallel straight lines cannot cross a ray and waste band space.
    #[must_use]
    pub fn flat_on(&self, axis: usize) -> bool {
        self.p1[axis] == self.p2[axis] && self.p2[axis] == self.p3[axis]
    }
}

/// Closed contours stored as ranges into one contiguous curve array.
#[derive(Clone, Debug)]
pub struct QuadShape {
    /// All contours in their original winding directions.
    pub curves: Vec<QuadCurve>,
    /// Each range delimits one contour for endpoint sharing in C1 packing.
    pub contours: Vec<Range<usize>>,
    /// Tight quadratic bounds in em space: min x/y followed by max x/y.
    pub bbox: [f32; 4],
}

/// Invalid or empty paths are reports, never panics from font input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeError {
    /// A command appeared before a contour's MoveTo.
    MissingMove,
    /// NaN, infinity or an f64 value outside f32 texture range appeared.
    NonFinite,
    /// A nonpositive or nonfinite subdivision tolerance was requested.
    InvalidTolerance,
    /// Every segment collapsed to a point.
    Empty,
}

/// Human-readable input errors for font and overlay callers.
impl std::fmt::Display for ShapeError {
    /// Names the path condition the caller should ignore or report.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::MissingMove => "path segment before MoveTo",
                Self::NonFinite => "path has a nonfinite or out-of-range coordinate",
                Self::InvalidTolerance => "cubic tolerance must be finite and positive",
                Self::Empty => "path contains no drawable curves",
            }
        )
    }
}

/// Standard error contract for path conversion.
impl std::error::Error for ShapeError {}

/// Builder state for one path conversion, kept private to prevent open output.
struct Builder {
    /// Converted curves, in input contour order.
    curves: Vec<QuadCurve>,
    /// Completed contour ranges in that array.
    contours: Vec<Range<usize>>,
    /// First point of the current contour, if one is open.
    start: Option<Point>,
    /// Current endpoint used to close paths that omit ClosePath.
    current: Point,
    /// Curve index where the current contour began.
    first: usize,
}

/// Input conversion with explicit closure and a kurbo cubic approximation.
impl QuadShape {
    /// Converts a kurbo path at the requested maximum em-space error.
    ///
    /// A path may leave a contour open; Slug requires a closed outline for
    /// winding, so the builder adds its final straight edge when necessary.
    pub fn from_path(path: &BezPath, tolerance: f64) -> Result<Self, ShapeError> {
        if !tolerance.is_finite() || tolerance <= 0.0 {
            return Err(ShapeError::InvalidTolerance);
        }
        let mut builder = Builder {
            curves: Vec::new(),
            contours: Vec::new(),
            start: None,
            current: Point::ORIGIN,
            first: 0,
        };
        for element in path.iter() {
            builder.push(element, tolerance)?;
        }
        builder.close()?;
        // Reject a move-only or fully collapsed font outline. Empty space
        // glyphs are handled by the text probe without an atlas entry.
        if builder.curves.is_empty() {
            return Err(ShapeError::Empty);
        }
        Self::from_curves(builder.curves, builder.contours)
    }

    /// Derives tight extrema bounds after internal finite quadratic clipping.
    pub(crate) fn from_curves(
        curves: Vec<QuadCurve>,
        contours: Vec<Range<usize>>,
    ) -> Result<Self, ShapeError> {
        if curves.is_empty() {
            return Err(ShapeError::Empty);
        }
        // Kurbo's extrema solver gives the actual outline bounds, not the
        // larger control hull used conservatively for band membership.
        let mut bbox = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for curve in &curves {
            let quad = QuadBez::new(to_point(curve.p1), to_point(curve.p2), to_point(curve.p3));
            let bounds = quad.bounding_box();
            bbox[0] = bbox[0].min(bounds.x0);
            bbox[1] = bbox[1].min(bounds.y0);
            bbox[2] = bbox[2].max(bounds.x1);
            bbox[3] = bbox[3].max(bounds.y1);
        }
        Ok(Self {
            curves,
            contours,
            bbox: bbox.map(|coordinate| coordinate as f32),
        })
    }
}

/// Contour operations preserve endpoint continuity for shared texels.
impl Builder {
    /// Consumes one kurbo instruction without accepting malformed starts.
    ///
    /// `current` advances after every segment, whereas `start` remains the
    /// first point of the contour until ClosePath. Confusing them breaks
    /// connected quadratics even if isolated curve tests still pass.
    fn push(&mut self, element: PathEl, tolerance: f64) -> Result<(), ShapeError> {
        match element {
            PathEl::MoveTo(point) => {
                // A new contour implicitly closes the previous one, which
                // some outline producers omit as an explicit command.
                self.close()?;
                self.start = Some(point);
                self.current = point;
                self.first = self.curves.len();
                checked(point)?;
            }
            PathEl::LineTo(point) => self.line_to(point)?,
            PathEl::QuadTo(control, point) => {
                let start = self.start.ok_or(ShapeError::MissingMove)?;
                checked(start)?;
                checked(control)?;
                checked(point)?;
                // Preserve the control point; the root solver expects the
                // original quadratic, not a flattened approximation.
                self.add(self.current, control, point);
                self.current = point;
                self.start = Some(start);
            }
            PathEl::CurveTo(a, b, point) => {
                let start = self.start.ok_or(ShapeError::MissingMove)?;
                checked(start)?;
                checked(a)?;
                checked(b)?;
                checked(point)?;
                // Kurbo owns cubic subdivision; no duplicate geometry
                // algorithm can diverge from its proven tolerance rule.
                // Its yielded quadratics are already sequential, allowing
                // the texture packer to share their boundary endpoints.
                for (_, _, quad) in CubicBez::new(self.current, a, b, point).to_quads(tolerance) {
                    self.add(quad.p0, quad.p1, quad.p2);
                }
                self.current = point;
            }
            PathEl::ClosePath => self.close()?,
        }
        Ok(())
    }

    /// Encodes a straight line by repeating its endpoint, per the README.
    fn line_to(&mut self, point: Point) -> Result<(), ShapeError> {
        self.start.ok_or(ShapeError::MissingMove)?;
        checked(point)?;
        self.add(self.current, point, point);
        self.current = point;
        Ok(())
    }

    /// Completes an open contour even when the source omitted ClosePath.
    ///
    /// The closing segment is omitted when the last endpoint already equals
    /// the first; adding a zero-length line would only waste a curve slot.
    fn close(&mut self) -> Result<(), ShapeError> {
        if let Some(start) = self.start.take() {
            if self.current != start {
                self.line_without_start(start)?;
            }
            if self.first < self.curves.len() {
                self.contours.push(self.first..self.curves.len());
            }
            self.first = self.curves.len();
        }
        Ok(())
    }

    /// Appends the closing line after the start marker has been consumed.
    fn line_without_start(&mut self, end: Point) -> Result<(), ShapeError> {
        checked(end)?;
        self.add(self.current, end, end);
        self.current = end;
        Ok(())
    }

    /// Drops only a segment whose three points collapse to one position.
    ///
    /// A line with distinct endpoints is not degenerate even though p2=p3.
    /// Keeping it preserves the reference's straight-edge encoding.
    fn add(&mut self, p1: Point, p2: Point, p3: Point) {
        if p1 == p2 && p2 == p3 {
            return;
        }
        self.curves.push(QuadCurve {
            p1: [p1.x as f32, p1.y as f32],
            p2: [p2.x as f32, p2.y as f32],
            p3: [p3.x as f32, p3.y as f32],
        });
    }
}

/// Rejects coordinates that a finite f32 curve texture cannot represent.
fn checked(point: Point) -> Result<(), ShapeError> {
    if [point.x, point.y]
        .into_iter()
        .all(|v| v.is_finite() && v.abs() <= f64::from(f32::MAX))
    {
        Ok(())
    } else {
        Err(ShapeError::NonFinite)
    }
}

/// Converts stored f32 texture coordinates back for kurbo's extrema solver.
fn to_point(point: [f32; 2]) -> Point {
    Point::new(f64::from(point[0]), f64::from(point[1]))
}

#[cfg(test)]
/// Conversion tests cover closure, cubic tolerance and invalid coordinates.
mod tests;
