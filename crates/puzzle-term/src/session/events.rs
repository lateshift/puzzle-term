//! OSC boundary identities receive Session time while VT stays clock-free.
//! Marker invalidation also trims clocks after resets or mark-list eviction.
//! Translates coarse VT facts into the bus without per-cell traffic (P8).
//!
//! Facts are drained after a parser slice rather than emitted per cell.
//! The bus therefore carries titles, shell phases and grid geometry, while
//! ordinary printing and damage stay on the direct VT path (P8).
//! A bell is both a fact for observers and a host action for sound policy.
//! Observers cannot veto that BEL arrived; a RingBell interceptor can still
//! deny an audible effect without hiding the fact from the page.
//! Titles and directory URIs are already bounded by VT's OSC handlers.
//! This translation preserves their typed payloads instead of re-parsing OSC.
//! Prompt, input and output markers remain distinct for future command zones.
//! Grid resize facts carry cells, whereas surface pixels are host geometry.
//! Clipboard, notification and pointer requests are explicitly Terminal-origin.
//! That provenance allows a host to distinguish application OSC from a user
//! paste gesture and enforce the clipboard/security policy consistently.
//! Publishing through Bus skips facts without listeners before queue growth.
//! Completed OSC 5522 requests cross the same owner event boundary as OSC 52.
//! Forwarding queues policy commands rather than synchronously invoking the host.
//! This preserves ordinary output/reply flow while an OS clipboard operation waits.
//! Read permission and gesture grants belong to Session, not the pure VT parser.
//! No renderer or text context participates in clipboard authorization.
use super::Session;
use crate::{
    messages::{Command, Event},
    vt::terminal::TerminalEvent,
};
use puzzle_bus::Origin;
/// Facts enter observers; host requests enter the policy lane.
impl Session {
    /// Converts active VT facts into their matching typed bus events.
    pub(super) fn forward_events(&mut self) {
        // OSC source changes invalidate marker colors/positions once per drain.
        // Plain cell output is handled by the separate ring-change reconciliation.
        let marker_change = self.prepare_source_events();
        // Contexts are contents, not a historical trace. One final immutable path
        // per drain avoids copying every intermediate stack in a parser slice.
        let mut contexts_changed = false;
        // Own the reusable queue while dispatching: handlers may borrow Session,
        // but never parse output or append VT facts during this translation.
        let mut events = std::mem::take(&mut self.terminal.events);
        for event in events.drain(..) {
            match event {
                TerminalEvent::ZoneBoundary { id, kind } => {
                    if let Some(now) = self.host_time {
                        self.model_overlays.blocks.boundary(id, kind, now);
                    }
                }
                TerminalEvent::Bell { line } => self.forward_bell(line),
                TerminalEvent::TitleChanged(title) => self.bus.publish(Event::TitleChanged(title)),
                TerminalEvent::IconNameChanged(name) => {
                    self.bus.publish(Event::IconNameChanged(name))
                }
                TerminalEvent::CwdChanged(uri) => self.bus.publish(Event::CwdChanged(uri)),
                TerminalEvent::ContextsChanged => contexts_changed = true,
                TerminalEvent::PromptStarted => {
                    // Optional protocols observe the same fact before a later OSC is parsed.
                    self.osc.event(&Event::PromptStarted, &mut self.bus);
                    self.bus.publish(Event::PromptStarted);
                }
                TerminalEvent::InputStarted => self.bus.publish(Event::InputStarted),
                TerminalEvent::OutputStarted(command) => {
                    self.bus.publish(Event::CommandStarted(command));
                }
                TerminalEvent::CommandFinished(exit) => {
                    self.bus.publish(Event::CommandFinished(exit))
                }
                TerminalEvent::GridResized { columns, rows } => {
                    self.bus.publish(Event::GridResized { columns, rows })
                }
                TerminalEvent::Progress { state, percent } => {
                    self.model_overlays.visuals.progress = (state != 0).then_some((state, percent));
                    self.overlay_events.0 |= puzzle_overlay::provider::EventKinds::STATUS.0;
                    self.bus.publish(Event::Progress { state, percent })
                }
                // Host requests use the same interceptor lane as user
                // commands, so OSC output cannot bypass clipboard policy.
                TerminalEvent::ClipboardRequest(request) => {
                    self.clipboard
                        .forward(request, &mut self.bus, &mut self.terminal.replies)
                }
                TerminalEvent::ClipboardWrite(text) => self
                    .bus
                    .submit(Command::ClipboardWrite(text.into()), Origin::Terminal),
                // Marshal a value to policy; parser callbacks never post native UI.
                TerminalEvent::Drop(effect) => {
                    self.bus.submit(Command::Drop(effect), Origin::Terminal)
                }
                TerminalEvent::DesktopNotification(request) => self
                    .bus
                    .submit(Command::DesktopNotification(request), Origin::Terminal),
                // Older OSC origins enter the same policy without inventing OSC 99 metadata.
                TerminalEvent::Notification { title, body } => self
                    .bus
                    .submit(Command::Notify { title, body }, Origin::Terminal),
                TerminalEvent::PointerShape(shape) => self
                    .bus
                    .submit(Command::SetPointerShape(shape), Origin::Terminal),
            }
        }
        self.terminal.events = events;
        self.finish_source_events(marker_change, contexts_changed);
    }

