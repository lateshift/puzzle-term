//! Session-owned overlay source and scheduling state, independent of the frame builder.
//!
//! Mutable callbacks read current source models but the last presented geometry.
//! Source buffers retain capacity across updates. Immutable view states and
//! frozen source copies are the only values handed to the drawing owner.
use puzzle_overlay::{
    decoration::Sample,
    geometry::TextGeometry,
    models::Models,
    provider::{Requests, Theme},
};
/// Model callbacks require no text context, shape cache or renderer ownership.
#[derive(Default)]
pub(crate) struct Source {
    /// Geometry is committed only when its frame is reported presented.
    pub geometry: TextGeometry,
    /// Search, scroll and selection inputs are sampled from the Session owner.
    pub models: Models,
    /// Declarative annotation definitions share their immutable allocations.
    pub decorations: Vec<Sample>,
    /// Effective semantic colors at the latest source update.
    pub theme: Theme,
    /// Callback requests are consumed by Session scheduling, never by a view.
    pub requests: Requests,
    /// Injected seconds since the overlay epoch.
    pub time: f64,
}
