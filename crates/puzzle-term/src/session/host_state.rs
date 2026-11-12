//! Copy-only input and animation facts replace synchronous native queries (D-50).
//!
//! A host can coalesce physical samples using these observations, but Session
//! remains authoritative for mode policy and overlay consumption at dispatch.
//! Every value is independent of glyph resources, drawing caches and VT borrows.
//! Comparing the last delivered value avoids per-feed events during steady output.
//! Bare headless parsers have no host observer and never execute this collector.
//! Animation time is the latest injected core turn, not a new wall-clock read.
//! An animation edge is queued before its wake request, so a receiving host can
//! decide refresh pacing without a callback into the owner or a focus race.
use super::{Session, host_events::HostEvent};
use puzzle_text::metrics::CellMetrics;
use puzzle_vt::input::MouseModes;

/// Input observations are plain values, never references into a live terminal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputState {
    /// Device-cell dimensions arrive when the rendering owner supplies metrics.
    pub cell: Option<CellMetrics>,
    /// Live terminal bounds clamp application reports independently of glyph layout.
    pub grid: [usize; 2],
    /// Hosts may use this as a coalescing hint; dispatch rechecks current modes.
    pub mouse: MouseModes,
    /// Whether Shift reserves a gesture for local text selection.
    pub shift_selects: bool,
    /// Physical hover samples may cross regions within the same terminal cell.
    pub overlays: bool,
}

/// Facts change only when their source changes, independently of frame allocation.
impl Session {
    /// Queues a small input-policy value and an animation edge before host wakes.
    pub(super) fn collect_host_state(&mut self) {
        let (columns, rows) = self.terminal.size();
        let state = InputState {
            cell: self.cell_metrics(),
            grid: [columns, rows],
            mouse: MouseModes::from_terminal(&self.terminal),
            shift_selects: self.mouse_shift_selects(),
            overlays: self.has_overlays(),
        };
        if self.host_input != Some(state) {
            // No cell data or strings are cloned: an unchanged flood pays one comparison.
            self.host_input = Some(state);
            self.host_events.push_back(HostEvent::InputState(state));
        }
        // Native find interfaces cannot borrow the session thread's search model.
        // Copy only its small progress value, and emit nothing while it is unchanged.
        let search = self.search_state();
        if self.host_search != Some(search) {
            self.host_search = Some(search);
            self.host_events.push_back(HostEvent::SearchState(search));
        }
        // The latest injected turn includes pointer integration and search work.
        // Before any turn, no animation has begun, so no platform clock is needed.
        let animating = self
            .host_time
            .is_some_and(|now| self.scroll_animation_frame(now));
        if self.host_animation != Some(animating) {
            self.host_animation = Some(animating);
            self.host_events
                .push_back(HostEvent::AnimationChanged(animating));
        }
    }
}
