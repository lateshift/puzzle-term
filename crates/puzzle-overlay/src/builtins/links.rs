//! Grouped link ink and one finite tooltip delay on the public split overlay API.
//!
//! The Session supplies only links whose row versions match presented pixels.
//! A below-text grid region observes first hover without swallowing selection.
//! Higher overlay slots still win; opening happens only on a primary-modifier
//! left press. The typed action returns to normal Command::OpenLink policy.
//! The model owns the hover epoch and schedules a single 500-ms wake. Neither
//! URL lookup nor a timer is available to the view. Moving within one target
//! preserves that epoch, including disjoint cells sharing an OSC 8 id.
//! Underlines use cell geometry, so fallback glyph bounds cannot move clicks.
//! Tooltip paper and text are clipped to the surface, including narrow windows.
//! A tooltip is intentionally not a popup window: it shares the terminal's
//! submitted frame and survives the same clipping and upload rules as other ink.
//! Link content comes from application output, so the view treats it as a label,
//! never as markup, an executable command or a browser-provided HTML fragment.
//! Scheme approval is an owner-side fact. Drawing a disallowed URI is useful
//! feedback but does not grant permission to open it.
//! Both passive and active regions belong to this model; crossing their boundary
//! therefore leaves the URI epoch intact instead of restarting the delay.
use super::*;
use crate::{
    draw::{CursorIcon, RegionId},
    model::{ModelCtx, OverlayModel, SharedState, ViewState, state_pool::StatePool},
    provider::{EventKinds, Handled, PointerEvent, PointerKind},
    view::OverlayView,
};

/// The one specified delay is not a recurring polling interval (§11.6).
const TOOLTIP_DELAY: f64 = 0.5;
/// Interaction and time stay on the Session side of each publication.
#[derive(Default)]
pub struct Links {
    /// Target identity, retained only while the pointer sees the same URI.
    uri: String,
    /// Clock at the first hover over this target.
    since: f64,
    /// True after the model's one delayed wake.
    tooltip: bool,
    /// Boolean payloads recycle after sealed publications release them.
    states: StatePool<bool>,
}
/// Interaction owns the deadline; drawing receives its boolean result.
impl OverlayModel for Links {
    /// Stable identity used by presented regions and diagnostic reports.
    fn name(&self) -> &'static str {
        "link-hover"
    }
    /// Output can replace a hovered target; pointer movement can discover one.
    fn wants(&self) -> EventKinds {
        EventKinds::ALL
    }
    /// Only the model owns the delay; ordinary redraws do not restart it.
    fn update(&mut self, ctx: &ModelCtx<'_>) {
        // The owner already checked these cells against the last presented rows.
        let uri = &ctx.models.link.uri;
        if self.uri != *uri {
            // Reuse the previous URI allocation for repeated targets (P1).
            self.uri.clone_from(uri);
            // Changing identity starts a new delay; a redraw of it does not.
            self.since = ctx.time;
            self.tooltip = false;
        }
        // Leaving all links removes the label without scheduling a cleanup timer.
        if uri.is_empty() {
            self.tooltip = false;
            return;
        }
        // Equality includes the exact injected 500-ms boundary used by tests.
        self.tooltip = ctx.time >= self.since + TOOLTIP_DELAY;
        if !self.tooltip {
            // A single future deadline also works when the terminal is otherwise idle.
            ctx.wake_at(self.since + TOOLTIP_DELAY);
        }
    }
    /// One immutable boolean suffices; the target is already a sealed source.
    fn view_state(&mut self) -> SharedState {
        // The pool retains the small publication while render readers own older values.
        self.states.publish(&self.tooltip)
    }
    /// Hover never consumes selection, while a valid modifier-click does.
    fn on_pointer(&mut self, ev: &PointerEvent, _: RegionId, ctx: &mut ModelCtx<'_>) -> Handled {
        let link = &ctx.models.link;
        // Ordinary presses, releases and hover continue to selection/mouse routing.
        // Primary is supplied by the host OS table, not inferred in this neutral crate.
        // The observer supports a click before hover ink has been presented.
        // Reject covered points directly rather than requiring a hover redraw.
        if ev.kind == PointerKind::Press
            && ev.button == Some(0)
            && link.openable
            && !link.uri.is_empty()
            && ev.modifiers & link.primary != 0
            && ctx
                .models
                .occluded_rects
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .all(|rect| !rect.contains(ev.point))
        {
            // URI is deliberately absent: the owner validates its current source.
            ctx.requests
                .action
                .set(Some(crate::models::Action::OpenHoveredLink));
            return Handled::Yes;
        }
        Handled::No
    }
}

