//! Each public grid effect has a separate labeled reference row (§11.2).
//!
//! Source cells and labels remain ordinary VT text. The provider selects only
//! the three B cells following each label; transforms pivot at their cell tops.
//! Two blank rows between samples expose magnified and rotated ink overhang.
//! Hidden ink is contrasted with its intact label and still-readable buffer.
//! Both DPRs use the same logical effect descriptors; the host supplies scale.
//! Labels occupy columns zero through eleven independently of primary glyph metrics.
//! B has both hinted and unhinted outlines; effects require the unhinted variant.
//! Magnification and rotation have separate samples with distinct pixel footprints.
//! Opacity fades coverage while leaving ordinary cell backgrounds unchanged.
//! The final hidden span must contribute zero ink while its label remains visible.
//! BufferView still reports hidden B cells because an effect never erases content.
use crate::overlay::{
    anchor::Anchor,
    draw::{DrawList, Effect, Primitive},
    geometry::{Cell, Range, Rect},
    model::ViewState,
    view::{OverlayView, ViewCtx},
};
use kurbo::Affine;
use puzzle_frame::ZSlot;

/// An immutable specimen built through the same retained API as extensions.
pub(super) struct Scene;
/// Every row independently demonstrates one effect without changing VT cells.
impl OverlayView for Scene {
    /// Selects visible source cells; no pixel instance is fabricated by the fixture.
    fn build(&mut self, _state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        for (index, line) in ctx.geometry.visible_lines().enumerate() {
            // Blank separator rows are retained in geometry but receive no effect.
            if index % 3 != 0 || index / 3 >= 7 {
                continue;
            }
            let effect = match index / 3 {
                0 => Effect {
                    // Packed words use little-endian RGBA at the frame boundary.
                    color: Some(0xff55dd88),
                    ..Default::default()
                },
                1 => Effect {
                    // Translucent ink leaves the cell paper unchanged.
                    opacity: 0.35,
                    ..Default::default()
                },
                2 => Effect {
                    // Logical displacement receives DPR once during compilation.
                    transform: Affine::translate((8.0, 6.0)),
                    ..Default::default()
                },
                3 => Effect {
                    // Enlarged ink extends past the neutral source row.
                    transform: Affine::scale(1.8),
                    ..Default::default()
                },
                4 => Effect {
                    // Rotation pivots around the source cell top-left.
                    transform: Affine::rotate(-0.25),
                    ..Default::default()
                },
                5 => Effect {
                    // Column-major x shear retains the unmodified y axis.
                    transform: Affine::new([1.0, 0.0, 0.5, 1.0, 0.0, 0.0]),
                    ..Default::default()
                },
                _ => Effect {
                    // Only instances disappear; cell text and advance remain intact.
                    hidden: true,
                    ..Default::default()
                },
            };
            // Effects change only the B ink. Labels prove ordinary drawing still works.
            out.push(
                Anchor::Viewport(Rect::default()),
                ZSlot::Text,
                Primitive::GlyphEffect(
                    Range {
                        start: Cell { line, column: 12 },
                        end: Cell { line, column: 14 },
                    },
                    effect,
                ),
            );
        }
    }
}
