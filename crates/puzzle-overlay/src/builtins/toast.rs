//! Harness notification toasts (user decision 2026-10-12, design §11).
//!
//! Session owns admission and the bounded queue. This provider only draws its
//! immutable source. There are no timers or expiration guesses: idle stays free.
//! Presented regions carry generation IDs, so an old click cannot dismiss an update.
//!
//! No protocol metadata is interpreted here. The frozen source already contains
//! sanitized text and admitted urgency values. Serial numbers identify a single
//! generation rather than an application ID that can be reused on replacement.
//! Bit zero distinguishes dismissal from activation; higher bits preserve serial.
//! The close affordance is emitted after the body region so painter-order hit
//! testing selects it in the overlap. Neither callback writes application bytes.
//! Panels use the same free-text and rounded-rectangle paths as other built-ins.
//! All coordinates are logical surface pixels; the compiler applies DPR once.
//! Rendering never schedules an expiration timer or samples host notification state.
//!
//! A non-fitting entry remains queued until the host clears presentation space.
//! Relocation preserves the serial used by activation and dismissal regions.
use super::*;
use crate::{
    draw::{CursorIcon, RegionId},
    model::{ModelCtx, OverlayModel, SharedState, ViewState},
    models::Action,
    provider::*,
    view::{OverlayView, ViewCtx},
};
/// Stateless interaction half; notification lifetime remains with Session.
pub struct Toasts {
    /// One reused view-state allocation for the registration lifetime.
    state: SharedState,
}
/// Registration does not allocate a notification queue or start a clock.
impl Default for Toasts {
    /// Dirty source publications share a unit state rather than copying a queue.
    fn default() -> Self {
        Self {
            state: std::sync::Arc::new(()),
        }
    }
}
/// Pointer actions return to the session owner through the overlay request mailbox.
impl OverlayModel for Toasts {
    /// Stable registry name for diagnostics and presented-region routing.
    fn name(&self) -> &'static str {
        "notifications"
    }
    /// Delivery, geometry and theme updates invalidate the retained draw list.
    fn wants(&self) -> EventKinds {
        EventKinds::ALL
    }
    /// State lives in the source snapshot; the model never copies it per frame.
    fn view_state(&mut self) -> SharedState {
        // Cloning the immutable unit state changes a reference count, not queue contents.
        self.state.clone()
    }
    /// Release activates the body or dismisses the explicit close affordance.
    fn on_pointer(
        &mut self,
        ev: &PointerEvent,
        region: RegionId,
        ctx: &mut ModelCtx<'_>,
    ) -> Handled {
        // Only left-button release completes a user action; motion and press merely consume.
        if ev.kind == PointerKind::Release && ev.button == Some(0) {
            ctx.requests.action.set(Some(Action::Notification {
                // Presented regions preserve the generation, not a reusable application ID.
                serial: region.0 >> 1,
                // Bit zero is reserved for the explicit dismiss affordance.
                activate: region.0 & 1 == 0,
            }));
        }
        // Consume presses/motion too: a toast must not click through into a mouse program.
        if matches!(
            ev.kind,
            PointerKind::Press | PointerKind::Release | PointerKind::Move
        ) {
            Handled::Yes
        } else {
            Handled::No
        }
    }
}
/// Rebuildable drawing half, with no scheduling or native notification calls.
pub struct ToastsView;
/// Drawing consumes only published values, never the host notification service.
impl OverlayView for ToastsView {
    /// Draw newest first; older queued entries appear after dismissal frees space.
    fn build(&mut self, _: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        // An empty source contributes neither primitives nor regions.
        let Some(items) = &ctx.models.toasts else {
            return;
        };
        // Surface bounds include padding; notifications are fixed to the view, not grid rows.
        let surface = ctx.geometry.viewport.size;
        // Small windows clamp the panel instead of creating an out-of-surface scissor.
        let width = 360.0_f32.min(surface[0]);
        let x = (surface[0] - width - 12.0).max(0.0);
        // Stacking starts below the surface edge, leaving the title bar outside this layer.
        let mut y = 12.0;
        // Newest-first layout keeps recent notifications visible when the queue exceeds height.
        for item in items.iter().rev() {
            // Entries beyond available height remain queued rather than drawing clipped controls.
            if y + 76.0 > surface[1] {
                break;
            }
            // One source entry contributes one independently dismissible panel.
            let Some(bounds) = ctx.place(Rect {
                origin: [x, y],
                size: [width, 72.0],
            }) else {
                break;
            };
            panel(ctx, out, item, bounds.origin[0], bounds.origin[1], width);
            // Stack from the relocated panel, never on top of a preceding toast.
            y = bounds.origin[1] + 84.0;
        }
    }
}

