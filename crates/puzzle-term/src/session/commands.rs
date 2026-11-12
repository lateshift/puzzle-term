//! Typed command execution at the session and transport boundary (§5.3).
//! Selection requests mutate only the borrowed model, not terminal cells.
//! Pump later reconciles source mutations and refreshes selection markers.
//!
//! Bus policies run before this handler. Input is encoded from the current
//! VT modes and reaches the transport during the same pump; a display frame
//! is never part of that path. A trusted ChildExited command arrives only
//! after the desktop parser drains EOF, and changes the later input rule.
//! The matcher is borrowed separately from the terminal handler, so its
//! pending sequence can be inspected without shared mutable ownership.
//! Input-generated actions queue host work until Session's borrow ends;
//! the settings store is never called recursively under this drain.
//! Encoder writes still complete before pump returns, without a frame.
//! Clipboard completion does not run native work inside the command drain.
//! MIME commands issue one host effect, while later owner messages supply results.
//! After interceptors finish, an unissued request receives denial instead of hanging.
//! The same finalization reconciles reset epochs and bounded host deadlines.
//! Replies then flush through the existing transport order, not a second writer.
//! User input joins the same producer FIFO as program replies.
use super::theme::{ThemeState, install};
use super::{OnExit, Session, exit};
use crate::{
    frame_builder::policy::FramePolicy,
    messages::{Command, Event, ExitInfo},
    platform::Services,
    theme::Appearance,
    vt::Terminal,
};
use puzzle_bus::{
    Origin,
    hooks::{BorrowedPolicy, CommandKinds, HookCtx},
};
/// Shortcut execution uses the same policy-approved handler as input.
mod bindings;
/// Selection copy enters ClipboardWrite so all host policies retain control.
mod copy;
/// Byte encoders stay independent of presentation mutations.
mod input;
/// Query editing and typed built-in navigation share the model owner.
mod model_overlays;
/// Shared gestures retain signed coordinates and policy-approved selection.
mod pointer;
/// Queries and match navigation share the policy-approved model.
mod search;
/// Expanded selection requests keep glyph caches unchanged.
mod selection;
/// Relative and absolute navigation, shared with later wheel routing.
mod viewport;
/// Tracking and alternate-scroll conversion share one policy-approved wheel lane.
mod wheel;
use web_time::Instant;

