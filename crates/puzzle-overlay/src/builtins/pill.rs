//! New-output pill draws M8's state and returns click-to-follow (§7.5).
//!
//! There is no local unread count: the captured Boolean comes from Viewport.
//! Its region owns presses/releases even when the application tracks the mouse.
//!
//! Viewport owns whether output arrived while the view was detached from its
//! live bottom. The Boolean is sampled with the frame, so this provider neither
//! scans history nor maintains an unread count. Following again clears that
//! state through M8's normal navigation operation and invalidates this list.
//!
//! A surface anchor keeps the pill centered while text scrolls below it. Top
//! intentionally draws beyond the grid scissor, with twelve logical pixels of
//! breathing room below the pill. Text uses natural font advances and shares
//! the host's primary/fallback faces. Both semantic tokens are resolved anew
//! on a theme invalidation, including the host's live theme transition.
//!
//! Its hit target covers both label and paper. Press is consumed to establish
//! capture, and release emits exactly one typed request after the callback.
//! The router consumes the captured pair even if the pointer leaves the target.
//! No input bytes, clipboard data or focus ownership is involved in the click.
use super::*;
use crate::{
    draw::{CursorIcon, RegionId},
    model::{ModelCtx, OverlayModel, SharedState, ViewState},
    models::Action,
    provider::*,
    view::{OverlayView, ViewCtx},
};
/// A stateless provider retains its draw list until the model changes.
pub struct Pill {
    /// One immutable unit payload is reused even on dirty grid/source frames.
    state: SharedState,
}
/// Installation is cold; rebuilding a stateless view never creates a new payload.
impl Default for Pill {
    /// Allocate the unit state once for this registration's lifetime.
    fn default() -> Self {
        Self {
            state: std::sync::Arc::new(()),
        }
    }
}
impl OverlayModel for Pill {
    /// Stable diagnostics name for the new-output affordance.
    fn name(&self) -> &'static str {
        "new-output"
    }
    /// Output/following, size and theme can alter its retained presentation.
    fn wants(&self) -> EventKinds {
        EventKinds::ALL
    }
    /// Source data lives in the Session snapshot; this model has no private render payload.
    fn view_state(&mut self) -> SharedState {
        self.state.clone()
    }
    /// A completed click asks Session to use the existing Bottom navigation command.
    fn on_pointer(&mut self, ev: &PointerEvent, _: RegionId, ctx: &mut ModelCtx<'_>) -> Handled {
        if ev.kind == PointerKind::Release && ev.button == Some(0) {
            ctx.requests.action.set(Some(Action::Bottom));
        }
        // UI hover must not emit an application report and trigger input-following.
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

/// Stateless drawing half; source data and interaction remain in the Session.
pub struct PillView;
/// The view can be recreated without losing any query, click or following state.
impl OverlayView for PillView {
    /// Positions the pill above the bottom edge without altering terminal cells.
    fn build(&mut self, _state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        if !ctx.models.new_output {
            return;
        }
        let surface = ctx.geometry.viewport.size;
        let bounds = Rect {
            origin: [
                (surface[0] - 132.0).max(0.0) / 2.0,
                (surface[1] - 36.0).max(0.0),
            ],
            size: [132.0_f32.min(surface[0]), 24.0],
        };
        // A host's native controls can cover the usual bottom-centred location.
        let Some(bounds) = ctx.place(bounds) else {
            return;
        };
        rect(
            out,
            bounds,
            ctx.theme.color("pillBackground", 0xff333333),
            12.0,
        );
        text(
            out,
            "↓ new output".into(),
            [bounds.origin[0] + 12.0, bounds.origin[1] + 4.0],
            ctx.theme.color("pillForeground", 0xffffffff),
        );
        push(
            out,
            Primitive::Region(bounds, RegionId(0), CursorIcon::Pointer),
        );
    }
}
