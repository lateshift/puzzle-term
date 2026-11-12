//! Link source lookup precedes callbacks and still validates presented row versions.
//! Pointer callbacks stay with the Session's model owner throughout frame builds.
//!
//! Hit targets come from the last presented region table. A future build can
//! prepare different targets without changing these interactions. Models need
//! current source values and time, but never fonts, shapes or a builder loan.
use super::*;
use puzzle_overlay::{
    model::ModelCtx,
    provider::{PointerEvent, PointerKind, PointerResult},
};

/// Input follows the same Session-owned model callbacks on native and web hosts.
impl Session {
    /// Routes logical input before application reporting or local selection.
    /// A handled sample must never also enter the application's mouse encoder.
    /// Source models are refreshed on this owner, while geometry stays tied to
    /// the presented table; a concurrent future build cannot alter its targets.
    pub fn route_overlay_pointer(&mut self, event: &PointerEvent, now: Instant) -> PointerResult {
        if self.overlays.is_empty() {
            return PointerResult::default();
        }
        // Before first presentation there are no targets; a nonzero scale still
        // keeps source geometry conversion valid for model callbacks.
        self.overlays.pointer = (event.kind != PointerKind::Leave).then_some(event.point);
        let dpr = self.overlay_source.geometry.dpr.max(f32::EPSILON);
        self.update_overlay_source(now, dpr);
        let source = &self.overlay_source;
        let mut ctx = ModelCtx {
            geometry: &source.geometry,
            time: source.time,
            theme: &source.theme,
            reduced_motion: self.overlay_reduced_motion,
            models: &source.models,
            decorations: &source.decorations,
            requests: &source.requests,
        };
        // Hit selection, enter/leave transitions and capture run on the same owner.
        let result = self.overlays.route_pointer(event, &mut ctx);
        // Typed actions and deadlines apply only after the callback borrow ends.
        self.finish_overlay_requests(now);
        result
    }
    /// Converts a signed device-coordinate sample using the presented frame's geometry.
    /// Device input is grid-relative; adding the presented logical inset gives
    /// surface-relative regions. Signed values preserve dragging beyond the grid.
    pub fn route_overlay_sample(
        &mut self,
        input: crate::pointer::PointerInput,
        now: Instant,
    ) -> PointerResult {
        use crate::vt::input::MouseKind;
        // Wheel routing remains the Session viewport/application policy, not a click.
        let kind = match input.event.kind {
            MouseKind::Move => PointerKind::Move,
            MouseKind::Press => PointerKind::Press,
            MouseKind::Release => PointerKind::Release,
            MouseKind::Wheel => return PointerResult::default(),
        };
        let geometry = &self.overlay_source.geometry;
        let scale = f64::from(geometry.dpr.max(f32::EPSILON));
        // Converting once avoids rounding a logical small button through cell indices.
        // X includes the reserved gutter; Y includes only the outer top padding.
        let event = PointerEvent {
            kind,
            point: std::array::from_fn(|axis| {
                (input.position[axis] / scale) as f32 + geometry.origin()[axis]
            }),
            button: u8::try_from(input.event.button).ok(),
            modifiers: input.event.modifiers.0,
        };
        self.route_overlay_pointer(&event, now)
    }
    /// Adapters may schedule hover independently of application mouse-tracking modes.
    /// Model availability does not depend on the render-side builder's lifetime.
    pub fn has_overlays(&self) -> bool {
        !self.overlays.is_empty()
    }
    /// Queries the last presented cell geometry without borrowing render-side resources.
    pub fn overlay_geometry(&self) -> &puzzle_overlay::geometry::TextGeometry {
        &self.overlay_source.geometry
    }
}
