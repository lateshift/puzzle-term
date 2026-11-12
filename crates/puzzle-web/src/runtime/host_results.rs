//! Browser dispatch of D-50's one Session result queue.
//!
//! Taking results moves owned values; it never invokes JS under a Session borrow.
//! Each platform consumer keeps its FIFO so native clipboard gestures, ordered
//! transport writes and convenience callbacks retain their existing delivery order.
//! Wake requests coalesce independently of foreign callbacks and never poll.
use super::Runtime;
use puzzle_term::platform::{ClipboardEffect, TransportEffect, WakeRequests};
use puzzle_term::{HostEvent, SessionMessage, keybind::Action, messages::Event};
use std::collections::VecDeque;
use web_time::Instant;

/// Reusable platform-owned pending deliveries, containing no core callbacks.
#[derive(Default)]
pub(super) struct HostResults {
    /// Facts await callback delivery after every preceding transport transaction.
    facts: VecDeque<Event>,
    /// Write/resize/close stay in their single producer's execution order.
    transport: VecDeque<TransportEffect>,
    /// Clipboard writes remain available inside the original key gesture.
    clipboard: VecDeque<ClipboardEffect>,
    /// URLs stay queued until a gesture turn can open without holding the runtime.
    urls: VecDeque<String>,
    /// Settings work follows the resolved appearance captured by Session.
    actions: VecDeque<(Action, bool, bool)>,
    /// Repeated damage and timer requests need only one scheduling record.
    wake: WakeRequests,
    /// Native insertion is decided anew by each printable key message.
    pub(super) native_text: bool,
    /// Browser default suppression follows the last pointer message's result.
    pub(super) pointer_handled: bool,
}

/// A synchronous web turn uses the same message/event seam as a desktop owner.
impl Runtime {
    /// Applies a host mutation and dispatches its values without running foreign code.
    pub(super) fn apply(&mut self, message: SessionMessage) -> Result<(), String> {
        let result = self.session.apply(message, Instant::now());
        // Rejected geometry may still produce valid diagnostics or earlier effects.
        self.collect_results();
        result
    }

    /// Collects parser or message results before any individual platform consumer drains.
    fn collect_results(&mut self) {
        // A turn may parse bytes without an explicit host message; drain those results too.
        // Category queues preserve the existing host order: writes precede JS facts.
        while let Some(event) = self.session.take_event() {
            match event {
                HostEvent::Fact(event) => self.results.facts.push_back(event),
                HostEvent::Transport(effect) => self.results.transport.push_back(effect),
                HostEvent::Clipboard(effect) => self.results.clipboard.push_back(effect),
                HostEvent::OpenLink(uri) => self.results.urls.push_back(uri),
                // The appearance belongs to action execution, not eventual JS dispatch.
                HostEvent::BoundAction { action, all, dark } => {
                    self.results.actions.push_back((action, all, dark));
                }
                HostEvent::Wake(wake) => {
                    self.results.wake.frame |= wake.frame;
                    if let Some(at) = wake.deadline {
                        // Independent core turns share the earliest finite deadline.
                        self.results.wake.deadline =
                            Some(self.results.wake.deadline.map_or(at, |old| old.min(at)));
                    }
                }
                // Only an acknowledged image returns storage; skipped frames never change hits.
                HostEvent::RegionsReturned(regions) => self.render.regions = regions,
                HostEvent::CursorChanged(cursor) => {
                    if let Some(dom) = &self.dom {
                        // Hit testing stays with Session; this only maps its portable result.
                        let css = match cursor {
                            puzzle_term::overlay::draw::CursorIcon::Default
                            | puzzle_term::overlay::draw::CursorIcon::Text => "text",
                            puzzle_term::overlay::draw::CursorIcon::Pointer => "pointer",
                            puzzle_term::overlay::draw::CursorIcon::Grab => "grab",
                            puzzle_term::overlay::draw::CursorIcon::Grabbing => "grabbing",
                        };
                        let _ = dom.canvas().style().set_property("cursor", css);
                    }
                }
                // These dispositions are consumed synchronously by the DOM event callback.
                // They prevent duplicate keydown/beforeinput encoding on the one web thread.
                HostEvent::NativeTextAllowed(allowed) => self.results.native_text = allowed,
                HostEvent::PointerHandled(handled) => self.results.pointer_handled = handled,
                // Native optional host effects have no DOM adapter (D-42). No
                // optional effect handler is registered in this web runtime.
                HostEvent::OscEffect(_) => {},
                // Native drag registration has no DOM adapter until the wasm phase (D-42).
                HostEvent::Drop(_) => {},
                // Native find contracts have no JS adapter here. The browser still
                // receives its existing SearchRequested fact and reads selection
                // directly on its owning thread; no web behavior changes (D-42).
                HostEvent::SearchRequested | HostEvent::SelectedText { .. } => {},
                // The current JS subset has no asynchronous registration API (M10).
                HostEvent::OverlayAdded { .. } | HostEvent::DecorationAdded { .. } | HostEvent::MarkerAdded { .. }
                // Web policy queries remain direct on its one owning thread.
                | HostEvent::InputState(_) | HostEvent::AnimationChanged(_)
                | HostEvent::SearchState(_) | HostEvent::Colors { .. } | HostEvent::Activity => {}
            }
        }
    }

    /// Takes one approved URI inside the synchronous pointer gesture.
    pub(super) fn take_open_link(&mut self) -> Option<String> {
        self.collect_results();
        self.results.urls.pop_front()
    }
    /// Delivers only completed facts, after transport callbacks have relinquished ownership.
    pub(super) fn take_fact(&mut self) -> Option<Event> {
        self.collect_results();
        self.results.facts.pop_front()
    }
    /// Ordered transport ownership can move into JS only after this method returns.
    pub(super) fn take_transport_effect(&mut self) -> Option<TransportEffect> {
        self.collect_results();
        self.results.transport.pop_front()
    }
    /// The DOM key callback consumes clipboard ownership before user activation expires.
    pub(super) fn take_clipboard_effect(&mut self) -> Option<ClipboardEffect> {
        self.collect_results();
        self.results.clipboard.pop_front()
    }
    /// Settings actions use their execution-time appearance, even after later output.
    pub(super) fn take_binding_action(&mut self) -> Option<(Action, bool, bool)> {
        self.collect_results();
        self.results.actions.pop_front()
    }
    /// Scheduling drains one coalesced record without constructing a frame.
    pub(super) fn take_wake_requests(&mut self) -> WakeRequests {
        self.collect_results();
        std::mem::take(&mut self.results.wake)
    }
}
