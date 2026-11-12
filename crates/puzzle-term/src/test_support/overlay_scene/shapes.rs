//! Public-provider SVG, cap/join/dash and guarded dense-shape review scene.
//!
//! Paths are fixture-owned geometry, generated locally with the approved kurbo.
//! No GPU instances or atlas addresses are fabricated by this provider. A single
//! translucent dense plot exposes both gaps and double-blended overlap seams.
//!
//! The first specimen has an oppositely wound inner contour. It verifies that
//! SVG holes survive preparation and do not become filled rectangles at cuts.
//! Its curved roof also requires quadratic coverage under a small rotation.
//! The cyan/orange color line checks that baked physical chunks still evaluate
//! the same logical brush rather than restarting the gradient for each piece.
//!
//! Three zigzags share a centerline and dash pattern. Round caps and joins show
//! curved expansion; butt caps with bevel joins show clipped corners; square
//! caps with miter joins show extension beyond the centerline endpoints.
//! A square cap extends by half the stroke width. With this particular short
//! gap, neighbouring square dashes touch, unlike the butt-cap specimen.
//!
//! The last centerline deliberately contains thousands of short segments.
//! Its shape needs density subdivision even where the bounds already fit.
//! A translucent color makes overlapping ownership show as darker stripes;
//! missing guarded geometry instead appears as breaks in the continuous curve.
//! The scenario checks ink area independently and retains both DPR images for
//! human seam review; the provider never declares its own pixels acceptable.
//! Both DPR variants use identical source paths and brushes before physical cuts.
use crate::overlay::{
    anchor::Anchor,
    draw::{DrawList, Paint, Primitive},
    geometry::Rect,
    model::ViewState,
    view::{OverlayView, ViewCtx},
};
use kurbo::{Affine, BezPath, Cap, Join, Stroke, StrokeOpts};
use puzzle_slug::{
    ShapeKey,
    paint::{Brush, Extend, Stop},
};
use std::sync::Arc;

/// Immutable fixture source outlines, cached by the ordinary Session provider API.
pub(super) struct Scene {
    /// SVG fill and several fully expanded strokes.
    paths: Vec<BezPath>,
    /// Existing Slug paint-table brush, in the SVG's original coordinate space.
    gradient: Arc<puzzle_slug::paint::Paint>,
}

/// Cold fixture generation is separate from dirty or animated provider building.
impl Scene {
    /// Builds a self-contained shape specimen without fonts or external assets.
    pub(super) fn new() -> Result<Self, String> {
        // Parsing exercises the extension's SVG surface rather than a private atlas.
        let svg =
            BezPath::from_svg("M0 35 Q40 -25 80 35 L68 75 L12 75 Z M25 35 L25 52 L55 52 L55 35 Z")
                .map_err(|e| e.to_string())?;
        let mut paths = vec![svg];
        // These pairs span each supported cap and join without duplicating geometry.
        for (cap, join) in [
            (Cap::Round, Join::Round),
            (Cap::Butt, Join::Bevel),
            (Cap::Square, Join::Miter),
        ] {
            // Sharp bends expose the different join rules before dash expansion.
            let mut source = BezPath::new();
            source.move_to((0.0, 45.0));
            source.line_to((30.0, 0.0));
            source.line_to((60.0, 45.0));
            source.line_to((95.0, 10.0));
            let stroke = Stroke::new(7.0)
                .with_caps(cap)
                .with_join(join)
                .with_dashes(0.0, [13.0, 6.0]);
            // Expand once at cold creation. Chunking receives the complete outline,
            // including round joins, so internal partitions never invent new caps.
            paths.push(kurbo::stroke(
                source.iter(),
                &stroke,
                &StrokeOpts::default(),
                0.0005,
            ));
        }
        let mut source = BezPath::new();
        // Uniform sample spacing makes the plot dense along its entire visible span.
        source.move_to((0.0, 50.0));
        for index in 1..5000 {
            let x = index as f64 / 9.0;
            source.line_to((x, 50.0 + (x * 0.11).sin() * 35.0));
        }
        paths.push(kurbo::stroke(
            source.iter(),
            &Stroke::new(3.0)
                .with_caps(Cap::Round)
                .with_join(Join::Round),
            &StrokeOpts::default(),
            0.0005,
        ));
        // Sharing one immutable brush lets both DPR builds reuse the logical source.
        let gradient = gradient();
        Ok(Self { paths, gradient })
    }
}

/// The fixture consumes only the public provider surface used by real extensions.
impl OverlayView for Scene {
    /// Returns immutable paths; the shared context performs all GPU preparation.
    fn build(&mut self, _state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        // Fixed viewport coordinates keep specimen placement independent of VT text.
        let anchor = Anchor::Viewport(Rect::default());
        for (index, source) in self.paths.iter().enumerate() {
            // A provider owns its source keys; the shared atlas assigns global IDs.
            // The closure clones only on a cold cache miss, never on a dirty hit.
            let shape = match ctx
                .shapes
                .get_or_prepare(ShapeKey(index as u64), || source.clone())
            {
                Ok(shape) => shape,
                Err(error) => {
                    // A rejected extension path is reported without aborting the frame.
                    tracing::warn!(event = "overlay.fixture_path", %error);
                    continue;
                }
            };
            let (transform, color) = match index {
                // The fill rotates; the caps/joins stay aligned for direct comparison.
                0 => (
                    Affine::translate((60.0, 45.0)) * Affine::rotate(0.12),
                    0xffffffff,
                ),
                1..=3 => (
                    Affine::translate((180.0 + (index - 1) as f64 * 130.0, 55.0)),
                    0xffeecc88,
                ),
                _ => (Affine::translate((40.0, 190.0)), 0x9988dd33),
            };
            // Top permits the specimen to use terminal padding without a grid clip.
            out.push(
                anchor,
                puzzle_frame::ZSlot::Top,
                Primitive::Shape(
                    shape,
                    transform,
                    Paint {
                        color,
                        gradient: (index == 0).then(|| self.gradient.clone()),
                    },
                ),
            );
        }
    }
}

/// Paint-table color line is authored in the SVG’s logical coordinate system.
fn gradient() -> Arc<puzzle_slug::paint::Paint> {
    // Root SVG units define both endpoints; the frame maps the inverse under DPR.
    Arc::new(puzzle_slug::paint::Paint {
        brush: Brush::Linear {
            p0: [0.0; 2],
            p1: [80.0, 75.0],
        },
        // Colors come from these stops rather than the terminal foreground token.
        foreground: false,
        // Identity maps root coordinates into the authored linear brush initially.
        inverse: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        extend: Extend::Pad,
        // Opaque premultiplied stops isolate gradient mapping from plot transparency.
        stops: vec![
            Stop {
                offset: 0.0,
                color: [0.0, 0.8, 1.0, 1.0],
                foreground: false,
            },
            Stop {
                offset: 1.0,
                color: [1.0, 0.3, 0.0, 1.0],
                foreground: false,
            },
        ],
        // The SVG's inner winding supplies its hole without separate clip handles.
        clips: Vec::new(),
    })
}
