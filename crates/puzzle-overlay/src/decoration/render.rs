//! One built-in provider paints every declaration through the public API (§11.3).
//!
//! Source positions arrive from Session; this provider only uses visible logical
//! geometry. Rectangles are already surface-relative and use a zero viewport
//! anchor, avoiding a second marker lookup or a duplicate padding translation.
//!
//! The declaration owner and painter have different lifetimes. Session keeps
//! source versions and marker handles while this provider keeps hover state.
//! A snapshot can outlive a parser turn without changing its definitions.
//! The provider never reads a hidden line or obtains a mutable terminal borrow.
//!
//! Ranges may start outside the viewport. Their visible row rectangles are
//! independently painted, so the first endpoint need not itself be visible.
//! A Gone source produces neither ink nor a region; hiding leaves ownership
//! with Session until the source returns or an auto-remove witness expires.
//!
//! All regions use the same slot as their declared ink. Grid slots inherit
//! the renderer's exact grid scissor, whereas Top permits gutter and panels.
//! The fixed origin is essential: these rectangles already include padding
//! and fractional scroll displacement from TextGeometry.
//!
//! Tooltips render in Top, after all source annotations. They have no region
//! of their own, which prevents the moving panel from stealing its source's
//! hover. Their placement is clamped to the surface and their theme colors
//! are resolved from the same frozen theme table as the underlying text.
//!
//! A handled press acquires the shared router's capture. Even after leaving
//! the source, its moves and release remain owned by that annotation. Hover
//! transitions still follow the visible region, so moving away hides its
//! tooltip while the captured release cannot start application selection.
//!
//! The reduced-motion flag is available through the public context; these
//! static annotations have no animation interval and request only one frame
//! when pointer presentation changes. Clean draw lists remain retained.
use super::*;
use crate::{
    draw::{CursorIcon, DrawList, Primitive, RectSpec, RegionId},
    model::{ModelCtx, OverlayModel, SharedState, ViewState},
    provider::*,
    view::{OverlayView, ViewCtx},
};
/// Size/fit helpers preserve natural text baselines and authored SVG coordinates.
mod content;
/// Font-adjacent underline primitives share the grid's style catalogue.
mod underline;
/// Retained pointer state is presentation only; source definitions stay in Session.
#[derive(Default)]
pub struct Decorations {
    /// Visible row segments reuse their allocation across dirty builds.
    rects: Vec<Rect>,
    /// Built-in underline paths have a private key space, isolated from extension keys.
    underline_shapes: crate::shapes::ShapeCache,
}
/// Session-owned tooltip interaction, separate from source definitions and drawing caches.
#[derive(Default)]
pub struct DecorationsModel {
    /// Small copy-only value; no shapes or font resources live with interaction.
    state: DecorationState,
    /// Recycles payload storage only after every snapshot has released it.
    states: crate::model::state_pool::StatePool<DecorationState>,
}
/// The drawing half receives a read-only contact state on dirty publications.
#[derive(Clone, Copy, Default)]
struct DecorationState {
    /// Hovered identity drives one optional floating tooltip.
    hover: Option<RegionId>,
    /// Most recent logical coordinate places the tooltip near the pointer.
    pointer: [f32; 2],
}
/// Uses the same provider contract as an extension; keyboard focus is absent.
impl OverlayModel for DecorationsModel {
    /// Stable diagnostic name for the single declaration renderer.
    fn name(&self) -> &'static str {
        "decorations"
    }
    /// Source geometry, theme and pointer changes can alter visible annotation ink.
    fn wants(&self) -> EventKinds {
        EventKinds::ALL
    }
    /// Only hover and pointer position cross into the immutable drawing state.
    fn view_state(&mut self) -> SharedState {
        self.states.publish(&self.state)
    }
    /// Tooltips consume their button gestures so no selection/report can begin below them.
    fn on_pointer(
        &mut self,
        event: &PointerEvent,
        region: RegionId,
        ctx: &mut ModelCtx<'_>,
    ) -> Handled {
        self.state.pointer = event.point;
        match event.kind {
            PointerKind::Enter => self.state.hover = Some(region),
            PointerKind::Leave if self.state.hover == Some(region) => self.state.hover = None,
            _ => {}
        }
        // Moving a shown tooltip or crossing a region requires one rebuilt presentation.
        ctx.request_frame();
        if matches!(event.kind, PointerKind::Press | PointerKind::Release) {
            Handled::Yes
        } else {
            Handled::No
        }
    }
}
/// Drawing uses only immutable interaction state and rebuildable render caches.
impl OverlayView for Decorations {
    /// Renders visible segments only, including ranges entering from offscreen text.
    /// A range is sampled once by Session and converted to row rectangles here.
    /// Reused scratch capacity prevents one rectangle-vector allocation per source.
    /// Fixed viewport attachments retain those already resolved logical coordinates.
    /// Tooltip output comes last so its Top panel is above the declaration ink.
    fn build(&mut self, state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let Some(state) = state.as_any().downcast_ref::<DecorationState>() else {
            return;
        };
        for sample in ctx.decorations {
            self.rects.clear();
            match sample.resolved {
                Resolved::Cells(range) => ctx.geometry.range_rects(range, &mut self.rects),
                Resolved::Rect(rect) => self.rects.push(rect),
                Resolved::Gone | Resolved::Hidden => continue,
            }
            if self.rects.is_empty() {
                continue;
            }
            self.paint(sample, ctx, out);
            if sample.declaration.region.is_some() {
                for rect in self.rects.iter().filter(|rect| ctx.unoccluded(**rect)) {
                    // Keep annotation ink anchored; covered segments have no hit target.
                    push(
                        out,
                        sample.declaration.slot,
                        Primitive::Region(*rect, RegionId(sample.id.0), CursorIcon::Pointer),
                    );
                }
            }
        }
        self.tooltip(state, ctx, out);
    }
}
/// Kind dispatch shares source rectangles but keeps preparation helpers independent.
impl Decorations {
    /// Appends annotation ink in the caller's declared painter slot.
    /// Highlights and boxes emit one rectangle per visible row segment.
    /// Effects retain their inclusive cell interval rather than transforming cells.
    /// Natural text and SVG preparation use separate helpers for their own units.
    fn paint(&self, sample: &Sample, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let slot = sample.declaration.slot;
        match &sample.declaration.kind {
            DecorationKind::Highlight { fill } => {
                for rect in &self.rects {
                    rectangle(out, slot, *rect, *fill, 0.0, 0.0);
                }
            }
            DecorationKind::Box { stroke, radius } => {
                for rect in &self.rects {
                    rectangle(out, slot, *rect, *stroke, *radius, 1.0 / ctx.geometry.dpr);
                }
            }
            DecorationKind::Underline { style, color } => {
                underline::paint(
                    &self.rects,
                    *style,
                    *color,
                    slot,
                    ctx,
                    out,
                    &self.underline_shapes,
                );
            }
            DecorationKind::GlyphEffect(effect) => {
                if let Resolved::Cells(range) = sample.resolved {
                    push(out, slot, Primitive::GlyphEffect(range, *effect));
                }
            }
            // Text and SVG share natural measurement and fitting without accessing live VT.
            _ => content::paint(sample, &self.rects, ctx, out),
        }
    }
    /// Draws a bounded one-line tooltip from the hovered immutable definition.
    /// A removed or rewritten definition immediately loses its floating panel.
    /// Font measurement determines panel width including whitespace advances.
    /// The text transform translates its line top-left; its baseline is already laid out.
    fn tooltip(&self, state: &DecorationState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let Some(sample) = ctx
            .decorations
            .iter()
            .find(|s| Some(RegionId(s.id.0)) == state.hover)
        else {
            return;
        };
        let Some(RegionSpec::Tooltip(label)) = &sample.declaration.region else {
            return;
        };
        let style = TextStyle {
            size: 12.0,
            face: None,
            style: 0,
            color: ctx.theme.color("tooltipForeground", 0xffffffff),
        };
        let mut text = content::text(label, style);
        let metrics = content::measure(&text, ctx);
        let extent = [metrics.width + 12.0, metrics.height + 8.0];
        // Clamp the floating panel so crossing the final cell cannot put it off-surface.
        let origin = [
            (state.pointer[0] + 12.0).min((ctx.geometry.viewport.size[0] - extent[0]).max(0.0)),
            (state.pointer[1] + 16.0).min((ctx.geometry.viewport.size[1] - extent[1]).max(0.0)),
        ];
        rectangle(
            out,
            ZSlot::Top,
            Rect {
                origin,
                size: extent,
            },
            ctx.theme.color("tooltipBackground", 0xff242424),
            3.0,
            0.0,
        );
        text.transform =
            Affine::translate((f64::from(origin[0] + 6.0), f64::from(origin[1] + 4.0)));
        push(out, ZSlot::Top, Primitive::Text(text));
    }
}
/// Global logical geometry uses a fixed zero attachment for all painter commands.
pub(super) fn push(out: &mut DrawList, slot: ZSlot, primitive: Primitive) {
    out.push(Anchor::Viewport(Rect::default()), slot, primitive);
}
/// Filled and inward-stroked rectangles use the renderer's shared SDF contract.
pub(super) fn rectangle(
    out: &mut DrawList,
    slot: ZSlot,
    rect: Rect,
    color: u32,
    radius: f32,
    stroke: f32,
) {
    push(
        out,
        slot,
        Primitive::Rect(RectSpec {
            rect,
            color,
            radius,
            stroke,
        }),
    );
}
