//! Session viewport reconciliation and observable state transitions (§7.5).
//!
//! Reconciliation is source-driven: output, input and resize call it before
//! presentation. The frame path reads the resulting document index without
//! repeating scrollback scans. The bus publishes facts only when state changes.
//!
//! BufferView still describes the application cursor and cells in VT storage.
//! Session attaches only a presentation range to the borrowed view. A host
//! reading buffer.viewport therefore sees the same historical rows that the
//! snapshot/frame path draws, without moving the program's screen or cursor.
//!
//! Track geometry is derived from the selected device-cell measurements and
//! grid inset. It is model data until M9 supplies the overlay API; no rects,
//! hover regions or fade timers are created by these geometry updates.
//! A ring change invalidates normalized marker fractions even if a prompt's
//! LineId stayed identical, because its index and total document length moved.
//!
//! Event construction remains lazy inside Bus. No subscriber means publishing
//! a viewport state edge is only a mask check. Marker vectors retain their
//! allocation when source changes, and normal idle frame reads borrow them.
//! Absolute API navigation enters interceptors just like user wheel commands.
//! A page can therefore veto navigation without a separate callback path.
use super::Session;
use crate::{
    messages::Event,
    viewport::{ScrollTarget, Viewport},
};
/// Session owns the model; platform code only reads it or submits commands.
impl Session {
    /// Hosts use this edge to bypass only output settling at their refresh.
    pub fn scroll_animation_frame(&self, now: web_time::Instant) -> bool {
        self.viewport.animation_frame(now)
            || self.pointer.animating(&self.terminal, &self.viewport)
            || self.search.working()
    }
    /// Keeps an accessibility preference separate from persisted settings.
    pub fn set_reduced_motion(&mut self, enabled: bool) {
        // Retain the smooth setting so disabling accessibility restores user intent.
        self.viewport.reduced_motion(enabled);
        self.overlay_reduced_motion = enabled;
        // Accessibility policy belongs to the same frame as overlay animation time.
        self.overlay_changed(puzzle_overlay::provider::EventKinds::THEME);
    }
    /// Advances the injected clock before either direct or external snapshots.
    pub(super) fn advance_scroll(&mut self, now: web_time::Instant) {
        // Sampling can reach the live tail and clear the pill as well as move rows.
        let before = (self.viewport.scroll_px(), self.viewport.new_output());
        // An outside stationary pointer integrates only at these display turns.
        if let Some(request) = self
            .pointer
            .advance(&mut self.terminal, &mut self.viewport, now)
        {
            self.submit(
                crate::messages::Command::Select(request),
                puzzle_bus::Origin::User,
            );
            self.pump(now);
        }
        if before.0 != self.viewport.scroll_px() || self.viewport.advance(&mut self.terminal, now) {
            self.needs_frame = true;
            // Viewport-dependent callbacks observe the new fractional position once.
            self.overlay_changed(puzzle_overlay::provider::EventKinds::SCROLL);
            self.bus
                .publish(Event::ViewportScrolled(self.viewport.scroll_px()));
        }
        // Observers see the final accepted position on this turn.
        if before.1 != self.viewport.new_output() {
            self.bus
                .publish(Event::NewOutputChanged(self.viewport.new_output()));
        }
        if before.0 != self.viewport.scroll_px() || before.1 != self.viewport.new_output() {
            // Deliver the sampled facts before the frame copies its visible range.
            self.pump(now);
        }
    }
    /// Only a moving segment requests refreshes; silence waits on one timer.
    pub(super) fn schedule_scroll(&mut self, now: web_time::Instant) {
        if self.viewport.animating(now) || self.pointer.animating(&self.terminal, &self.viewport) {
            // A refresh request is an edge, not a repeating platform timer.
            self.needs_frame = true;
            self.services.waker.request_frame();
        } else if let Some(deadline) = self.viewport.snap_deadline() {
            // Pixel input stays idle during the 150-ms silence interval.
            self.services.waker.wake_at(deadline);
        }
    }
    /// Borrows retained rows without copying screen contents.
    #[must_use]
    pub fn buffer(&self) -> crate::vt::view::BufferView<'_> {
        let buffer = self.terminal.buffer();
        let ring = &self.terminal.screen().ring;
        let top = self.viewport.top_index();
        // Include both clipped edges so contents queries agree with presentation.
        if let (Some(first), Some(last)) = (
            ring.retained(top),
            self.viewport
                .visible_count(self.terminal.size().1)
                .checked_sub(1)
                .and_then(|row| self.viewport.source_row(row))
                .and_then(|row| ring.retained(row)),
        ) {
            buffer.with_viewport(crate::vt::view::Viewport {
                first: first.id,
                last: last.id,
                fractional_px: self.viewport.device_shift(),
            })
        } else {
            buffer
        }
    }

    /// Borrows viewport geometry and cached scrollbar markers for host UI.
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }
    /// Reconciles terminal changes and publishes state only at transitions.
    pub(super) fn reconcile_viewport(&mut self, output: bool) {
        let before = (self.viewport.scroll_px(), self.viewport.new_output());
        if self.viewport.context_changed(&self.terminal) {
            // Screen-local bells cannot be transferred to equal IDs in another grid.
            for handle in self.bells.drain(..) {
                self.terminal.tracked.release(handle);
            }
        }
        // Fold updates can originate in pointer routing, keybindings or a search
        // reveal inside this bus cascade. Consume one geometry edge after all
        // of those operations, so every provider sees the final displayed rows.
        // Ordinary sessions have no projection cache and take only the empty guard.
        self.viewport.update_projection(&self.terminal, &self.osc);
        let projected = self.viewport.take_projection_change();
        if projected {
            // A user omission/reveal changes geometry even at the same scroll offset.
            self.needs_frame = true;
            self.services.waker.request_frame();
            self.overlay_changed(puzzle_overlay::provider::EventKinds::GEOMETRY);
        }
        let ring_changed = projected || self.viewport.scroll_total_changed(&self.terminal);
        self.viewport.reconcile(&mut self.terminal, output);
        if let Some(builder) = &self.frame_policy {
            let (columns, rows) = self.terminal.size();
            self.viewport
                .scrollbar
                .set_track(builder.scrollbar_track(columns, rows));
        }
        if ring_changed {
            self.refresh_scrollbar();
        }
        if before.0 != self.viewport.scroll_px() {
            self.overlay_changed(puzzle_overlay::provider::EventKinds::SCROLL);
            self.bus
                .publish(Event::ViewportScrolled(self.viewport.scroll_px()));
        }
        if before.1 != self.viewport.new_output() {
            self.bus
                .publish(Event::NewOutputChanged(self.viewport.new_output()));
        }
    }
    /// Rebuilds source markers after a shell boundary, bell or ring change.
    pub(super) fn refresh_scrollbar(&mut self) {
        self.overlay_changed(puzzle_overlay::provider::EventKinds::QUERY);
        // Resolve tracked bell lines after reflow; Gone never becomes a fresh mark.
        // Reuse the scratch vector instead of allocating a new list per output.
        self.bell_lines.clear();
        self.bell_lines
            .extend(self.bells.iter().filter_map(
                |handle| match self.terminal.tracked.get(*handle) {
                    crate::vt::tracked::PositionState::Present { line, .. } => Some(line),
                    crate::vt::tracked::PositionState::Gone => None,
                },
            ));
        // Marker sources are refreshed after selection edges, not during assembly.
        // Keep the selection’s retained endpoints even when they lie offscreen.
        // M9 will paint this model; no scrollbar geometry enters the frame yet.
        self.viewport.scrollbar.update(
            &self.terminal,
            &self.search.lines,
            // Numeric endpoints describe document extent; the model resolves fractions.
            // There is no selected-text allocation on this source-update path.
            self.selection.range().map(|r| (r.start.line, r.end.line)),
            &self.bell_lines,
        );
        self.viewport.project_markers(self.terminal.size().1);
    }
    /// Requests absolute navigation through the ordinary bus policy lane.
    pub fn scroll_to(&mut self, target: ScrollTarget) {
        self.submit(
            crate::messages::Command::ScrollTo(target),
            puzzle_bus::Origin::Api,
        );
    }
}

