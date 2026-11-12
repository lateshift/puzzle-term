//! Finite visual bells and source-driven progress use the public split API.
//!
//! The model owns the bell interval. Views only interpolate frozen endpoints;
//! no view has a timer. Reduced motion holds a static border until its one wake.
//!
//! Bell facts have already crossed the terminal's protocol rate limiter. The
//! identity is independent of output damage: printing more text during a flash
//! does not restart its duration. A sealed state contains only two clock values,
//! so a slow renderer cannot observe a later bell through the earlier snapshot.
//! Accessibility disables interpolation, while retaining the removal deadline.
//! Progress is similarly a source value, rather than a recurring animation.
//! No spinner or polling timer is created for indeterminate application status.
//! All rectangles use logical surface coordinates, including the padding area.
use super::*;
use crate::{
    model::{ModelCtx, OverlayModel, SharedState, ViewState, state_pool::StatePool},
    provider::EventKinds,
    view::OverlayView,
};
/// A short UI flash, independent of the rate-limited Bell protocol (design §11.6).
const FLASH_SECONDS: f64 = 0.2;
/// Frozen endpoints; values do not change while a renderer is using them.
#[derive(Clone, Default)]
struct Flash {
    /// The first frame's model time, not a renderer's clock sample.
    start: f64,
    /// Zero disables drawing; otherwise a final wake removes the border.
    end: f64,
}
/// Bell interaction owner; unchanged grid output cannot restart its interval.
#[derive(Default)]
pub struct Bell {
    /// Last accepted BEL identity from Session.
    seen: u64,
    /// Mutable endpoint owner remains on the Session side.
    flash: Flash,
    /// Recycles states only after every immutable reader releases them.
    states: StatePool<Flash>,
}
/// Source observation and finite scheduling remain entirely on the model owner.
impl OverlayModel for Bell {
    /// Stable diagnostic name for finite flash work.
    fn name(&self) -> &'static str {
        "visual-bell"
    }
    /// Ordinary text output cannot invalidate or restart this overlay.
    fn wants(&self) -> EventKinds {
        EventKinds(EventKinds::STATUS.0 | EventKinds::GEOMETRY.0 | EventKinds::THEME.0)
    }
    /// The model alone owns animation demands and the final removal wake.
    fn update(&mut self, ctx: &ModelCtx<'_>) {
        let source = &ctx.models.visuals;
        // Only a new accepted fact begins an interval; geometry/theme changes
        // merely redraw its remaining portion at the new surface dimensions.
        if self.seen != source.bell {
            self.seen = source.bell;
            self.flash = Flash {
                start: ctx.time,
                end: ctx.time + FLASH_SECONDS,
            };
        }
        if !source.bell_visual || self.flash.end <= ctx.time {
            // Expiry must publish an empty list once to remove retained border ink.
            self.flash.end = 0.0;
        }
        if self.flash.end > ctx.time {
            ctx.animate_until(self.flash.end);
            // With reduced motion, one final wake clears the otherwise static frame.
            ctx.wake_at(self.flash.end);
        }
    }
    /// Publishing an active interval cannot mutate a previously sealed frame.
    fn view_state(&mut self) -> SharedState {
        self.states.publish(&self.flash)
    }
}
/// Pure drawing half for the short border flash.
pub struct BellView;
/// The painter can be rebuilt at any time from the two immutable endpoints.
impl OverlayView for BellView {
    /// A static reduced-motion border and a fading border share the same endpoint.
    fn build(&mut self, state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let Some(flash) = state.as_any().downcast_ref::<Flash>() else {
            return;
        };
        if flash.end <= ctx.time {
            // A late build must never resurrect a flash whose model already expired.
            return;
        }
        let opacity = if ctx.reduced_motion {
            1.0
        } else {
            // Straight-alpha fades preserve the configured token's own transparency.
            ((flash.end - ctx.time) / (flash.end - flash.start)) as f32
        };
        push(
            out,
            Primitive::Rect(RectSpec {
                rect: ctx.geometry.viewport,
                color: alpha(ctx.theme.color("bellFlash", 0xffffffff), opacity),
                radius: 0.0,
                stroke: 2.0,
            }),
        );
    }
}
/// OSC 9;4 progress reads one source value and costs nothing while unchanged.
pub struct ProgressView;
/// Source-driven progress has no private counter, timer or transport destination.
impl OverlayView for ProgressView {
    /// Draws normal/error/paused progress, with an indeterminate full-width stripe.
    fn build(&mut self, _: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let Some((state, percent)) = ctx.models.visuals.progress else {
            return;
        };
        let token = match state {
            // Error and paused states use existing semantic colors, not hard-coded RGB.
            2 => "zoneError",
            4 => "gutter",
            _ => "accent",
        };
        let extent = if state == 3 {
            // Indeterminate status has no meaningful percentage; show one static stripe.
            1.0
        } else {
            f32::from(percent.min(100)) / 100.0
        };
        rect(
            out,
            Rect {
                origin: [0.0; 2],
                size: [ctx.geometry.viewport.size[0] * extent, 2.0],
            },
            ctx.theme.color(token, 0xffffffff),
            0.0,
        );
    }
}
