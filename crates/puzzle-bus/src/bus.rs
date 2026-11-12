//! Registration and enqueue side of the deterministic bus (design §5.2–§5.4).
//!
//! Hook order is fixed when registered: higher priority first, then earlier
//! registration. The combined observer mask is updated only when the hook
//! table changes. An unobserved `publish` therefore reads one integer and
//! returns without allocating or touching a queue.
mod drain;
/// Typed plugin hook adapters for the extension mask bit.
mod ext;
/// Summary of work completed by one bounded drain.
pub use drain::DrainStats;

use crate::{
    hooks::{
        ByteDirection, ByteTap, CommandKinds, EventKinds, HookId, Interceptor, Observer,
        QueuedCommand, Snapshot,
    },
    message::{Message, Origin},
    value::{ValidationReport, Value},
    wire::{Extension, ExtensionRegistry, WireExt},
};
use std::{
    any::TypeId,
    collections::{HashMap, VecDeque},
};

/// One registered command policy and its stable order keys.
pub(super) struct InterceptorSlot<C: Message, E: Message> {
    /// Handle used for removal and tie ordering.
    pub(super) id: HookId,
    /// Larger values run before smaller values.
    pub(super) priority: i16,
    /// Stable diagnostic name supplied by the registrant.
    pub(super) name: &'static str,
    /// Precomputed command filter.
    pub(super) kinds: CommandKinds,
    /// Concrete plugin type, if this policy registered through `intercept_ext`.
    pub(super) extension: Option<TypeId>,
    /// Owned policy callback.
    pub(super) hook: Box<dyn Interceptor<C, E>>,
}

/// One registered event listener and its stable order keys.
pub(super) struct ObserverSlot<C: Message, E: Message> {
    /// Handle used for removal and tie ordering.
    pub(super) id: HookId,
    /// Larger values run before smaller values.
    pub(super) priority: i16,
    /// Stable diagnostic name supplied by the registrant.
    pub(super) name: &'static str,
    /// Precomputed event filter.
    pub(super) kinds: EventKinds,
    /// Concrete plugin type, if this observer registered through `observe_ext`.
    pub(super) extension: Option<TypeId>,
    /// Owned read-only callback.
    pub(super) hook: Box<dyn Observer<C, E>>,
}

/// One opt-in byte-stream tap.
struct TapSlot {
    /// Handle shared with other hook registrations.
    id: HookId,
    /// Raw slices are borrowed only during the tap call.
    hook: Box<dyn ByteTap>,
}

/// Queued bus with independent command and event lanes.
pub struct Bus<C: Message, E: Message> {
    /// Requests not yet processed by a drain point.
    pub(super) commands: VecDeque<QueuedCommand<C>>,
    /// Facts not yet delivered to observers.
    pub(super) events: VecDeque<E>,
    /// Sorted command policies.
    pub(super) interceptors: Vec<InterceptorSlot<C, E>>,
    /// Sorted fact listeners.
    pub(super) observers: Vec<ObserverSlot<C, E>>,
    /// Optional raw-byte observers.
    taps: Vec<TapSlot>,
    /// Bitwise union of the registered observer masks.
    pub(super) observed: EventKinds,
    /// Listener counts by plugin type, avoiding unrelated extension allocation.
    ext_observed: HashMap<TypeId, usize>,
    /// A generic event observer may request every plugin type.
    any_ext_observed: bool,
    /// The next never-reused registration handle.
    next_id: u64,
    /// Read-only session state supplied at a drain point.
    pub(super) snapshot: Snapshot,
}

/// Empty lanes and registries do not require default domain messages.
impl<C: Message, E: Message> Default for Bus<C, E> {
    /// Starts with no listeners, so unobserved facts are immediately cheap.
    fn default() -> Self {
        Self {
            commands: VecDeque::new(),
            events: VecDeque::new(),
            interceptors: Vec::new(),
            observers: Vec::new(),
            taps: Vec::new(),
            observed: EventKinds::empty(),
            ext_observed: HashMap::new(),
            any_ext_observed: false,
            next_id: 0,
            snapshot: Snapshot::default(),
        }
    }
}