/// Browser input queries share the exact destination used by accepted Wheel commands.
impl Session {
    /// True when this finite nonzero gesture has an application or movable local view.
    /// A sub-row application delta is consumed because its fraction is retained.
    /// At local history boundaries the embedding page keeps its ordinary scrolling.
    pub fn wheel_consumable(
        &self,
        delta: crate::viewport::ScrollInput,
        pointer: crate::vt::input::MouseEvent,
    ) -> bool {
        // This query cannot consume fractional steps: the command owns that ledger.
        // Browser default ownership still accepts sub-row input for an application.
        let amount = match delta {
            crate::viewport::ScrollInput::Pixels(n) | crate::viewport::ScrollInput::Rows(n) => n,
        };
        if !amount.is_finite() || amount == 0.0 {
            return false;
        }
        // EOF disables protocol routes but leaves retained primary history inspectable.
        if super::wheel::delivery(&self.terminal, pointer).is_some() {
            return self.exited.is_none();
        }
        // Only applications accept sideways scrolling; history is vertical.
        if pointer.button >= 66 {
            return false;
        }
        // Distance above the tail increases for an upward (negative) gesture.
        // Test the direction against real retained extent, avoiding page-scroll theft.
        let limit = self.viewport.display_history(&self.terminal) as f64
            * f64::from(self.viewport.row_height());
        if amount < 0.0 {
            self.viewport.scroll_px() < limit
        } else {
            self.viewport.scroll_px() > 0.0
        }
    }
}
