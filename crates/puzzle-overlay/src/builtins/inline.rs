//! Cursor annotations are ordinary public overlay primitives (design §11.6).
//!
//! Candidates and suggestions stay outside VT cells. Cursor anchoring follows
//! the sealed publication, so parsing after capture cannot move older ink.
//! Composition wins over a suggestion; clearing the candidate reveals it again.
//!
//! Text uses the shared primary/fallback layouter at the grid's logical font size.
//! Natural annotation advances do not reserve columns or move the VT cursor.
//! The cursor anchor is resolved against the frame's visible text geometry, so
//! scrolling the application cursor offscreen also hides these annotations.
//! The underline uses the same text measurement as the compiler, including
//! ligatures and fallback advances; byte length is never treated as a width.
//! Its thickness is exactly one device pixel after the logical-to-device map.
//! Top intentionally permits composition ink to extend into surface padding.
//! OS candidate-window placement remains the platform adapter's responsibility.
use super::*;
use crate::{model::ViewState, view::OverlayView};

/// Rebuildable drawing half for preedit and ghost text; no interaction or scheduling.
pub struct InlineView;
/// Text placement is reconstructible; the source owner decides when to invalidate it.
impl OverlayView for InlineView {
    /// Draws natural text at the current cursor without changing any cell advance.
    fn build(&mut self, _: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let source = &ctx.models.visuals;
        // An in-progress IME candidate owns the cursor annotation until it clears.
        let composing = !source.preedit.is_empty();
        let label = if composing {
            &source.preedit
        } else {
            &source.ghost
        };
        if label.is_empty() || ctx.models.search.open || ctx.models.help.open {
            // Query composition belongs in its query field, not the application's grid.
            return;
        }
        let size = source.font_size;
        let token = if composing { "foreground" } else { "ghostText" };
        let color = ctx.theme.color(token, 0xffaaaaaa);
        let spec = TextSpec {
            text: label.clone(),
            size,
            face: None,
            style: 0,
            color,
            transform: kurbo::Affine::IDENTITY,
        };
        // Measure before moving the owned specification into the retained command.
        let width = ctx
            .text
            .and_then(|text| text.measure(&spec).ok())
            .map_or(0.0, |metrics| metrics.width);
        out.push(Anchor::Cursor, ZSlot::Top, Primitive::Text(spec));
        if composing {
            // The public layouter measures the same natural advances used for drawing.
            let thickness = 1.0 / ctx.geometry.dpr.max(1.0);
            out.push(
                Anchor::Cursor,
                ZSlot::Top,
                Primitive::Rect(RectSpec {
                    rect: Rect {
                        origin: [0.0, ctx.geometry.cell[1] - thickness],
                        size: [width, thickness],
                    },
                    color: ctx.theme.color("preeditUnderline", color),
                    radius: 0.0,
                    stroke: 0.0,
                }),
            );
        }
    }
}
