//! Session-owned protocols for OSC numbers the terminal does not implement.
//! The VT parser yields at registered commands, then resumes after the handler.
//! This keeps queries ordered without storing an unbounded protocol transcript.
//! Handlers have no thread, clock or GPU dependency; web uses this same seam.
use crate::messages::{Command, Event};
use puzzle_bus::Bus;
/// Owner-scoped tracked-source storage is separate from protocol parsing.
mod anchors;
/// Actual private source capacity is admitted transactionally before capture.
mod capacity;
/// Borrowed commands never expose a mutable Session to a plugin.
mod context;
pub use anchors::{Endpoint, EndpointError, EndpointReceipt};
pub use context::OscContext;
/// Bounded uninterpreted payload and terminator metadata from the VT parser.
pub use puzzle_vt::osc::OscRequest;
/// Trusted host controls share handler ownership without a second registry.
mod control;
/// Optional source-owned overlay publications and genuine user interactions.
mod presentation;
pub use presentation::{OscOverlay, OscUiEffect, OscUserAction, SourceSpan, SourceWindow};
/// Bounded optional owner continuations and immutable native host effects.
mod service;
/// Caller-turn credit and one retained ordered request, without another queue.
mod turn;
pub use service::OscHostEffect;
/// Constant-size owner-scoped witnesses; absent from default parser work.
mod witness;
pub use witness::{SourceTextScan, SourceWitness};
/// Incremental source reads preserve the handler's ordered parser barrier.
mod source_scan;
/// Source boundaries preserve whole wide characters and rectangular text units.
mod units;
pub use source_scan::{ScanProgress, SourceReadError, SourceRevision, SourceRow, SourceScan};

/// A handler can pause parsing while bounded owner work continues next turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OscProgress {
    /// This boundary is finished; following application bytes may be consumed.
    Complete,
    /// Retain this exact request and leave all following bytes behind it.
    Deferred,
    /// An immutable reply buffer or writer credit needs an external completion wake.
    AwaitReply,
}