/// One panel uses logical geometry; its close region is emitted last and wins hit tests.
fn panel(
    ctx: &ViewCtx<'_>,
    out: &mut DrawList,
    item: &crate::models::toast::Toast,
    x: f32,
    y: f32,
    width: f32,
) {
    // The same rectangle drives panel ink, text clipping and its presented hit target.
    let bounds = Rect {
        // The top-right stack remains independent of the terminal scroll offset.
        origin: [x, y],
        size: [width, 72.0],
    };
    // Background is opaque enough to read over arbitrary application output.
    rect(
        out,
        bounds,
        ctx.theme.color("tooltipBackground", 0xff303030),
        8.0,
    );
    // Urgency selects existing semantic theme tokens, including high-contrast themes.
    let token = match item.urgency {
        // Low urgency recedes visually without hiding the notification text.
        0 => "ghostText",
        // Urgent notifications share the existing error/status colour.
        2 => "zoneError",
        // Normal urgency uses the same accent as other interactive overlays.
        _ => "accent",
    };
    rect(
        out,
        Rect {
            origin: [x, y + 8.0],
            size: [3.0, 56.0],
        },
        ctx.theme.color(token, 0xffffffff),
        0.0,
    );
    panel_text(ctx, out, item, bounds, token);
    // The body is the default activation target, including otherwise empty panel space.
    push(
        out,
        Primitive::Region(bounds, RegionId(item.serial << 1), CursorIcon::Pointer),
    );
    // The close button covers long title ink and wins the overlapping body hit region.
    close_button(ctx, out, item.serial, x + width - 28.0, y);
}

/// Clip title and body together without clipping the independently painted close button.
fn panel_text(
    ctx: &ViewCtx<'_>,
    out: &mut DrawList,
    item: &crate::models::toast::Toast,
    bounds: Rect,
    token: &str,
) {
    // Fixed panel clips long labels; protocol text is never interpreted as markup.
    push(out, Primitive::ClipPush(bounds));
    // Heading and urgency rail share a semantic colour to reinforce the priority.
    text(
        out,
        item.title.clone(),
        [bounds.origin[0] + 12.0, bounds.origin[1] + 10.0],
        ctx.theme.color(token, 0xffffffff),
    );
    // The body uses normal tooltip contrast rather than the urgency colour.
    text(
        out,
        item.body.clone(),
        [bounds.origin[0] + 12.0, bounds.origin[1] + 34.0],
        ctx.theme.color("tooltipForeground", 0xffffffff),
    );
    // Restore the previous clip before adding controls to the interaction table.
    push(out, Primitive::ClipPop);
}

/// Dismissal has a separate presented identity from the default activation region.
fn close_button(ctx: &ViewCtx<'_>, out: &mut DrawList, serial: u64, x: f32, y: f32) {
    // A full button-sized region makes dismissal easier than targeting only the × glyph.
    let close = Rect {
        // Inset four points prevents the button from touching the rounded panel edge.
        origin: [x, y + 4.0],
        size: [24.0, 24.0],
    };
    // Paint a separate background so long titles cannot obscure the dismiss mark.
    rect(
        out,
        close,
        ctx.theme.color("badgeBackground", 0xff404040),
        4.0,
    );
    // Free text uses the common Slug layouter, including the multiply glyph fallback.
    text(
        out,
        "×".into(),
        [close.origin[0] + 7.0, y + 8.0],
        ctx.theme.color("badgeForeground", 0xffffffff),
    );
    // Emit last because reverse painter order resolves overlapping hit regions.
    push(
        out,
        Primitive::Region(close, RegionId((serial << 1) | 1), CursorIcon::Pointer),
    );
}