/// Mutable session parts borrowed while the bus owns its command queue.
struct Handler<'a> {
    /// Optional source actions stay inside the same bus cascade as their keybinding.
    osc: &'a mut crate::osc::Registry,
    /// Clipboard permission and native correlation owner, never a platform handle.
    clipboard: &'a mut crate::clipboard::State,
    /// Host intent is queued only after policy; no native callback runs in this drain.
    host_events: &'a mut std::collections::VecDeque<super::host_events::HostEvent>,
    /// Same rate/lifetime policy serves every notification origin.
    notifications: &'a mut crate::notification::State,
    /// VT supplies active keyboard modes and receives the held notice.
    terminal: &'a mut Terminal,
    /// Local navigation shares the same policy-approved command drain.
    viewport: &'a mut crate::viewport::Viewport,
    /// Selection engine shares terminal tracking through this same drain.
    selection: &'a mut crate::selection::SelectionState,
    /// Incremental query work belongs to the same document.
    search: &'a mut crate::search::SearchModel,
    /// Native query input belongs to Session, never an overlay keyboard callback.
    query: &'a mut super::model_overlays::query::Query,
    /// Web hosts own their search interface and receive SearchRequested instead.
    web_search: bool,
    /// Explicit host query UI, independent of URL and native key policy.
    host_search: bool,
    /// Immutable host UI metadata affects floating panels and line navigation only.
    occluded_rects: &'a [puzzle_overlay::geometry::Rect],
    /// Schemes are validated after all OpenLink interceptors.
    link_policy: &'a super::model_overlays::links::policy::Policy,
    /// Source marker rebuilding is deferred until the handler borrow ends.
    selection_seen: bool,
    /// Direct screen/reset actions need the same stability check as parser output.
    selection_source_changed: bool,
    /// Semantic shortcut identifies the output under the latest pointer.
    selection_pointer: &'a mut Option<crate::vt::selection::SelectPoint>,
    /// Shared neutral drag and auto-scroll state.
    pointer: &'a mut crate::pointer::Gesture,
    /// Retained fractions belong to one screen/destination/geometry identity.
    wheel: &'a mut super::wheel::Steps,
    /// A composition update is UI state and must never write to the PTY.
    preedit: &'a mut crate::messages::PreeditState,
    /// Settings operations must return to the owning platform store.
    binding_actions: &'a mut Vec<(crate::keybind::Action, bool)>,
    /// Input writer and host policy capabilities remain platform-owned.
    services: &'a mut Services,
    /// Reply streams retain their ordered position while keys remain accepted.
    replies: &'a mut super::replies::Lane,
    /// Set only by the trusted platform's final child status.
    exited: &'a mut Option<ExitInfo>,
    /// The native host reads this after a close policy or held key.
    close_requested: &'a mut bool,
    /// A held notice must be included in the next frame snapshot.
    needs_frame: &'a mut bool,
    /// Normal child-completion preference until settings are wired.
    on_exit: OnExit,
    /// Failed shell starts under this duration remain readable.
    abnormal_exit_ms: u64,
    /// Appearance selection is neutral; only the host supplies the fact.
    theme_state: &'a mut ThemeState,
    /// Style epoch changes are applied to retained row colors.
    frame_policy: &'a mut Option<FramePolicy>,
    /// Accepted input restarts the finite cursor-blink window.
    input_seen: bool,
    /// Typed default stays separate from the terminal request, even within a bus cascade.
    mouse_shift_capture: crate::settings::ShiftCapture,
    /// A changed appearance must show the caret before its next blink cycle.
    appearance_seen: bool,
    /// A gesture-local drain records actual service acceptance after write policy.
    clipboard_written: bool,
    /// Injected turn time also starts a deterministic theme fade.
    now: Instant,
}

