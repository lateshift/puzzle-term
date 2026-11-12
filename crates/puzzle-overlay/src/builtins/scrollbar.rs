//! Exact M8 thumb/markers with hover, capture and one-shot auto-hide (§7.6).
//!
//! An invisible track retains its hit region so entering it can reveal ink.
//! The 800-ms hold uses a single deadline, followed by a 150-ms fade. Reduced
//! motion keeps the hold deadline and removes ink immediately at its end.
//!
//! The scrollbar never derives document fractions from visible rows: the frozen
//! Models sample already contains the full retained-document marker intervals.
//! Track and thumb coordinates arrive in logical pixels, converted once by
//! Session from M8's device geometry. The provider does not round either rectangle,
//! so drag travel and visual thumb extent describe the same model at both DPRs.
//!
//! Region zero covers the track and region one covers its thumb. Submission order
//! makes the thumb win when both overlap. The router owns pointer capture after
//! a consumed press, allowing moves outside the track to continue the drag.
//! The grab offset keeps a thumb from jumping when pressed below its top edge.
//! Track presses request page navigation; they do not imitate a drag fraction.
//! Session handles that request after the callback and applies M8's page size.
//!
//! Source movement restarts the hold, while unrelated theme or text changes do
//! not. Hover and active capture keep the ink visible without polling. Leaving
//! restarts the hold from that injected time. A single deadline wakes the fade;
//! intermediate fade frames use the ordinary animation path, including its final
//! zero-alpha rebuild. The transparent regions survive removal of visible ink.
//! Once the fade ends no deadline remains. This preserves P2/P11 idle behavior.
//!
//! Semantic markers render after the thumb so sparse source facts remain visible.
//! Their intervals include the final document row; a point gets at least one
//! physical pixel of height. Status coloring is resolved by Session, and the
//! provider only looks up the supplied theme token. Opacity multiplies the token's
//! authored alpha, rather than replacing it, retaining translucent thumb themes.
//!
//! The model and view never share mutable state. The model publishes timing and
//! contact facts in an immutable payload; the view evaluates the same visibility
//! function at the snapshot time. Only the model consumes its wake/until values.
//! This keeps fade ink and scheduling consistent without granting a drawing
//! callback access to a Waker or letting it extend an interaction's lifetime.
//!
//! Payload storage recycles only after every outstanding snapshot releases its
//! reference. Reusing a uniquely owned allocation preserves earlier publications
//! and permits warm dirty animation frames without allocating a box per update.
//! Track, thumb and marker samples travel with that same publication, so a view
//! cannot combine new document fractions with old hover or capture state.
//!
//! Pointer coordinates use the last presented geometry, while navigation reads
//! current Session models. Captured drag travel remains an M8 model operation;
//! this UI never owns document scroll state or changes the grid directly.
use super::*;
/// Fixed track clipping differs from relocating an ordinary overlay control.
mod occlusion;
use crate::{
    draw::{CursorIcon, RegionId},
    model::{ModelCtx, OverlayModel, SharedState, ViewState},
    models::Action,
    provider::*,
    view::{OverlayView, ViewCtx},
};
/// Presentation state survives clean frames; navigation belongs to Session.
#[derive(Default)]
pub struct Scrollbar {
    /// Timers and contact are copied into immutable state only on dirty updates.
    state: ScrollbarState,
    /// Unreferenced payload allocations are reused after snapshot rotation.
    states: crate::model::state_pool::StatePool<ScrollbarState>,
}
/// Copy-only interaction data has no renderer cache or glyph ownership.
#[derive(Clone, Copy, Default)]
struct ScrollbarState {
    /// Latest model movement already observed by this painter.
    activity: Option<u64>,
    /// Beginning of the finite visibility hold in injected seconds.
    shown: f64,
    /// Start of the 100-ms reveal, independent of the visibility hold.
    revealed: f64,
    /// Region hover pins visibility until the pointer leaves.
    hover: bool,
    /// Logical distance from thumb top to pressed pointer, during capture.
    grab: Option<f32>,
}
/// The Session advances interaction and publishes render values at its slice boundary.
impl OverlayModel for Scrollbar {
    /// Stable name identifies this built-in without reserving extension region IDs.
    fn name(&self) -> &'static str {
        "scrollbar"
    }
    /// Source markers and geometry are data; pointer and theme affect presentation.
    fn wants(&self) -> EventKinds {
        EventKinds::ALL
    }
    /// Source movement and animation deadlines advance only on the Session owner.
    fn update(&mut self, ctx: &ModelCtx<'_>) {
        let model = ctx.models;
        if model.thumb.size[1] >= model.track.size[1] || model.track.size[1] <= 0.0 {
            return;
        }
        if self.state.activity != Some(model.scroll_activity) {
            // Only a model movement restarts the hold, not a cursor blink or source redraw.
            // Activity after complete fade restarts reveal; continuous movement stays opaque.
            if self.state.activity.is_none() || ctx.time >= self.state.shown + 0.95 {
                self.state.revealed = ctx.time;
            }
            self.state.activity = Some(model.scroll_activity);
            self.state.shown = ctx.time;
        }
        let visibility = self.state.visibility(ctx.time, ctx.reduced_motion);
        if let Some(wake) = visibility.wake {
            ctx.wake_at(wake);
        }
        if let Some(until) = visibility.until {
            ctx.animate_until(until);
        }
    }
    /// Publish timing/contact state, never a draw list or renderer-owned cache.
    fn view_state(&mut self) -> SharedState {
        self.states.publish(&self.state)
    }
    /// Captured pointer coordinates become typed model operations, with exact thumb travel.
    fn on_pointer(
        &mut self,
        ev: &PointerEvent,
        region: RegionId,
        ctx: &mut ModelCtx<'_>,
    ) -> Handled {
        match ev.kind {
            PointerKind::Enter => {
                // Entering a hidden track starts one finite reveal; crossing to its thumb
                // does not flicker because Leave refreshed the hold at this same time.
                if ctx.time >= self.state.shown + 0.95 {
                    self.state.revealed = ctx.time;
                }
                self.state.hover = true;
            }
            PointerKind::Leave => {
                // The hold after leave lets the last position remain legible briefly.
                self.state.hover = false;
                self.state.shown = ctx.time;
            }
            PointerKind::Press if ev.button == Some(0) => {
                self.state.shown = ctx.time;
                if region == RegionId(1) {
                    // Keep the original contact distance while the model moves underneath.
                    self.state.grab = Some(ev.point[1] - ctx.models.thumb.origin[1]);
                } else {
                    // PageTrack carries device y, matching the M8 operation exactly.
                    ctx.requests.action.set(Some(Action::PageTrack(f64::from(
                        ev.point[1] * ctx.geometry.dpr,
                    ))));
                }
                ctx.request_frame();
                return Handled::Yes;
            }
            PointerKind::Move if self.state.grab.is_some() => {
                self.drag(ev, ctx);
                ctx.request_frame();
                return Handled::Yes;
            }
            PointerKind::Release => {
                self.release(ev, ctx);
            }
            _ => {}
        }
        ctx.request_frame();
        // Hover belongs to this UI as well: an application mouse-motion report
        // could invoke scrolling.onInput and move the thumb under the pointer.
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
/// Drawing half has no hover, drag or timer ownership.
pub struct ScrollbarView;
/// Retained commands can be reconstructed from immutable model state at any time.
impl OverlayView for ScrollbarView {
    /// Retains a transparent hit target and paints exact M8 geometry while visible.
    fn build(&mut self, state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let Some(state) = state.as_any().downcast_ref::<ScrollbarState>() else {
            return;
        };
        // Full-height thumbs need neither visible ink nor a scrollable hit target.
        let model = ctx.models;
        if model.thumb.size[1] >= model.track.size[1] || model.track.size[1] <= 0.0 {
            return;
        }
        // A time-based alpha changes presentation only, never thumb geometry.
        let opacity = state.visibility(ctx.time, ctx.reduced_motion).alpha;
        // Native UI wins covered clicks. Preserve the fixed document mapping by
        // excluding only covered vertical bands, even for a one-pixel shadow.
        let covered = model.occluded_rects.as_deref().unwrap_or(&[]);
        occlusion::track_bands(model.track, covered, |band| {
            push(
                out,
                Primitive::Region(band, RegionId(0), CursorIcon::Pointer),
            );
        });
        // Keep partial thumbs' complete grab bounds and painter priority. Hidden
        // ink retains these targets too, so hover needs no polling timer.
        if occlusion::thumb_visible(model.thumb, covered) {
            push(
                out,
                Primitive::Region(
                    model.thumb,
                    RegionId(1),
                    if state.grab.is_some() {
                        CursorIcon::Grabbing
                    } else {
                        CursorIcon::Grab
                    },
                ),
            );
        }
        if opacity == 0.0 {
            return;
        }
        // Draw the track first, then the translucent thumb above its paper.
        rect(
            out,
            model.track,
            alpha(ctx.theme.color("scrollbarTrack", 0xff202020), opacity),
            3.0,
        );
        rect(
            out,
            model.thumb,
            alpha(ctx.theme.color("scrollbarThumb", 0x66ffffff), opacity),
            3.0,
        );
        markers(ctx, out, opacity);
    }
}

/// Contact updates remain model-only, independent of the painter's cached commands.
impl Scrollbar {
    /// Completes a drag from its final sample before releasing provider capture.
    fn release(&mut self, ev: &PointerEvent, ctx: &ModelCtx<'_>) {
        // Hosts discard queued motion on release. Apply its final coordinate
        // before ending capture, including a drag shorter than one refresh.
        if self.state.grab.is_some() {
            self.drag(ev, ctx);
        }
        // Capture releases in the shared router after this final callback.
        self.state.grab = None;
        self.state.shown = ctx.time;
    }
    /// Maps a captured point to available travel, letting M8 clamp the resulting fraction.
    fn drag(&self, ev: &PointerEvent, ctx: &ModelCtx<'_>) {
        // Available travel excludes the thumb height so both ends are reachable.
        let travel = ctx.models.track.size[1] - ctx.models.thumb.size[1];
        if travel > 0.0 {
            let top = ev.point[1] - self.state.grab.unwrap_or(0.0) - ctx.models.track.origin[1];
            ctx.requests
                .action
                .set(Some(Action::DragThumb(f64::from(top / travel))));
        }
    }
}

/// Visibility evaluation is pure, so the view may animate without scheduling access.
impl ScrollbarState {
    /// Computes visibility from immutable timing state, without requesting work.
    fn visibility(&self, time: f64, reduced_motion: bool) -> Visibility {
        if !reduced_motion && time < self.revealed + 0.1 {
            return Visibility {
                alpha: ((time - self.revealed) / 0.1).clamp(0.0, 1.0) as f32,
                wake: Some(self.shown + 0.8),
                until: Some(self.revealed + 0.1),
            };
        }
        if self.hover || self.grab.is_some() {
            return Visibility::opaque();
        }
        let deadline = self.shown + 0.8;
        if time < deadline {
            return Visibility {
                wake: Some(deadline),
                ..Visibility::opaque()
            };
        }
        // One authored endpoint avoids floating-point drift from adding 0.8 + 0.15.
        let end = self.shown + 0.95;
        if reduced_motion || time >= end {
            return Visibility {
                alpha: 0.0,
                wake: None,
                until: None,
            };
        }
        Visibility {
            alpha: (1.0 - (time - deadline) / 0.15) as f32,
            wake: None,
            until: Some(end),
        }
    }
}

/// Projects cached document fractions onto the exact track, retaining one-pixel point ink.
fn markers(ctx: &ViewCtx<'_>, out: &mut DrawList, opacity: f32) {
    for marker in &ctx.models.markers {
        // Fractions come from retained document indices, including inclusive range ends.
        let y = ctx.models.track.origin[1] + marker.start as f32 * ctx.models.track.size[1];
        let height = ((marker.end - marker.start) as f32 * ctx.models.track.size[1])
            .max(1.0 / ctx.geometry.dpr);
        rect(
            out,
            Rect {
                origin: [ctx.models.track.origin[0], y],
                size: [ctx.models.track.size[0], height],
            },
            alpha(ctx.theme.color(marker.token, 0xffffffff), opacity),
            0.0,
        );
    }
}

/// Pure visibility evaluation lets models schedule while views only paint.
struct Visibility {
    /// Straight-alpha multiplier for this injected time.
    alpha: f32,
    /// Sleeping hold endpoint, consumed only by the model half.
    wake: Option<f64>,
    /// Active reveal/fade endpoint, consumed only by the model half.
    until: Option<f64>,
}
/// Hover and capture keep ink opaque without recurring timers.
impl Visibility {
    /// No deadline is needed while input itself drives changes.
    fn opaque() -> Self {
        Self {
            alpha: 1.0,
            wake: None,
            until: None,
        }
    }
}
