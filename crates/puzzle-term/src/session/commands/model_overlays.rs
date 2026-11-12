//! Typed model operations and Session query input execute after bus policy.
//!
//! Session-owned model callbacks return typed actions before bus dispatch. Thumb/track
//! operations reuse M8 navigation exactly.
//!
//! PageTrack values are device y positions; DragThumb values are normalized
//! travel fractions. The Action enum keeps their distinct operations explicit
//! even though both payloads are f64. M8 applies the clamping and row snapping.
//! Comparing the original following/position pair publishes normal scroll facts.
//! A pill click shares the public Bottom target and clears new-output state.
//!
//! Query input is tested before application encoding. Changed committed text
//! replaces the existing incremental search query; pump does not scan history.
//! Composition/caret motion only requests new field ink. Enter navigates already
//! discovered results. Escape and the close button cancel results and the mode.
//! Non-input commands continue through the normal handler while the field is open,
//! so terminal output, appearance, focus and lifecycle processing remain active.
//! Every consumed input avoids the transport and its scrolling-on-input policy.
//! Keys in an open native query mode
//! replace the M8 search query and never reach application encoders.
//!
//! Help and text search share an editor, but not a search operation. Its kind
//! selects the recipient of committed text: the M8 engine or table filtering.
//! Enter in shortcut help therefore does nothing; it cannot navigate buffer hits.
//! Closing help keeps existing M8 results intact, whereas closing search clears
//! them through the ordinary Search command handler. Provisional IME text only
//! changes the field presentation in either mode and never becomes a query here.
//! Opening help clears the old filter and candidate together, preventing stale
//! text from a previous search field appearing under a different purpose label.
//! The handler requests a frame even when no application bytes were emitted;
//! a local input mode cannot rely on the child to echo its edits or dismissal.
use super::*;
use puzzle_overlay::models::Action;
impl Handler<'_> {
    /// Applies one model action, preserving normal viewport/search facts and scheduling.
    pub(super) fn overlay_action(&mut self, action: Action, ctx: &mut HookCtx<'_, Command, Event>) {
        match action {
            // Opening is resolved by Session before command policy, never by a view.
            Action::OpenHoveredLink | Action::CopyZoneOutput(_) => return,
            Action::DragThumb(fraction) | Action::PageTrack(fraction) => {
                let before = (self.viewport.scroll_px(), self.viewport.new_output());
                if matches!(action, Action::DragThumb(_)) {
                    self.viewport.drag_thumb_to(self.terminal, fraction);
                } else {
                    self.viewport.page_towards(self.terminal, fraction);
                }
                self.viewport_changed(before, ctx);
            }
            Action::Bottom => self.move_viewport(crate::viewport::ScrollTarget::Bottom, ctx),
            Action::SearchStep(delta) => self.search_move(delta, ctx),
            // Resolve the presented generation locally; stale regions cannot affect a replacement.
            Action::Notification { serial, activate } => self.notifications.region_feedback(
                serial,
                if activate {
                    crate::notification::Feedback::Activated
                } else {
                    crate::notification::Feedback::Closed
                },
                self.services.notifier.as_mut(),
                &mut self.terminal.replies,
            ),
            Action::SearchClose => {
                self.query.open = false;
                self.query.preedit.clear();
                self.query.dirty = true;
                self.search_command(&crate::search::SearchCommand::Clear, ctx);
            }
        }
        // A changed current result can be on the same row; its field still needs a rebuild.
        *self.needs_frame = true;
        self.services.waker.request_frame();
    }
    /// Consumes keys and committed/preedit text only while the native query mode is open.
    pub(super) fn query_input(
        &mut self,
        command: &Command,
        ctx: &mut HookCtx<'_, Command, Event>,
    ) -> bool {
        use super::super::model_overlays::query::Edit;
        match self.query.edit(command) {
            Edit::Pass => return false,
            Edit::Close => {
                if self.query.kind == super::super::model_overlays::query::Kind::Search {
                    self.overlay_action(Action::SearchClose, ctx);
                } else {
                    self.query.open = false;
                    self.query.preedit.clear();
                    self.query.dirty = true;
                }
            }
            Edit::Step(delta)
                if self.query.kind == super::super::model_overlays::query::Kind::Search =>
            {
                self.search_move(delta, ctx)
            }
            Edit::Step(_) => {}
            Edit::Changed
                if self.query.kind == super::super::model_overlays::query::Kind::Search =>
            {
                self.search_command(
                    &crate::search::SearchCommand::Find {
                        query: self.query.text.clone(),
                        options: Default::default(),
                    },
                    ctx,
                )
            }
            Edit::Consumed | Edit::Changed => {}
        }
        *self.needs_frame = true;
        self.services.waker.request_frame();
        true
    }
}

/// Both query fields share Session's local editor, with no general overlay focus.
impl Handler<'_> {
    /// Opens shortcut filtering without replacing the incremental text search.
    pub(super) fn help_action(&mut self, action: &crate::keybind::Action) -> bool {
        if !matches!(action, crate::keybind::Action::ShowKeybindings) {
            return false;
        }
        self.query.kind = super::super::model_overlays::query::Kind::Bindings;
        self.query.open = true;
        self.query.text.clear();
        self.query.preedit.clear();
        self.query.cursor = 0;
        self.query.dirty = true;
        *self.needs_frame = true;
        self.services.waker.request_frame();
        true
    }
}
