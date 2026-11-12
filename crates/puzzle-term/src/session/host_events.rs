//! One value outbox for host results after a synchronous Session turn (D-50).
//!
//! Native hosts forward these values through their event channel; web hosts
//! consume them on the same thread before invoking foreign callbacks. The bus
//! observer owns its queue, so no shared mutable state or synchronization is
//! needed to retain facts. Parser-only users leave observation disabled.
//!
//! Observation is installed before the first host-driven turn. A bare headless
//! parser has no host and pays neither payload copies nor result allocations.
//! This observer subscribes only to facts needed by platform UI state; ordinary
//! printing and per-cell mutation remain outside the bus (performance rule P8).
//! Plugin subscribers keep their typed extension registrations. An erased plugin
//! payload cannot be cloned indiscriminately into a host catch-all observer.
//! The host result queue retains capacity after draining, like the deferred
//! transport and clipboard service queues from which effects are transferred.
//! FIFO is preserved independently for facts and transport transactions. Input
//! bytes and parser replies both enter the transport's one producer order; this
//! merge does not re-encode, combine, or reorder those transactions.
//! Native transports may enqueue to their writer directly and have no deferred
//! effects. Browser transports move owned bytes here after the Session turn,
//! allowing JS invocation only after the mutable Session borrow has ended.
//! Wake requests are coalesced by their service and emit a value only when
//! damage or a finite timer exists. Repeated idle drains produce no work.
//! Close is a lifecycle edge rather than a polling query. A normal successful
//! child exit can choose Close without any subsequent key callback; the outbox
//! therefore reports that edge once even if no held-screen key produced it.
use super::Session;
use crate::{
    keybind::Action,
    messages::{Command, Event},
    platform::{ClipboardEffect, TransportEffect, WakeRequests},
};
use puzzle_bus::{
    hooks::{EventKinds, HookCtx, HookId, Observer},
    message::Message,
};
use puzzle_overlay::{decoration::DecorationId, draw::CursorIcon, registry::ProviderId};
use std::collections::VecDeque;

/// Results needed by a host are values; none permits calling into another owner.
pub enum HostEvent {
    /// Fresh program output after the last explicit host seen acknowledgement.
    Activity,
    /// Effective default encoded-sRGB colours for native components and host chrome.
    Colors {
        /// Default paper including OSC 11 overrides.
        background: [u8; 3],
        /// Default ink including OSC 10 overrides.
        foreground: [u8; 3],
        /// Background luminance gives white at least as much contrast as black.
        dark: bool,
    },
    /// Host-mode find intent; emitted without opening the built-in query overlay.
    SearchRequested,
    /// An explicit host read copies selection text without touching the clipboard.
    SelectedText {
        /// Host correlation token, returned unchanged.
        request: u64,
        /// Plain grapheme text, empty when no selection survives.
        text: String,
    },
    /// Change-only geometry and policy observations replace native synchronous queries.
    InputState(super::host_state::InputState),
    /// Change-only search count, current match and incremental completion for a host find UI.
    SearchState(crate::search::SearchState),
    /// Refresh-driven motion bypasses output settling using this change-only edge.
    AnimationChanged(bool),
    /// Immutable optional-protocol host work; its owner prepays the payload.
    /// No live Session, source borrow or private grant crosses this boundary.
    OscEffect(crate::osc::OscHostEffect),
    /// Policy-completed terminal fact, including title, bell, search and exit.
    Fact(Event),
    /// A browser may let its textarea insert text after the native-key policy turn.
    NativeTextAllowed(bool),
    /// Overlay consumption tells a browser whether to prevent its native gesture.
    PointerHandled(bool),
    /// Pointer affordance chosen against the last presented region table.
    CursorChanged(CursorIcon),
    /// Validated setting action and the resolved appearance at its execution.
    BoundAction {
        /// Operation performed by the host-owned settings store.
        action: Action,
        /// Whether all sessions sharing that store are targeted.
        all: bool,
        /// Theme toggles use effective appearance, not a later OS guess.
        dark: bool,
    },
    /// Deferred clipboard transaction after normal bus security policy.
    Clipboard(ClipboardEffect),
    /// Drop registration and feedback carry parsed values, never OSC strings.
    Drop(crate::vt::dnd::Effect),
    /// Approved URL opens in the original host gesture after releasing Session.
    OpenLink(String),
    /// Ordered bytes, resize or close from a deferred transport.
    Transport(TransportEffect),
    /// Coalesced frame and timer scheduling for the platform refresh source.
    Wake(WakeRequests),
    /// Prior presented-region allocation moves back for warm host-side reuse.
    RegionsReturned(puzzle_overlay::regions::PresentedRegions),
    /// Correlated registration identity; the host never blocks to obtain it.
    OverlayAdded {
        /// Token supplied with the addition request.
        request: u64,
        /// Monotonic registration identity used by later removal.
        id: ProviderId,
    },
    /// Correlated tracked endpoint; None means the source cell is unavailable.
    MarkerAdded {
        /// Token supplied by the host.
        request: u64,
        /// Caller owns this endpoint until explicitly released.
        marker: Option<puzzle_overlay::anchor::Marker>,
    },
    /// Correlated validation result for a declarative addition.
    DecorationAdded {
        /// Token supplied with the addition request.
        request: u64,
        /// Rejection leaves the existing declarations unchanged.
        result: Result<DecorationId, String>,
    },
}

