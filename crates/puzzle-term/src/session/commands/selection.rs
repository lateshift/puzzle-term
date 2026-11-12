//! Selection mutations pass the same interceptor chain as input (§5.3).
//!
//! The handler owns no separate selection: it borrows Session's VT engine.
//! Expanded ranges become observable facts only after validation succeeds.
//! Shortcut actions submit Select so policies can rewrite or veto the request.
use super::Handler;
use crate::{
    keybind::Action,
    messages::{Command, Event},
    selection::SelectionCommand,
};
use puzzle_bus::{Origin, hooks::HookCtx};
/// Mutates borrowed Session state only after the bus policy chain accepts the request.
impl Handler<'_> {
    /// Applies one selection command without altering application cells/cursor.
    pub(super) fn select(
        &mut self,
        command: SelectionCommand,
        ctx: &mut HookCtx<'_, Command, Event>,
    ) {
        // Invalid endpoints preserve the current range and produce no selection event.
        // A real range edge wakes once; it does not mark application rows damaged.
        if self.selection.apply(self.terminal, command) {
            // Geometry is assembled next refresh; cached glyph rows remain valid.
            ctx.publish(Event::SelectionChanged(self.selection.range()));
            *self.needs_frame = true;
            self.services.waker.request_frame();
            // Pump refreshes marker sources after the complete accepted cascade.
            self.selection_seen = true;
        }
    }
    /// Converts selection shortcuts into public, policy-approved requests.
    pub(super) fn selection_action(
        &mut self,
        action: &Action,
        ctx: &mut HookCtx<'_, Command, Event>,
    ) -> bool {
        // Selection actions remain interceptable rather than bypassing Select.
        let request = match action {
            Action::SelectAll => SelectionCommand::All,
            // Pointer position is optional; the API/keyboard fallback selects latest output.
            Action::SelectCommandOutput => SelectionCommand::CommandOutput(*self.selection_pointer),
            _ => return false,
        };
        // Preserve the originating user/API identity through the cascade.
        ctx.submit(
            Command::Select(request),
            ctx.origin().unwrap_or(Origin::User),
        );
        // The action is consumed even if absent shell marks leave the selection unchanged.
        true
    }
}
