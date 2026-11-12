//! Session-owned overlay interaction and immutable state publication (D-50).
//!
//! Models can request scheduling and mutate interaction state. Their contexts
//! deliberately expose no text layouter, shape cache or drawing operation;
//! preparing ink is the independently owned view's responsibility.
use crate::{draw::RegionId, geometry::TextGeometry, provider::*};
use std::{any::Any, sync::Arc};

/// Type-erased immutable payload; the concrete type belongs to one provider pair.
pub trait ViewState: Any + Send + Sync {
    /// Enables a view to recover its own strongly typed state without unsafe casts.
    fn as_any(&self) -> &dyn Any;
}
/// Any immutable, sendable value can cross the model/view seam.
impl<T: Any + Send + Sync> ViewState for T {
    /// Returns the original payload, never the smart pointer enclosing it.
    fn as_any(&self) -> &dyn Any {
        self
    }
}
/// Immutable state can be retained by all three rotating snapshots without deep copies.
pub type SharedState = Arc<dyn ViewState>;

/// Input and scheduling context borrowed only while the Session invokes a model.
pub struct ModelCtx<'a> {
    /// Geometry returned with the last presented frame, not a future frame's build.
    pub geometry: &'a TextGeometry,
    /// Seconds on the Session's injected overlay clock.
    pub time: f64,
    /// Effective semantic UI tokens.
    pub theme: &'a Theme,
    /// Accessibility policy suppresses animation deadlines.
    pub reduced_motion: bool,
    /// Search, scroll and selection data owned by the Session.
    pub models: &'a crate::models::Models,
    /// Sampled declarative annotations, independent of renderer resources.
    pub decorations: &'a [crate::decoration::Sample],
    /// Scheduling and typed actions are consumed by the Session after the callback.
    pub requests: &'a Requests,
}
/// Only models may schedule work; views derive animation output from time and state.
impl ModelCtx<'_> {
    /// Requests the earliest future update without a recurring idle timer.
    pub fn wake_at(&self, deadline: f64) {
        if deadline.is_finite() && deadline > self.time {
            let previous = self.requests.wake.get();
            self.requests.wake.set(if previous == 0.0 {
                deadline
            } else {
                previous.min(deadline)
            });
        }
    }
    /// Requests a single further frame after a state-changing callback.
    pub fn request_frame(&self) {
        self.requests.frame.set(true);
    }
    /// Keeps the model dirty through an animation's final endpoint.
    pub fn animate_until(&self, end: f64) {
        if !self.reduced_motion && end.is_finite() && end > self.time {
            self.requests.until.set(self.requests.until.get().max(end));
        }
    }
}
/// Mutable interaction belongs to the Session even during a long render build.
pub trait OverlayModel: Send {
    /// Stable diagnostic name for this registration.
    fn name(&self) -> &'static str;
    /// Coarse source changes which invalidate the published state.
    fn wants(&self) -> EventKinds;
    /// Advances source-driven state and timers only when dirty or animating.
    fn update(&mut self, _ctx: &ModelCtx<'_>) {}
    /// Creates a new immutable value only on an actual model update (P11).
    fn view_state(&mut self) -> SharedState;
    /// Replaces a source-owned immutable publication when this model is a thin
    /// interaction adapter. Ordinary models keep producing their own state.
    /// Returning false refuses the payload without invalidating the current view.
    fn accept_state(&mut self, _state: SharedState) -> bool {
        false
    }
    /// Handles presented regions without obtaining a renderer or font borrow.
    fn on_pointer(
        &mut self,
        _event: &PointerEvent,
        _region: RegionId,
        _ctx: &mut ModelCtx<'_>,
    ) -> Handled {
        Handled::No
    }
}

/// Draw-only extensions still have a Session-owned dirty/scheduling half.
pub struct StaticModel {
    /// Registration name is sampled once, independent of render callbacks.
    pub name: &'static str,
    /// Coarse interests are owned by the interaction registry.
    pub wants: EventKinds,
    /// The unit payload is allocated once, rather than on every dirty source frame.
    state: SharedState,
}
/// No private source or interaction state is needed by a static scene.
impl OverlayModel for StaticModel {
    /// Returns the registration's stable diagnostic label.
    fn name(&self) -> &'static str {
        self.name
    }
    /// Source interests stay with the model rather than the view.
    fn wants(&self) -> EventKinds {
        self.wants
    }
    /// The unit payload carries no mutable render or source state.
    fn view_state(&mut self) -> SharedState {
        self.state.clone()
    }
}

/// Payload buffers are reused only when no immutable publication refers to them.
pub mod state_pool;

/// Registration samples interests once and creates the immutable unit payload.
impl StaticModel {
    /// Draw-only state never changes, even when the view must rebuild for a source event.
    pub fn new(name: &'static str, wants: EventKinds) -> Self {
        Self {
            name,
            wants,
            state: Arc::new(()),
        }
    }
}