/// A bus-owned queue preserves fact ordering without an Arc or a borrowed callback.
#[derive(Default)]
struct Facts(VecDeque<Event>);
/// Only host-relevant facts are copied; cell traffic never enters this path (P8).
impl Observer<Command, Event> for Facts {
    /// Copies bounded payloads after policy execution; plugin facts keep their own API.
    fn observe(&mut self, event: &Event, _: &mut HookCtx<'_, Command, Event>) {
        let copy = match event {
            Event::TitleChanged(value) => Event::TitleChanged(value.clone()),
            // Tab metadata is coarse protocol state, never a scan of output cells.
            Event::CwdChanged(value) => Event::CwdChanged(value.clone()),
            Event::PromptStarted => Event::PromptStarted,
            Event::InputStarted => Event::InputStarted,
            Event::CommandStarted(command) => Event::CommandStarted(command.clone()),
            Event::CommandFinished(exit) => Event::CommandFinished(*exit),
            Event::Progress { state, percent } => Event::Progress {
                state: *state,
                percent: *percent,
            },
            Event::Bell => Event::Bell,
            Event::ContextsChanged(path) => Event::ContextsChanged(path.clone()),
            Event::GridResized { columns, rows } => Event::GridResized {
                columns: *columns,
                rows: *rows,
            },
            Event::ViewportScrolled(value) => Event::ViewportScrolled(*value),
            Event::SelectionChanged(value) => Event::SelectionChanged(*value),
            Event::NewOutputChanged(value) => Event::NewOutputChanged(*value),
            Event::SearchRequested => Event::SearchRequested,
            Event::SessionExited(info) => Event::SessionExited(info.clone()),
            Event::SessionCloseRequested => Event::SessionCloseRequested,
            Event::ThemeChanged { name, appearance } => Event::ThemeChanged {
                name: name.clone(),
                appearance: *appearance,
            },
            Event::Ext(value) => match value.try_clone() {
                Ok(value) => Event::Ext(value),
                Err(error) => {
                    tracing::warn!(event = "session.extension_copy_failed", ?error);
                    return;
                }
            },
            // Unsubscribed kinds cannot allocate or queue in this observer.
            _ => return,
        };
        self.0.push_back(copy);
    }
    /// Moves one owned fact after the bus drain's borrow has ended.
    fn take_event(&mut self) -> Option<Event> {
        self.0.pop_front()
    }
}