impl Handler<'_> {
    /// Applies one policy-approved command without re-entering the bus.
    fn handle(&mut self, command: &Command, ctx: &mut HookCtx<'_, Command, Event>) {
        // Pointer reports after EOF are inert and do not close a held screen.
        if matches!(command, Command::Mouse(_) | Command::Pointer(_)) && self.exited.is_some() {
            return;
        }
        // Once EOF has been reported, all three byte-producing command kinds
        // share the same rule: no subsequent keystroke reaches the child.
        if matches!(
            command,
            Command::Key(_) | Command::BindingAction { .. } | Command::Text(_) | Command::Paste(_)
        ) && self.exited.is_some()
        {
            // A held exit consumes its next key; it cannot send into a
            // transport whose child has already reached EOF.
            if !*self.close_requested {
                *self.close_requested = true;
                ctx.publish(Event::SessionCloseRequested);
            }
            return;
        }
        if let Command::ChildExited(info) = command {
            // Lifecycle dispatch precedes ordinary input encoding so a
            // recorded exit changes policy at its exact command position.
            self.child_exited(info, ctx);
            return;
        }
        if let Command::SetSystemAppearance(appearance) = command {
            self.handle_appearance(*appearance, ctx);
            return;
        }
        if let Command::BindingAction { action, all } = command {
            self.binding_action(action, *all, ctx);
            return;
        }
        if self.query_input(command, ctx) || self.local_command(command, ctx) {
            return;
        }
        // Terminal permissions do not restrict an explicit user copy gesture.
        if matches!(command, Command::ClipboardWrite(_))
            && ctx.origin() == Some(Origin::Terminal)
            && !self.clipboard.allow_write
        {
            return;
        }
        let bytes = self.input_or_host_action(command);
        // Host-only actions return no bytes. This avoids a zero-length PTY
        // write while still allowing clipboard and notification side effects.
        if self.viewport.on_input && (!bytes.is_empty() || matches!(command, Command::Paste(_))) {
            // Only application-bound bytes/paste resume following; inert keys do not.
            self.move_viewport(crate::viewport::ScrollTarget::Bottom, ctx);
        }
        if !bytes.is_empty()
            && let Err(error) = self.write_input(&bytes)
        {
            tracing::warn!(event = "session.input_write_failed", %error, "input could not reach transport");
        }
    }

    /// Flush earlier protocol outcomes before accepting this input in the same FIFO.
    pub(super) fn write_input(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.replies
            .replies(self.services.transport.as_mut(), &mut self.terminal.replies)?;
        self.replies.input(self.services.transport.as_mut(), bytes)
    }

    /// UI requests share policy dispatch while leaving application bytes untouched.
    fn local_command(&mut self, command: &Command, ctx: &mut HookCtx<'_, Command, Event>) -> bool {
        match command {
            Command::GlyphRasterized => {
                // Completion never performs discovery here; the next text-frame poll owns results.
                if ctx.origin() == Some(Origin::Platform) {
                    *self.needs_frame = true;
                    self.services.waker.request_frame();
                }
            }
            Command::OverlayAction(action) => self.overlay_action(*action, ctx),
            Command::Pointer(input) => self.pointer(*input, ctx),
            Command::Copy(target) => self.copy(*target, ctx),
            Command::Select(request) => self.select(*request, ctx),
            Command::Search(request) => self.search_command(request, ctx),
            _ => return self.viewport_command(command, ctx) || self.presentation_input(command),
        }
        true
    }

    /// Resolves one trusted host fact before the render transition starts.
    fn handle_appearance(&mut self, appearance: Appearance, ctx: &mut HookCtx<'_, Command, Event>) {
        // A forged user/API command cannot claim a platform appearance.
        if ctx.origin() != Some(Origin::Platform) {
            return;
        }
        match self.theme_state.system_changed(appearance) {
            Ok(Some(theme)) => {
                self.theme_state.begin(&theme, self.now);
                install(
                    &theme,
                    self.terminal,
                    self.frame_policy.as_mut(),
                    self.services,
                );
                *self.needs_frame = true;
                // Restoring the cursor's visible phase prevents a prior
                // blink-off interval from hiding the new themed block.
                self.appearance_seen = true;
                ctx.publish(Event::ThemeChanged {
                    name: theme.name,
                    appearance: theme.appearance,
                });
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(event="session.appearance_rejected", %error),
        }
    }

    /// Stops writes and selects hold after an early unsuccessful start.
    fn child_exited(&mut self, info: &ExitInfo, ctx: &mut HookCtx<'_, Command, Event>) {
        // A public wire action or plugin cannot forge the platform's EOF.
        if ctx.origin() != Some(Origin::Platform) || self.exited.is_some() {
            return;
        }
        // A nonzero status after a long-running program is an ordinary end;
        // a fast failure is held so shell startup errors remain legible.
        let abnormal = info.runtime_ms < self.abnormal_exit_ms
            && (info.code.is_some_and(|code| code != 0) || info.signal.is_some());
        let hold = self.on_exit == OnExit::Hold || abnormal;
        // Record the lifecycle boundary before closing the writer. Later
        // input can then be consumed as a close request, never forwarded.
        *self.exited = Some(info.clone());
        // A held transcript is no longer a live SSH/privilege context either.
        if self.terminal.clear_contexts() {
            ctx.publish(Event::ContextsChanged(Vec::new()));
        }
        // Drop registration cannot survive its program, including a held final screen.
        self.terminal.clear_drop_target();
        self.forward_drop_reset();
        if let Err(error) = self.services.transport.close() {
            tracing::warn!(event = "session.exit_close_failed", %error);
        }
        if hold {
            // The notice is written directly to VT because the child has
            // already closed its output side and cannot render a final line.
            exit::show_notice(self.terminal, info);
            *self.needs_frame = true;
        } else {
            // Desktop's event loop observes this flag and closes its window
            // even if there are no further PTY bytes to wake the parser.
            *self.close_requested = true;
        }
        // Even a close decision needs a UI wake while the event loop sleeps.
        self.services.waker.request_frame();
        // Observers receive the fact after the grid and close policy settle,
        // so they can inspect the final screen synchronously.
        ctx.publish(Event::SessionExited(info.clone()));
    }

    /// UI-only input is stored locally before any encoder samples VT modes.
    /// A composition has no bytes until the separate Text commit, and focus
    /// reports must retain their queue position relative to surrounding keys.
    fn presentation_input(&mut self, command: &Command) -> bool {
        match command {
            Command::Preedit(state) => {
                // Keep the candidate separate from the VT cells. Replacing
                // application text here would corrupt both terminal state
                // and a later commit; M9 overlays will read this UI snapshot.
                self.preedit.clone_from(state);
                *self.needs_frame = true;
                self.services.waker.request_frame();
            }
            Command::Focus(focused) => {
                self.notifications.focused = *focused;
                // A focus report must sample the same live modes as a key.
                // Deferring it to frame building could reorder it after text.
                super::geometry::apply_focus(
                    self.terminal,
                    self.frame_policy.as_mut(),
                    self.services,
                    self.replies,
                    self.needs_frame,
                    *focused,
                );
            }
            _ => return false,
        };
        true
    }

    /// Encodes an accepted pointer gesture using the latest VT modes.
    pub(super) fn mouse_bytes(&self, event: crate::vt::input::MouseEvent) -> Vec<u8> {
        // The application may change tracking after a host callback;
        // sample it at dispatch, alongside the keyboard modes.
        let mouse = crate::vt::input::MouseModes::from_terminal(self.terminal);
        let Some(event) = self.application_pointer(event) else {
            return Vec::new();
        };
        // Resolve both the user preference and the latest program request at dispatch.
        // Hover reports do not re-arm cursor blinking: continuous motion
        // should not extend the keyboard's fifteen-second idle window.
        crate::vt::input::encode_mouse(
            event,
            mouse.tracking,
            mouse.encoding,
            !self
                .mouse_shift_capture
                .captures(self.terminal.mouse_shift_capture),
        )
    }
}

