//! Executes currently available shortcuts after the public policy chain.
//! Selection shortcuts enqueue public Select requests for policy approval.
//! Clear/reset actions flag document changes before model reconciliation.
//!
//! Sending actions queue ordinary Text/Paste commands, so clipboard and
//! input policies remain observable. Screen edits bypass the PTY parser:
//! its partially received escape sequence still belongs to the child.
//!
//! The typed action catalogue includes operations which belong to later
//! milestones. Those operations remain inert until their features exist;
//! performable consumption is decided before this handler is entered.
//! Clipboard reads happen only as a user shortcut, never from OSC output.
//! An asynchronous clipboard provider returns None now and supplies Paste
//! later; a synchronous desktop provider submits the same Paste command.
//! Screen shortcuts operate directly on VT state. Injecting synthetic CSI
//! into a partially received child CSI would corrupt its parser state.
//! Zoom and appearance need the shared settings owner. They are returned
//! as typed operations rather than giving Session a platform callback.
//! The all flag is retained for a host with several terminals sharing a store.
//! One-window desktop currently has just one target for that broadcast.
use super::Handler;
use crate::{
    keybind::Action,
    messages::{Command, Event},
};
use puzzle_bus::{Origin, hooks::HookCtx};

/// Actions use the same exit and transport rules as direct input.
impl Handler<'_> {
    /// Executes core actions or lends settings operations back to the host.
    pub(super) fn binding_action(
        &mut self,
        action: &Action,
        all: bool,
        ctx: &mut HookCtx<'_, Command, Event>,
    ) {
        if self.help_action(action)
            || self.scroll_action(action, ctx)
            || self.selection_action(action, ctx)
            || self.copy_action(action, ctx)
        {
            return;
        }
        let origin = ctx.origin().unwrap_or(Origin::User);
        match action {
            // UI request is a separate typed command, preserving policy interception.
            // Desktop keys do not reach this arm until the M9 bar is available.
            Action::Search => ctx.submit(
                Command::Search(crate::search::SearchCommand::Request),
                origin,
            ),
            Action::Paste => match self.services.clipboard.request_text() {
                Ok(Some(text)) => ctx.submit(Command::Paste(text), origin),
                Ok(None) => {}
                Err(error) => tracing::warn!(event="session.clipboard_read_failed",%error),
            },
            Action::SendText(text) => ctx.submit(Command::Text(text.clone()), origin),
            Action::Csi(text) => ctx.submit(Command::Text(format!("\x1b[{text}")), origin),
            Action::Esc(text) => ctx.submit(Command::Text(format!("\x1b{text}")), origin),
            Action::ClearScreen | Action::ClearScrollback | Action::Reset => {
                match action {
                    Action::ClearScreen => self.terminal.clear_preserving_line(),
                    Action::ClearScrollback => self.terminal.clear_history(),
                    _ => {
                        self.terminal.reset();
                        // RIS unregisters native dragging even when triggered by a shortcut.
                        // Forward disposal now: no parser feed follows this direct state change.
                        self.forward_drop_reset();
                    }
                }
                self.selection_source_changed = true;
                self.move_viewport(crate::viewport::ScrollTarget::Bottom, ctx);
                *self.needs_frame = true;
                self.services.waker.request_frame();
            }
            Action::Zoom(_)
            | Action::ZoomReset
            | Action::SetAppearance(_)
            | Action::ToggleAppearance
            | Action::Profile(_)
            | Action::ReloadSettings => {
                // The caller applies these after pump, avoiding a callback
                // from the neutral core into a borrowed platform store.
                self.binding_actions.push((action.clone(), all));
            }
            Action::Ext(name) => self.extension_action(name, all, ctx),
            Action::Ignore | Action::Unbind => {}
            _ => tracing::debug!(
                action = action.info().name,
                available = action.info().available,
                "shortcut feature not yet available"
            ),
        }
    }
}

/// Keyboard navigation uses ordinary commands so host policies can veto it.
impl Handler<'_> {
    /// Converts page counts using rows minus one, preserving one context row.
    /// Returns false for other actions so clipboard/settings dispatch still runs.
    /// Navigation preserves its origin and remains vetoable as Scroll/ScrollTo.
    fn scroll_action(&mut self, action: &Action, ctx: &mut HookCtx<'_, Command, Event>) -> bool {
        use crate::viewport::ScrollTarget;
        // Page fractions are user input, already finite-validated by the parser.
        let command = match action {
            Action::ScrollLines(rows) => Command::Scroll(f64::from(*rows)),
            Action::ScrollPages(pages) => {
                Command::Scroll(f64::from(*pages) * self.terminal.size().1.saturating_sub(1) as f64)
            }
            Action::ScrollTop => Command::ScrollTo(ScrollTarget::Top),
            Action::ScrollBottom => Command::ScrollTo(ScrollTarget::Bottom),
            // Query navigation shares the viewport target used by public APIs.
            Action::SearchNext => Command::ScrollTo(ScrollTarget::Match(1)),
            Action::SearchPrev => Command::ScrollTo(ScrollTarget::Match(-1)),
            Action::JumpToPrompt(delta) => Command::ScrollTo(ScrollTarget::Prompt(*delta)),
            _ => return false,
        };
        // Preserve the source instead of claiming these requests came from output.
        ctx.submit(command, ctx.origin().unwrap_or(Origin::User));
        true
    }
}

/// Source actions return only UI effects and cannot synthesize application input.
impl Handler<'_> {
    /// Keep follow-up navigation in the current bus drain, so an interceptor
    /// cycle remains bounded by the ordinary cascade limit instead of recursion.
    fn extension_action(&mut self, name: &str, all: bool, ctx: &mut HookCtx<'_, Command, Event>) {
        // Wire/API dispatch remains inert even when it spells a registered name.
        // Only the input policy's real User origin enters this source callback.
        if ctx.origin() != Some(Origin::User) {
            return;
        }
        // The callback can inspect source but has no writer or recursive bus.
        // Its result joins this drain, keeping the existing cascade bound intact.
        let effect =
            self.osc
                .user_action(None, crate::osc::OscUserAction::Named(name), self.terminal);
        let Some(effect) = effect else {
            // A native host may own a different registered extension action.
            self.binding_actions.push((Action::Ext(name.into()), all));
            return;
        };
        // Meaning is a fact, not a command that could synthesize application input.
        if let Some(event) = effect.event {
            ctx.publish(Event::Ext(event));
        }
        // Static UI requests one frame; the source never starts a background timer.
        if effect.changed {
            *self.needs_frame = true;
            self.services.waker.request_frame();
        }
        if let Some(line) = effect.scroll_to {
            // Scroll policies retain their normal priority and can veto this target.
            ctx.submit(
                Command::ScrollTo(crate::viewport::ScrollTarget::Line(line)),
                Origin::User,
            );
        }
    }
}