/// Rendering receives only immutable target cells and the elapsed-delay result.
pub struct LinksView;
/// Rebuildable ink has no access to timer requests or mutable target selection.
impl OverlayView for LinksView {
    /// Emits a passive grid observer, dotted cells, pointer regions and optional URL.
    fn build(&mut self, state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        // The passive observer is below text/UI, so it cannot steal a search button.
        out.push(
            Anchor::Viewport(Rect::default()),
            ZSlot::BelowText,
            Primitive::Region(ctx.geometry.grid, RegionId(0), CursorIcon::Default),
        );
        let link = &ctx.models.link;
        // Semantic link color follows live theme changes without changing geometry.
        let color = ctx.theme.color("link", 0xffffcc66);
        for cell in &link.cells {
            // A grouped segment can leave the viewport between two publications.
            let Some(bounds) = ctx.geometry.cell_rect(*cell) else {
                continue;
            };
            // The underline is source ink; its click target cannot move to other text.
            // Covered cells keep the underline but omit interaction until uncovered.
            if ctx.unoccluded(bounds) {
                out.push(
                    Anchor::Viewport(Rect::default()),
                    ZSlot::AboveText,
                    Primitive::Region(bounds, RegionId(1), CursorIcon::Pointer),
                );
            }
            // Repeating two-device-pixel dots align at each integer-width cell edge.
            // Logical spacing converts one device pixel back through this frame's DPR.
            let unit = 1.0 / ctx.geometry.dpr.max(f32::EPSILON);
            // Each cell restarts the pattern, so wide and disjoint segments stay aligned.
            let mut x = 0.0;
            while x < bounds.size[0] {
                out.push(
                    Anchor::Viewport(Rect::default()),
                    ZSlot::AboveText,
                    Primitive::Rect(RectSpec {
                        rect: Rect {
                            origin: [
                                bounds.origin[0] + x,
                                bounds.origin[1] + bounds.size[1] - unit,
                            ],
                            // Clamp the final dot at a fractional logical cell edge.
                            size: [unit.min(bounds.size[0] - x), unit],
                        },
                        color,
                        // Square, filled dots avoid rounded-edge changes in stem coverage.
                        radius: 0.0,
                        stroke: 0.0,
                    }),
                );
                // One device pixel of ink alternates with one device pixel of space.
                x += 2.0 * unit;
            }
        }
        // An empty source cannot leave a tooltip behind after output replaces the link.
        if !link.uri.is_empty() && state.as_any().downcast_ref::<bool>() == Some(&true) {
            tooltip(ctx, out, &link.uri);
        }
    }
}
/// A URL stays readable data, constrained to the visible surface rather than a window.
fn tooltip(ctx: &ViewCtx<'_>, out: &mut DrawList, uri: &str) {
    // Use cell placement rather than pointer coordinates from a later frame.
    let Some(cell) = ctx
        .models
        .link
        .cells
        .first()
        .and_then(|c| ctx.geometry.cell_rect(*c))
    else {
        return;
    };
    // Width uses the common layouter's advances rather than UTF-8 byte length.
    let size = [
        (super::query_field::width(ctx, uri) + 16.0).min(ctx.geometry.viewport.size[0]),
        24.0,
    ];
    // Clamp each edge against the surface; Top is not scissored to the text grid.
    let bounds = Rect {
        origin: [
            cell.origin[0].min((ctx.geometry.viewport.size[0] - size[0]).max(0.0)),
            (cell.origin[1] + cell.size[1]).min((ctx.geometry.viewport.size[1] - size[1]).max(0.0)),
        ],
        size,
    };
    // Tooltip text is movable chrome; the source underline remains on its cells.
    let Some(bounds) = ctx.place(bounds) else {
        return;
    };
    rect(
        out,
        bounds,
        ctx.theme.color("tooltipBackground", 0xff202020),
        4.0,
    );
    // Long URLs stay inside their paper even on a window narrower than the label.
    push(out, Primitive::ClipPush(bounds));
    text(
        out,
        uri.into(),
        [bounds.origin[0] + 8.0, bounds.origin[1] + 4.0],
        ctx.theme.color("tooltipForeground", 0xffffffff),
    );
    // Restore Top's prior clip before later providers append their commands.
    push(out, Primitive::ClipPop);
}