impl Session {
    /// Reads the host clipboard for a user paste gesture.
    pub fn clipboard_text(&mut self) -> Result<Option<String>, String> {
        self.services.clipboard.request_text()
    }

    /// Reads host text and submits paste through the normal bus policy lane.
    pub fn paste_from_clipboard(&mut self, now: Instant) -> Result<(), String> {
        // A clipboard read is a host gesture, not OSC 52. Keeping Paste as a
        // separate command lets interceptors inspect it before VT encoding.
        if let Some(text) = self.clipboard_text()? {
            self.submit(Command::Paste(text), Origin::User);
            self.pump(now);
        }
        Ok(())
    }

    /// Drains accepted commands synchronously, before any render deadline.
    /// Optional protocols receive exactly one trusted EOF after policy execution.
    /// Both drains borrow the ordinary keybinding policy, then reconcile models once.
    /// No extension callback changes the input priority or the current frame deadline.
    pub fn pump(&mut self, now: Instant) {
        self.drain_commands(now);
    }

    /// Returns clipboard execution for an isolated explicit overlay-copy gesture.
    /// A stopped Copy or ClipboardWrite never reaches the service and stays false.
    pub(super) fn drain_commands(&mut self, now: Instant) -> bool {
        // Split borrows allow the bus to own its queue while the handler
        // edits VT and services without cloning either large state object.
        let already_exited = self.exited.is_some();
        self.prepare_bindings(now);
        let mut handler = Handler {
            osc: &mut self.osc,
            clipboard: &mut self.clipboard,
            host_events: &mut self.host_events,
            notifications: &mut self.notifications,
            binding_actions: &mut self.binding_actions,
            terminal: &mut self.terminal,
            viewport: &mut self.viewport,
            selection: &mut self.selection,
            search: &mut self.search,
            query: &mut self.model_overlays.query,
            web_search: self.keybindings.web,
            host_search: self.search_ui == crate::search::SearchUi::Host,
            occluded_rects: self.model_overlays.occluded_rects.as_deref().unwrap_or(&[]),
            link_policy: &self.model_overlays.link_policy,
            selection_seen: false,
            selection_source_changed: false,
            selection_pointer: &mut self.selection_pointer,
            pointer: &mut self.pointer,
            wheel: &mut self.wheel,
            preedit: &mut self.preedit,
            services: &mut self.services,
            replies: &mut self.replies,
            exited: &mut self.exited,
            close_requested: &mut self.close_requested,
            needs_frame: &mut self.needs_frame,
            on_exit: self.on_exit,
            abnormal_exit_ms: self.abnormal_exit_ms,
            theme_state: &mut self.theme_state,
            frame_policy: &mut self.frame_policy,
            input_seen: false,
            clipboard_written: false,
            mouse_shift_capture: self.mouse_shift_capture,
            appearance_seen: false,
            now,
        };
        self.bus.drain_with(
            Some(binding_policy(&mut self.keybindings)),
            |command, ctx| handler.handle(command, ctx),
        );
        // A terminal-reset action and trusted EOF become protocol lifecycle facts
        // after policy execution. Drain their extension deltas before returning.
        if handler.osc.commands_finished(
            handler.terminal,
            handler.exited.as_ref(),
            already_exited,
            &mut self.bus,
        ) {
            // This second drain delivers plugin deltas with the normal cascade rules.
            self.bus.drain_with(
                Some(binding_policy(&mut self.keybindings)),
                |command, ctx| handler.handle(command, ctx),
            );
        }
        // Accepted input and a visible host appearance switch both reveal
        // the cursor. Unrelated bus commands cannot reset its blink phase.
        let reveal = handler.input_seen || handler.appearance_seen;
        let selection_seen = handler.selection_seen;
        let selection_source_changed = handler.selection_source_changed;
        let written = handler.clipboard_written;
        self.finish_command_effects(now, reveal, selection_seen, selection_source_changed);
        written
    }

