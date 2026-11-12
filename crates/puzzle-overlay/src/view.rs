//! Renderer-owned drawing context, with no route back to mutable model state.
//!
//! The immutable payload and frozen BufferView describe one published frame.
//! A mutable view may retain prepared shapes and layouts, but losing that cache
//! must never change interaction, source data or animation timing (D-50).
use crate::{draw::DrawList, geometry::TextGeometry, model::ViewState, provider::Theme};
use puzzle_vt::view::BufferView;

/// Frame-only resources; unlike ModelCtx this has drawing tools and no scheduling.
pub struct ViewCtx<'a> {
    /// Placement geometry of the frame being built.
    pub geometry: &'a TextGeometry,
    /// Bounded visible contents from the published snapshot.
    pub buffer: BufferView<'a>,
    /// Injected animation time associated with that snapshot.
    pub time: f64,
    /// Effective semantic UI colors captured with source state.
    pub theme: &'a Theme,
    /// Accessibility policy for endpoint rendering.
    pub reduced_motion: bool,
    /// Rebuildable retained vector shapes, owned by the renderer.
    pub shapes: &'a crate::shapes::ShapeCache,
    /// Font measurement exists only on the frame-building side.
    pub text: Option<&'a dyn crate::text::TextLayouter>,
    /// Session-sampled declarative annotations.
    pub decorations: &'a [crate::decoration::Sample],
    /// Frozen source models, never independently mutated by a view.
    pub models: &'a crate::models::Models,
}
/// Drawing is separate from models so a frame build cannot delay input ownership.
pub trait OverlayView: Send {
    /// Reclaim rebuildable allocations from the preceding list before it clears.
    /// This may retain text/shape scratch only; it cannot change model state or
    /// schedule work. Default views need no recycling callback.
    fn recycle(&mut self, _out: &mut DrawList) {}
    /// Replaces the retained draw list from the pair's immutable state.
    fn build(&mut self, state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList);
}

/// Every floating view can share the same host-UI avoidance policy.
impl ViewCtx<'_> {
    /// Reposition a preferred panel, or omit it when no unobscured placement fits.
    pub fn place(&self, preferred: crate::geometry::Rect) -> Option<crate::geometry::Rect> {
        crate::occlusion::place(
            preferred,
            self.geometry.viewport,
            self.models.occluded_rects.as_deref().unwrap_or(&[]),
        )
    }
}

/// Interactive source UI can move without relocating the underlying annotation.
impl ViewCtx<'_> {
    /// Keep source ink fixed, but never leave a hit target beneath native UI.
    pub fn unoccluded(&self, rect: crate::geometry::Rect) -> bool {
        self.models
            .occluded_rects
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .all(|covered| !rect.overlaps(*covered))
    }

    /// Slide a control left, then down visible source rows; None omits ink and hits.
    /// The caller supplies the source's visible output/gutter rectangle as its limit.
    pub fn place_control(
        &self,
        preferred: crate::geometry::Rect,
        area: crate::geometry::Rect,
    ) -> Option<crate::geometry::Rect> {
        crate::occlusion::anchored(
            preferred,
            area,
            self.geometry.lines.iter().map(|line| line.y),
            self.models.occluded_rects.as_deref().unwrap_or(&[]),
        )
    }
}