/// Registration and enqueue methods used by the later session layer.
impl<C: Message, E: Message> Bus<C, E> {
    /// Stores a request with a trusted host-assigned source.
    pub fn submit(&mut self, command: C, origin: Origin) {
        self.commands.push_back(QueuedCommand {
            command,
            origin,
            resume_after: None,
        });
    }

    /// Resumes buffered domain input after a named policy at a later pump.
    ///
    /// The domain supplies a trusted hook name; this is not wire metadata.
    /// If the name is absent at the next drain, the command is dropped without
    /// running its handler: losing a policy must never grant a bypass.
    /// This programming error logs `bus.resume_unknown_target` and asserts
    /// in debug builds; release builds retain the drop-and-warn behavior.
    pub fn resume(&mut self, command: C, origin: Origin, after: &'static str) {
        self.commands.push_back(QueuedCommand {
            command,
            origin,
            resume_after: Some(after),
        });
    }

    /// Converts an external action, attaching `Origin::Api` only on success.
    pub fn submit_wire(
        &mut self,
        name: &str,
        value: &Value,
        extensions: &ExtensionRegistry,
    ) -> Result<(), ValidationReport> {
        // Decode before enqueueing so malformed external input never enters
        // the trusted command lane or triggers a policy callback.
        let command = C::from_value(name, value, extensions)?;
        // The host assigns origin after decoding; wire callers cannot claim
        // to be the terminal or a physical user (design §5.7).
        self.submit(command, Origin::Api);
        Ok(())
    }

    /// Enqueues an observed fact; with no listener this is one mask test.
    pub fn publish(&mut self, event: E) {
        // The union mask is rebuilt on registration changes. This early
        // branch is the measured no-allocation unobserved path (design §5.3).
        if self.observed.intersects(EventKinds::at(event.kind_index())) {
            // Preserve FIFO order for events that have any listener.
            self.events.push_back(event);
        }
    }

    /// Whether an owner callback queued facts after the command drain finished.
    /// Hosts use this O(1) test to deliver lifecycle/source deltas in the same turn.
    #[must_use]
    pub fn has_pending_events(&self) -> bool {
        !self.events.is_empty()
    }

    /// Tests an opt-in kind before constructing an expensive event payload.
    #[must_use]
    pub fn observes(&self, kinds: EventKinds) -> bool {
        // Frame statistics and output-chunk maps are built by their owning
        // subsystem only when this precomputed union contains the kind.
        self.observed.intersects(kinds)
    }

    /// Publishes a typed extension fact under its registered wire name.
    pub fn publish_ext<T: WireExt>(&mut self, value: T) -> Result<(), ValidationReport> {
        // A different plugin's listener does not justify boxing or
        // serializing this event. Generic observers still see every type.
        if self.any_ext_observed || self.ext_observed.contains_key(&TypeId::of::<T>()) {
            self.publish(E::from_extension(Extension::new(value)?));
        }
        Ok(())
    }

    /// Supplies modes, dimensions and focus for subsequent hook calls.
    pub fn set_snapshot(&mut self, snapshot: Snapshot) {
        // Hooks read a copy of session state supplied at the drain point,
        // rather than borrowing mutable terminal internals through the bus.
        self.snapshot = snapshot;
    }