    /// Reconciles coarse command effects once, after all policy-approved mutations.
    fn finish_command_effects(
        &mut self,
        now: Instant,
        reveal: bool,
        selection_seen: bool,
        selection_source_changed: bool,
    ) {
        // Host completion is asynchronous; only correlation and a hang deadline run here.
        self.clipboard
            .reconcile_grant(self.terminal.clipboard_grant_epoch);
        self.clipboard
            .finish(self.terminal.reset_epoch, now, &mut self.terminal.replies);
        if let Some(deadline) = self.clipboard.deadline() {
            self.services.waker.wake_at(deadline);
        }
        self.notifications
            .reset(self.terminal.reset_epoch, self.services.notifier.as_mut());
        self.refresh_notification_toasts();
        self.flush_clipboard_replies();
        // Clear/reset shortcuts mutate the ring without a parser feed to reconcile it.
        if selection_source_changed {
            self.reconcile_selection();
            self.search.dirty = true;
        }
        // User folds/reveals settle here before any later immutable publication.
        self.reconcile_viewport(false);
        if selection_seen {
            self.refresh_scrollbar();
        }
        self.schedule_scroll(now);
        self.schedule_bindings(now);
        if reveal {
            self.reveal_blinking_ink(now);
        }
    }

    /// True after Close policy or a key on a held exit screen.
    #[must_use]
    pub fn close_requested(&self) -> bool {
        self.close_requested
    }

    /// Returns status only after all output before EOF was parsed.
    #[must_use]
    pub fn exit_info(&self) -> Option<&ExitInfo> {
        self.exited.as_ref()
    }
}

/// Each drain borrows the same matcher state at its established priority.
/// Borrowing, rather than registration, keeps introspection and sequence state in Session.
fn binding_policy(
    bindings: &mut crate::keybind::Keybindings,
) -> BorrowedPolicy<'_, Command, Event> {
    // Priority/name/mask are unchanged from the pre-extension command policy.
    BorrowedPolicy {
        priority: 100,
        kinds: CommandKinds::from_bits_retain(1),
        name: "keybindings",
        hook: bindings,
    }
}