/// A trusted optional protocol installed by a host, never by terminal output.
pub trait OscHandler: Send {
    /// Handles one complete command; append replies to the terminal's FIFO.
    /// Publish plugin facts with `bus.publish_ext`, without adding core variants.
    fn handle(
        &mut self,
        request: &OscRequest,
        replies: &mut Vec<u8>,
        bus: &mut Bus<Command, Event>,
    );
    /// Handles source-sensitive protocols at their authenticated ordered boundary.
    /// Existing protocols retain their original callback and pay no endpoint allocation.
    fn handle_with_context(
        &mut self,
        request: &OscRequest,
        _source: &mut OscContext<'_>,
        replies: &mut Vec<u8>,
        bus: &mut Bus<Command, Event>,
    ) {
        self.handle(request, replies, bus);
    }
    /// Applies a trusted host value on this handler's existing Session owner.
    /// The extension's typed payload may contain private provisioning material;
    /// its wire conversion must redact secrets and must never decode authority
    /// from program output. Source cleanup uses this registration's context.
    /// Controls neither replenish work credits nor implicitly resume parsing.
    fn control(
        &mut self,
        _control: &mut puzzle_bus::wire::Extension,
        _source: &mut OscContext<'_>,
        _bus: &mut Bus<Command, Event>,
    ) -> Result<(), String> {
        Err("OSC handler does not support host controls".into())
    }
    /// Transfers a model/view pair once at explicit registration. The model
    /// stays with Session; the view moves only through sealed publications.
    fn overlay(&mut self) -> Option<OscOverlay> {
        None
    }
    /// Produces immutable source state only when its meaning or placement changes.
    /// It must own/account its storage; the view receives no live source borrow.
    fn overlay_state(
        &mut self,
        _source: &mut OscContext<'_>,
        _window: SourceWindow,
    ) -> Option<puzzle_overlay::model::SharedState> {
        None
    }
    /// Handles a real user action after policy/presented-region routing. A
    /// protocol must revalidate its source and generation before changing UI.
    fn user_action(
        &mut self,
        _action: OscUserAction<'_>,
        _source: &mut OscContext<'_>,
    ) -> Option<OscUiEffect> {
        None
    }
    /// True only while local bounded work can make progress without a host answer.
    /// An external completion wait must return false, preserving idle freedom.
    fn owner_work_pending(&self) -> bool {
        false
    }
    /// Advances local user work under this existing parsing turn's credits.
    /// This never refills credit, waits on a host, or resumes a second parser.
    fn continue_owner_work(
        &mut self,
        _source: &mut OscContext<'_>,
        _bus: &mut Bus<Command, Event>,
    ) {
    }
    /// Moves one immutable host effect after the owner turn. The handler must
    /// bound outstanding effects and precharge payloads until completion.
    fn take_host_effect(&mut self) -> Option<OscHostEffect> {
        None
    }
    /// Starts one caller-declared parsing quantum, never an individual OSC.
    /// Registered handlers may replenish bounded work credits here. Input/pump
    /// and rendering do not call this method or replenish parsing credits.
    fn begin_turn(&mut self) {}
    /// Ends that quantum; aggregate diagnostics here rather than logging a flood.
    fn end_turn(&mut self) {}
    /// Processes or resumes the same ordered request without borrowing later text.
    /// A Deferred handler must retain its own incremental state and never repeat
    /// a committed mutation/reply when this callback resumes in the next turn.
    fn try_handle_with_context(
        &mut self,
        request: &OscRequest,
        source: &mut OscContext<'_>,
        replies: &mut Vec<u8>,
        bus: &mut Bus<Command, Event>,
    ) -> OscProgress {
        self.handle_with_context(request, source, replies, bus);
        OscProgress::Complete
    }
    /// Clear/reset/screen transitions invalidate receipts before this callback.
    /// Protocols may retire their source metadata without changing core enums.
    fn on_source_changed(&mut self, _source: &mut OscContext<'_>, _bus: &mut Bus<Command, Event>) {}
    /// Reconcile source writes/eviction after an owner turn, without a scalar hook.
    /// Registered consumers can query sticky constant-size watches; physical reads
    /// still need their own bounded continuation. No absent handler pays this call.
    fn on_source_updated(&mut self, _source: &mut OscContext<'_>, _bus: &mut Bus<Command, Event>) {}
    /// Existing PromptStarted and SessionExited facts drive protocol lifetimes.
    fn on_event(&mut self, _event: &Event, _bus: &mut Bus<Command, Event>) {}
    /// RIS and the equivalent reset action clear protocol state; DECSTR does not.
    fn reset(&mut self, _bus: &mut Bus<Command, Event>) {}
}

/// Registration storage is empty, allocation-free, when optional protocols are absent.
/// The parallel number list is the parser's complete interest set; endpoint
/// contexts are created only after a captured command selects its owner.
/// There is no scalar callback or source scan in the unregistered bulk path.
#[derive(Default)]
pub(crate) struct Registry {
    /// Parallel identities let VT inspect interests without seeing plugin objects.
    pub(crate) numbers: Vec<u16>,
    /// One owner and one handler per number; built-ins cannot be replaced.
    handlers: Vec<Registration>,
    /// One bounded buffer reused across commands and transport reads.
    pub(crate) request: OscRequest,
    /// Only RIS changes this identity; alternate screens preserve plugin state.
    reset_seen: u64,
    /// No later parser call may overwrite request while this boundary is paused.
    pending: bool,
    /// Unlike bounded work deferral, this wait must never schedule polling feeds.
    waiting_reply: bool,
}

/// A protocol and its private receipt namespace have one registration lifetime.
struct Registration {
    /// Host-installed handler is never selected or replaced by PTY output.
    handler: Box<dyn OscHandler>,
    /// Terminal-owned endpoints cannot be queried by a sibling handler.
    endpoints: anchors::Store,
    /// Source and interaction share one registration; views are separately owned.
    provider: Option<puzzle_overlay::registry::ProviderId>,
}

/// Registration and dispatch share a single mutable Session owner (D-50).
impl Registry {
    /// Validates before mutation; duplicate registrations cannot silently replace state.
    pub(crate) fn register(
        &mut self,
        number: u16,
        mut handler: Box<dyn OscHandler>,
        terminal: &mut puzzle_vt::Terminal,
        overlays: &mut puzzle_overlay::OverlayRegistry,
    ) -> Result<bool, String> {
        // Numeric identity belongs to the terminal for built-ins and to one plugin otherwise.
        if puzzle_vt::osc::is_builtin(number) || self.numbers.contains(&number) {
            return Err("OSC number is already handled".into());
        }
        // Every registered source context can sample revisions between watches.
        // Activate before its first callback, never on the unregistered feed path.
        terminal.register_source_observer();
        // Parallel order stays stable because a handler is never replaced in place.
        let provider = handler
            .overlay()
            .map(|(model, view)| overlays.add(model, view));
        self.numbers.push(number);
        // Source generations are sampled at registration, so a historical
        // clear cannot invalidate the first endpoint captured by this owner.
        self.handlers.push(Registration {
            handler,
            endpoints: anchors::Store::new(terminal),
            provider,
        });
        Ok(provider.is_some())
    }

    /// A semantic reset is applied before dispatching the next captured protocol command.
    pub(crate) fn reset_if_changed(&mut self, epoch: u64, bus: &mut Bus<Command, Event>) -> bool {
        // Comparing epochs catches RIS even when it occurs inside a larger parser slice.
        if self.reset_seen != epoch {
            self.reset_seen = epoch;
            // Handlers reuse their own storage while forgetting all protocol state.
            for handler in &mut self.handlers {
                handler.handler.reset(bus);
            }
            return true;
        }
        false
    }

    /// Delivers existing coarse facts without registering a second bus or shared owner.
    pub(crate) fn event(&mut self, event: &Event, bus: &mut Bus<Command, Event>) {
        // These are existing coarse lifecycle facts, never per-byte bus traffic (P8).
        for handler in &mut self.handlers {
            handler.handler.on_event(event, bus);
        }
    }

    /// Runs the one interested handler before later parser bytes can generate replies.
    pub(crate) fn dispatch(
        &mut self,
        terminal: &mut puzzle_vt::Terminal,
        replies_lane: &mut crate::session::replies::Lane,
        bus: &mut Bus<Command, Event>,
    ) {
        // The interest list was checked by VT; lookup selects the corresponding owner.
        if let Some(index) = self
            .numbers
            .iter()
            .position(|number| *number == self.request.number)
        {
            // The reply buffer is the same FIFO used by core queries before and after this OSC.
            let registration = &mut self.handlers[index];
            // Move only the FIFO borrow out: source commands cannot inspect or reorder replies.
            // Taking the Vec transfers its warmed allocation, not its bytes.
            // The callback receives the earlier core replies in that same FIFO;
            // the original allocation returns before parsing another VT query.
            let mut replies = std::mem::take(&mut terminal.replies);
            let mut source = OscContext {
                terminal,
                endpoints: &mut registration.endpoints,
                replies: Some(replies_lane),
            };
            let progress = registration.handler.try_handle_with_context(
                &self.request,
                &mut source,
                &mut replies,
                bus,
            );
            self.pending = progress != OscProgress::Complete;
            self.waiting_reply = progress == OscProgress::AwaitReply;
            terminal.replies = replies;
        }
    }
}

/// Command-driven resets and child exit share parser-driven lifecycle handling.
impl Registry {
    /// Delivers reset/exit after policy and reports whether another event drain is needed.
    pub(crate) fn commands_finished(
        &mut self,
        terminal: &mut puzzle_vt::Terminal,
        exit: Option<&crate::messages::ExitInfo>,
        already_exited: bool,
        bus: &mut Bus<Command, Event>,
    ) -> bool {
        // A reset command is semantically RIS even though no ESC byte was parsed.
        let reset = self.reset_if_changed(terminal.reset_epoch, bus);
        // Lifecycle ownership is reconciled even on an empty feed: host clear
        // and child exit need not be followed by another OSC command to retire
        // source handles. The empty registrations loop visits no source state.
        self.sync_endpoints(terminal, exit.is_some(), bus);
        if exit.is_some() {
            // An explicit disconnect ends ordered application work too. A held
            // final screen must not keep a continuation alive for a revoked peer.
            self.pending = false;
            self.waiting_reply = false;
            self.request.payload.clear();
            self.request.selector.clear();
        }
        // Child exit is delivered once; later held-screen keys cannot expire records again.
        if !already_exited && let Some(info) = exit {
            self.event(&Event::SessionExited(info.clone()), bus);
            return true;
        }
        // A source-only mutation may publish plugin facts after the first drain.
        // Deliver them now, rather than depending on another input or display tick.
        reset || bus.has_pending_events()
    }
}

/// Source lifetimes are reconciled only for explicitly registered handlers.
impl Registry {
    /// Clear/exit retires trackers on their owner; no render-side source queries.
    pub(crate) fn sync_endpoints(
        &mut self,
        terminal: &mut puzzle_vt::Terminal,
        exited: bool,
        bus: &mut Bus<Command, Event>,
    ) {
        for registration in &mut self.handlers {
            // Only coarse generations are compared here. Scrolling/reflow are
            // already maintained by VT's tracker; no rows are walked per pump.
            let changed = registration.endpoints.sync(terminal);
            if exited {
                // A held final screen is readable, but cannot acquire fresh source receipts.
                registration.endpoints.clear(terminal);
                continue;
            }
            if changed {
                // Invalidate before notifying the plugin, so its callback can
                // never query a receipt from the document that just ended.
                let mut source = OscContext {
                    terminal,
                    endpoints: &mut registration.endpoints,
                    replies: None,
                };
                registration.handler.on_source_changed(&mut source, bus);
            }
            // Only registered consumers receive an owner-turn reconciliation.
            // This borrows no frame and does not scan any source rows by itself.
            let mut source = OscContext {
                terminal,
                endpoints: &mut registration.endpoints,
                replies: None,
            };
            registration.handler.on_source_updated(&mut source, bus);
        }
    }
}

#[cfg(test)]
/// Tests observe retained ownership rather than peeking into source handles.
impl Registry {
    /// Counts live endpoint entries across registrations, including held-screen exit.
    pub(crate) fn endpoint_count(&self) -> usize {
        self.handlers
            .iter()
            .map(|registration| registration.endpoints.len())
            .sum()
    }
}
