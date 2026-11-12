//! Directed quadratic contour output for the guarded half-plane clipper.
//!
//! Every surviving interval retains its original direction. Connections across
//! omitted excursions lie along the cutting plane and preserve winding, including
//! holes. They remain outside the owned pixel rectangle because of the AA guard.
//! Direct output keeps already rounded texture controls instead of reconstructing
//! and parsing a BezPath at every subdivision depth. Tight extrema bounds are
//! intersected with each half-plane before band selection; untouched curves
//! require no repeated extrema solution during recursive subdivision.
use crate::shape::{QuadCurve, QuadShape, ShapeError};

/// One contour is open at a time; completed ranges index the single curve owner.
#[derive(Default)]
pub(super) struct Output {
    /// Preserved source intervals and new plane-parallel connection edges.
    curves: Vec<QuadCurve>,
    /// Separate closed rings retain independent outer/hole winding.
    contours: Vec<std::ops::Range<usize>>,
    /// First retained point, absent if the contour lies entirely outside.
    start: Option<[f32; 2]>,
    /// Most recent retained endpoint, used to connect disjoint intervals.
    previous: [f32; 2],
    /// Beginning of the currently open ring in the curve array.
    first: usize,
}

/// Keeps Slug's straight-edge encoding and drops only collapsed points.
impl Output {
    /// Appends a directed interval, joining the preceding omitted excursion.
    pub(super) fn push(&mut self, curve: QuadCurve) {
        if self.start.is_none() {
            // An empty clipped contour never acquires a range or artificial edge.
            self.start = Some(curve.p1);
            self.previous = curve.p1;
            self.first = self.curves.len();
        }
        self.line(curve.p1);
        if curve.p1 != curve.p2 || curve.p2 != curve.p3 {
            self.curves.push(curve);
        }
        self.previous = curve.p3;
    }

    /// Encodes a plane connection as endpoint-repeated quadratic controls.
    fn line(&mut self, point: [f32; 2]) {
        if self.previous != point {
            // Exactly parallel connections cannot affect the parallel ray direction.
            self.curves.push(QuadCurve {
                p1: self.previous,
                p2: point,
                p3: point,
            });
            self.previous = point;
        }
    }

    /// Connects the last exit to the first entry before starting another ring.
    pub(super) fn close(&mut self) {
        if let Some(start) = self.start.take() {
            self.line(start);
            if self.curves.len() > self.first {
                // Do not publish move-only or completely collapsed contours.
                self.contours.push(self.first..self.curves.len());
            }
        }
    }

    /// Publishes closed finite controls with conservatively intersected source bounds.
    pub(super) fn finish(self, bbox: [f32; 4]) -> Result<QuadShape, ShapeError> {
        // A line-only intersection cannot shade area or supply a valid band span.
        if self.curves.is_empty() || bbox[0] >= bbox[2] || bbox[1] >= bbox[3] {
            return Err(ShapeError::Empty);
        }
        Ok(QuadShape {
            curves: self.curves,
            contours: self.contours,
            bbox,
        })
    }
}