/// The host seam is opt-in so bare parser and bus zero-cost tests stay unchanged.
impl Session {
    /// Enables fact delivery before output arrives; repeated setup is harmless.
    pub fn enable_host_events(&mut self) -> Result<(), String> {
        if self.host_fact_hook.is_some() {
            return Ok(());
        }
        // Derive masks from the actual variants rather than duplicating catalogue indices.
        let samples = [
            Event::TitleChanged(String::new()),
            Event::CwdChanged(String::new()),
            Event::PromptStarted,
            Event::InputStarted,
            Event::CommandStarted(None),
            Event::CommandFinished(None),
            Event::Progress {
                state: 0,
                percent: 0,
            },
            Event::Bell,
            Event::ContextsChanged(Vec::new()),
            Event::GridResized {
                columns: 0,
                rows: 0,
            },
            Event::ViewportScrolled(0.0),
            Event::SelectionChanged(None),
            Event::NewOutputChanged(false),
            Event::SearchRequested,
            Event::SessionExited(crate::messages::ExitInfo {
                code: None,
                signal: None,
                runtime_ms: 0,
            }),
            Event::SessionCloseRequested,
            Event::ThemeChanged {
                name: String::new(),
                appearance: crate::theme::Appearance::Dark,
            },
        ];
        let kinds = samples
            .iter()
            .fold(EventKinds::at(Event::EXT_INDEX), |mask, event| {
                mask | EventKinds::at(event.kind_index())
            });
        // Register once; later table changes by unrelated users cannot duplicate this queue.
        self.host_fact_hook = Some(
            self.bus
                .observe(kinds, 0, "session-host", Facts::default())?,
        );
        Ok(())
    }

    /// Returns one result after a feed, message or publication, with no host callback.
    pub fn take_event(&mut self) -> Option<HostEvent> {
        // Collection also handles a direct parser feed between host message turns.
        self.collect_host_events();
        // Startup installs metrics/theme/appearance before the owner transfers.
        // Sample colours at delivery, coalescing those undelivered defaults once.
        if self.host_fact_hook.is_some() {
            self.collect_host_colors();
        }
        self.host_events.pop_front()
    }

    /// Folds prior platform outboxes into one drain surface without re-running policy.
    pub(super) fn collect_host_events(&mut self) {
        let Some(hook) = self.host_fact_hook else {
            return;
        };
        // Optional payloads move once; source owners retain their charge until
        // the host completes or cancels, rather than cloning text through JSON.
        self.osc.take_host_effects(&mut self.host_events);
        self.collect_host_state();
        // Facts retain bus FIFO ordering; service transactions retain each producer's order.
        while let Some(event) = self.bus.take_observed(hook) {
            if matches!(event, Event::SessionCloseRequested) {
                self.host_close_sent = true;
            }
            self.host_events.push_back(HostEvent::Fact(event));
        }
        // Ordinary Close exits need no held-key command to tell the host to close.
        if self.close_requested && !self.host_close_sent {
            self.host_close_sent = true;
            self.host_events
                .push_back(HostEvent::Fact(Event::SessionCloseRequested));
        }
        let dark = self
            .active_theme()
            .is_none_or(|theme| theme.appearance == crate::theme::Appearance::Dark);
        // Capture appearance now so a later host theme action cannot change this result.
        while let Some((action, all)) = self.take_binding_action() {
            self.host_events
                .push_back(HostEvent::BoundAction { action, all, dark });
        }
        while let Some(uri) = self.services.urls.take_effect() {
            self.host_events.push_back(HostEvent::OpenLink(uri));
        }
        while let Some(effect) = self.services.clipboard.take_effect() {
            self.host_events.push_back(HostEvent::Clipboard(effect));
        }
        while let Some(effect) = self.services.transport.take_effect() {
            self.host_events.push_back(HostEvent::Transport(effect));
        }
        let wake = self.services.waker.take_requests();
        // An idle drain produces no event, timer or allocation (P2).
        if wake.frame || wake.deadline.is_some() {
            self.host_events.push_back(HostEvent::Wake(wake));
        }
    }
}

/// Observer handle is stored by Session but remains owned by its local bus.
pub(super) type FactHook = Option<HookId>;
