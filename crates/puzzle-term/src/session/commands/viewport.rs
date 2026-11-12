//! Executes viewport commands after interceptors without moving the VT cursor.
//!
//! Scroll and ScrollTo only change presentation. Facts describe the accepted,
//! clamped position and the pill state, so a host never guesses from requests.
//!
//! Negative relative counts move toward older text; positive counts move
//! toward the live tail. The viewport handles cell height and range limits,
//! so this command handler never edits the application's cursor or rows.
//! Absolute navigation uses the same changed-state transaction as input
//! following and clear/reset shortcuts. A bottom request also clears the
//! new-output state even when no additional terminal output is parsed.
//!
//! A command can clamp to its existing position at a document boundary.
//! Such a no-op neither requests a display refresh nor emits a scrolled fact.
//! A changed position requests exactly the normal host frame path; desktop
//! and web still decide when their next refresh can take the snapshot.
//! Observers run after this handler returns, so both position and pill state
//! already describe the committed navigation rather than the old viewport.
//!
//! Host UI exclusions affect centred line navigation, never the VT cursor or size.
//! Top, Bottom and explicit LineTop retain their original alignment semantics.
//! Search supplies its current occupied columns, including a wide-cell spacer.
//! A toolbar on the other side therefore cannot block its usable centre rows.
//! A generic Line request has no match extent and uses the complete row instead.
//! Fold reveal precedes centring so both operations use the same displayed rows.
use super::Handler;
use crate::{
    messages::{Command, Event},
    viewport::ScrollTarget,
};
use puzzle_bus::hooks::HookCtx;
/// Policy-approved navigation borrows the Session-owned model directly.
impl Handler<'_> {
    /// Dispatches the two public navigation commands to their typed model.
    pub(super) fn viewport_command(
        &mut self,
        command: &Command,
        ctx: &mut HookCtx<'_, Command, Event>,
    ) -> bool {
        match command {
            Command::Wheel { delta, pointer } => {
                self.wheel_command(*delta, *pointer, ctx);
                true
            }
            Command::Scroll(rows) => {
                let before = (self.viewport.scroll_px(), self.viewport.new_output());
                self.viewport.scroll(self.terminal, *rows);
                self.viewport_changed(before, ctx);
                true
            }
            Command::ScrollTo(target) => {
                self.move_viewport(*target, ctx);
                true
            }
            _ => false,
        }
    }
    /// Navigation publishes the resulting state, including return-to-bottom.
    pub(super) fn move_viewport(
        &mut self,
        target: ScrollTarget,
        ctx: &mut HookCtx<'_, Command, Event>,
    ) {
        // Match navigation selects an occurrence before resolving its centre line.
        // Using scrollbar positions alone would skip multiple hits on one row.
        if let ScrollTarget::Match(delta) = target {
            self.search_move(delta, ctx);
            return;
        }
        if let ScrollTarget::Line(line) | ScrollTarget::LineTop(line) = target {
            self.reveal_source_line(line);
        }
        let before = (self.viewport.scroll_px(), self.viewport.new_output());
        if let ScrollTarget::Line(line) = target {
            let row = super::super::occlusion::centre_row(
                self.frame_policy.as_ref(),
                self.occluded_rects,
                self.terminal.size().0,
                self.terminal.size().1,
                self.search
                    .fills
                    .iter()
                    .find(|fill| fill.current && fill.range.start.line == line)
                    .map(|fill| fill.range),
            );
            self.viewport.centre_line_at(self.terminal, line, row);
        } else {
            self.viewport.navigate(self.terminal, target);
        }
        self.viewport_changed(before, ctx);
    }
    /// Requests a frame only when scrolling actually changed presentation.
    pub(super) fn viewport_changed(
        &mut self,
        before: (f64, bool),
        ctx: &mut HookCtx<'_, Command, Event>,
    ) {
        if before.0 != self.viewport.scroll_px() {
            ctx.publish(Event::ViewportScrolled(self.viewport.scroll_px()));
            *self.needs_frame = true;
            self.services.waker.request_frame();
        }
        if before.1 != self.viewport.new_output() {
            ctx.publish(Event::NewOutputChanged(self.viewport.new_output()));
        }
    }
}

/// Revealing hidden source precedes centring it, including API search navigation.
impl Handler<'_> {
    /// Existing folds only become less restrictive; no new authority is granted.
    pub(super) fn reveal_source_line(&mut self, line: puzzle_vt::LineId) {
        // Reveal before computing the centred display index, not after navigation.
        // This changes only presentation: copy/search still use the original grid.
        if self.osc.reveal_line(self.terminal, line) {
            self.viewport.update_projection(self.terminal, self.osc);
            self.viewport.reconcile(self.terminal, false);
            // Refresh source markers even though the projection was reconciled here.
            self.selection_seen = true;
            *self.needs_frame = true;
            self.services.waker.request_frame();
        }
    }
}