    /// Flush coalesced contents invalidations after all facts in this slice.
    fn finish_source_events(&mut self, marker_change: bool, contexts_changed: bool) {
        if contexts_changed && self.bus.observes(puzzle_bus::hooks::EventKinds::at(27)) {
            // Unobserved metadata changes never allocate a duplicate payload (§5.3).
            self.bus
                .publish(Event::ContextsChanged(self.terminal.contexts().to_vec()));
        }
        self.finish_marker_events(marker_change);
    }

    /// Preserve the BEL source position and route its audible effect through policy.
    fn forward_bell(&mut self, line: Option<puzzle_vt::tracked::LineId>) {
        // The VT event carries the cursor at BEL, not at feed completion.
        // Later output in this same callback may already have scrolled it.
        retain_bell(&mut self.bells, &mut self.terminal.tracked, line);
        self.model_overlays.visuals.bell = self.model_overlays.visuals.bell.wrapping_add(1);
        // A coarse identity changes even when BEL leaves every cell untouched.
        // The model starts one interval; rendering cannot restart it per frame.
        self.overlay_events.0 |= puzzle_overlay::provider::EventKinds::STATUS.0;
        self.bus.publish(Event::Bell);
        self.bus.submit(Command::RingBell, Origin::Terminal);
    }

    /// Invalidate native completions before processing new requests after a reset.
    fn reconcile_protocol_epochs(&mut self) {
        // RIS invalidates native completions before any new request in this slice.
        self.clipboard.reset(self.terminal.reset_epoch);
        self.notifications
            .reset(self.terminal.reset_epoch, self.services.notifier.as_mut());
        self.clipboard
            .reconcile_grant(self.terminal.clipboard_grant_epoch);
    }

    /// Shell boundaries change cached scrollbar sources only on a coarse source event.
    fn finish_marker_events(&mut self, marker_change: bool) {
        // No per-frame marker scan: the next frame borrows this prepared data.
        if marker_change {
            // The bounded mark owner also bounds Session-only clock records.
            self.model_overlays
                .blocks
                .trim(self.terminal.zones.iter().next().map(|mark| mark.id));
            self.refresh_scrollbar();
        }
    }
}

/// Coarse source tests happen before the event vector is drained.
impl Session {
    /// Reconcile reset lifetimes before source facts are drained or forwarded.
    /// Plain output has no marker work; OSC and BEL invalidate one source capture.
    fn prepare_source_events(&mut self) -> bool {
        self.reconcile_protocol_epochs();
        self.terminal.events.iter().any(|event| {
            matches!(
                event,
                TerminalEvent::PromptStarted
                    | TerminalEvent::ZoneBoundary { .. }
                    | TerminalEvent::CommandFinished(_)
                    | TerminalEvent::Bell { .. }
            )
        })
    }
}

/// Keep bell markers bounded and release the evicted tracked endpoint.
fn retain_bell(
    bells: &mut Vec<puzzle_vt::tracked::TrackedPos>,
    tracked: &mut puzzle_vt::tracked::TrackedPositions,
    line: Option<puzzle_vt::tracked::LineId>,
) {
    if let Some(line) = line {
        if bells.len() == 10_000 {
            tracked.release(bells.remove(0));
        }
        bells.push(tracked.track(line, 0));
    }
}