    /// Registers a command interceptor at a deterministic priority.
    pub fn intercept(
        &mut self,
        kinds: CommandKinds,
        priority: i16,
        name: &'static str,
        hook: impl Interceptor<C, E> + 'static,
    ) -> Result<HookId, &'static str> {
        // Sorting on registration moves cost out of every command drain;
        // the ID gives same-priority hooks stable registration order.
        let id = self.allocate_id()?;
        self.interceptors.push(InterceptorSlot {
            id,
            priority,
            name,
            kinds,
            extension: None,
            hook: Box::new(hook),
        });
        self.interceptors
            .sort_by(|a, b| b.priority.cmp(&a.priority).then(a.id.0.cmp(&b.id.0)));
        Ok(id)
    }

    /// Registers a read-only fact observer at a deterministic priority.
    pub fn observe(
        &mut self,
        kinds: EventKinds,
        priority: i16,
        name: &'static str,
        hook: impl Observer<C, E> + 'static,
    ) -> Result<HookId, &'static str> {
        // Listener masks are unioned once after the table changes, keeping
        // unobserved publish independent of observer count.
        let id = self.allocate_id()?;
        self.observers.push(ObserverSlot {
            id,
            priority,
            name,
            kinds,
            extension: None,
            hook: Box::new(hook),
        });
        self.observers
            .sort_by(|a, b| b.priority.cmp(&a.priority).then(a.id.0.cmp(&b.id.0)));
        self.recompute_observed();
        Ok(id)
    }

    /// Takes one queued fact without exposing or downcasting hook state.
    /// Delivery happens after a synchronous drain, so a JS callback can submit
    /// new commands without reentering an active observer. Removed handles and
    /// hooks without an outbox return None; hook masks still decide what queues.
    pub fn take_observed(&mut self, id: HookId) -> Option<E> {
        let slot = self.observers.iter_mut().find(|slot| slot.id == id)?;
        slot.hook.take_event()
    }

    /// Registers a zero-copy raw-byte tap used only when explicitly called.
    pub fn tap_bytes(&mut self, hook: impl ByteTap + 'static) -> Result<HookId, &'static str> {
        let id = self.allocate_id()?;
        self.taps.push(TapSlot {
            id,
            hook: Box::new(hook),
        });
        Ok(id)
    }

    /// Reads one borrowed byte slice without cloning it for each tap.
    pub fn tap(&mut self, direction: ByteDirection, bytes: &[u8]) {
        for slot in &mut self.taps {
            slot.hook.tap(direction, bytes);
        }
    }

    /// Removes any hook kind and refreshes the observed-kind union.
    pub fn remove(&mut self, id: HookId) -> bool {
        // A handle may refer to any hook lane; recompute the event mask
        // after removal so later publishes can again take the cheap path.
        let before = self.interceptors.len() + self.observers.len() + self.taps.len();
        self.interceptors.retain(|slot| slot.id != id);
        self.observers.retain(|slot| slot.id != id);
        self.taps.retain(|slot| slot.id != id);
        self.recompute_observed();
        before != self.interceptors.len() + self.observers.len() + self.taps.len()
    }

    /// Allocates a handle without silently reusing one after overflow.
    fn allocate_id(&mut self) -> Result<HookId, &'static str> {
        // Reusing an exhausted handle could remove the wrong hook, so report
        // overflow rather than wrapping the monotone registration counter.
        let next = self.next_id.checked_add(1).ok_or("bus hook ID exhausted")?;
        self.next_id = next;
        Ok(HookId(next))
    }

    /// Precomputes the listener test used by `publish`.
    fn recompute_observed(&mut self) {
        self.observed = self
            .observers
            .iter()
            .fold(EventKinds::empty(), |mask, slot| mask | slot.kinds);
        // Listener churn is cold, so rebuild the concrete-type index here
        // rather than hashing every unobserved publish on the hot path.
        self.ext_observed.clear();
        self.any_ext_observed = false;
        let ext_bit = EventKinds::at(E::EXT_INDEX);
        for slot in &self.observers {
            if !slot.kinds.intersects(ext_bit) {
                continue;
            }
            if let Some(type_id) = slot.extension {
                *self.ext_observed.entry(type_id).or_default() += 1;
            } else {
                self.any_ext_observed = true;
            }
        }
    }
}

#[cfg(test)]
/// Dispatch order, policy, cascade and allocation guarantees.
mod tests;
