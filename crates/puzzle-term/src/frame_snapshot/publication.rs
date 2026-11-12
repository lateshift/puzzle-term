//! Immutable source values carried by a caller-owned frame slot (D-50).
//!
//! Registrations move to the renderer once; every slot keeps its own immutable
//! model versions. The renderer must accept registrations even from a skipped
//! publication before returning that slot to the Session. Source vectors keep
//! their capacity across circulation and never contain scheduling capabilities.
use puzzle_overlay::{buffer::BufferSnapshot, provider::Theme, view_registry::OverlaySnapshot};

/// Overlay source payload, without model callbacks or rendering caches.
#[derive(Default)]
pub(crate) struct OverlayPayload {
    /// Immutable declaration samples share their underlying definitions.
    pub decorations: Vec<puzzle_overlay::decoration::Sample>,
    /// Search, selection and scrollbar values sampled at the same boundary.
    pub models: puzzle_overlay::models::Models,
    /// Visible anchor metadata, never a reference into live history.
    pub buffer: Option<BufferSnapshot>,
    /// Semantic UI tokens retain their allocation across slot circulation.
    pub theme: Theme,
    /// Seconds on the Session's injected animation clock.
    pub time: f64,
    /// Views jump to animation endpoints when accessibility requires it.
    pub reduced_motion: bool,
    /// Model versions and one-time view ownership transfers.
    pub snapshot: OverlaySnapshot,
    /// Pointer anchors belong to this publication rather than later input.
    pub pointer: Option<[f32; 2]>,
}
