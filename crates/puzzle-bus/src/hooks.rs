//! Hook contracts and masks for generic coarse bus traffic (design §5.2).
//!
//! A hook borrows a short-lived context tied to one drain round. Follow-ups
//! enter queues rather than calling dispatch recursively. Domain enums
//! provide kind indices; the masks are independent of terminal concepts.
use crate::message::{Message, Origin};
use std::collections::VecDeque;

bitflags::bitflags! {
    /// Set of command kinds a policy may inspect.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct CommandKinds: u64 {
        /// Every possible kind bit in the current mask representation.
        const ALL = u64::MAX;
    }
}

/// Safe bit construction for a domain-owned catalogue index.
impl CommandKinds {
    /// Returns an empty filter for a kind outside the 64-bit mask.
    #[must_use]
    pub fn at(index: usize) -> Self {
        if index < 64 {
            Self::from_bits_retain(1_u64 << index)
        } else {
            Self::empty()
        }
    }
}

bitflags::bitflags! {
    /// Set of fact kinds an observer may receive.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EventKinds: u64 {
        /// Every possible kind bit in the current mask representation.
        const ALL = u64::MAX;
    }
}

/// Safe bit construction for a domain-owned event index.
impl EventKinds {
    /// Returns an empty filter rather than shifting by 64 or more.
    #[must_use]
    pub fn at(index: usize) -> Self {
        if index < 64 {
            Self::from_bits_retain(1_u64 << index)
        } else {
            Self::empty()
        }
    }
}

/// Stable registration handle shared by policy, observer and tap lanes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HookId(pub u64);

/// Whether a policy permits a command to reach its handler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Flow {
    /// Continue through remaining policies.
    Continue,
    /// Stop execution with a diagnostic reason.
    Stop(&'static str),
}

/// Small read-only state supplied by the future session owner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Current grid columns, if a terminal exists.
    pub cols: u16,
    /// Current grid rows, if a terminal exists.
    pub rows: u16,
    /// Whether input focus is active.
    pub focused: bool,
    /// Domain-defined mode flags without exposing terminal types here.
    pub mode_bits: u64,
}

/// One request and the source assigned by its trusted host.
pub struct QueuedCommand<C: Message> {
    /// Concrete request passed to policies and the handler.
    pub command: C,
    /// Origin is never read from a wire payload.
    pub origin: Origin,
    /// Deferred policies resume after this hook without re-running earlier ones.
    pub(crate) resume_after: Option<&'static str>,
}

/// Limited bus access available during one hook or handler call.
pub struct HookCtx<'a, C: Message, E: Message> {
    /// Follow-up requests enter a future cascade round.
    pub(crate) commands: &'a mut VecDeque<QueuedCommand<C>>,
    /// Facts from a handler are delivered after it returns.
    pub(crate) events: &'a mut VecDeque<E>,
    /// Copy of session state at the drain point.
    pub(crate) snapshot: Snapshot,
    /// Source of the active command, absent while observing a fact.
    pub(crate) origin: Option<Origin>,
    /// Names of hooks responsible for a cascade.
    pub(crate) chain: &'a mut Vec<&'static str>,
    /// Name of the active callback for that chain.
    pub(crate) name: &'static str,
    /// Precomputed fact-listener union for allocation-free skips.
    pub(crate) observed: EventKinds,
}

/// Safe follow-up operations that never re-enter the bus synchronously.
impl<C: Message, E: Message> HookCtx<'_, C, E> {
    /// Submits a request for the next cascade round.
    pub fn submit(&mut self, command: C, origin: Origin) {
        self.chain.push(self.name);
        self.commands.push_back(QueuedCommand {
            command,
            origin,
            resume_after: None,
        });
    }

    /// Defers a request while keeping its kind and continuing after this policy.
    ///
    /// Buffered leaders must remain ordinary domain keys for downstream hooks,
    /// but running the leader matcher twice would hold them again forever.
    pub fn resume(&mut self, command: C, origin: Origin) {
        self.chain.push(self.name);
        self.commands.push_back(QueuedCommand {
            command,
            origin,
            resume_after: Some(self.name),
        });
    }

    /// Publishes a fact only if someone observes its kind.
    pub fn publish(&mut self, event: E) {
        // Avoid even growing the hook-chain vector for an unobserved fact.
        if !self.observed.intersects(EventKinds::at(event.kind_index())) {
            return;
        }
        self.chain.push(self.name);
        self.events.push_back(event);
    }

    /// Reads state captured at the current drain point.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot
    }

    /// Reads the trusted source of the current command.
    #[must_use]
    pub fn origin(&self) -> Option<Origin> {
        self.origin
    }

    /// Emits a hook decision through structured tracing.
    pub fn debug(&self, reason: &str) {
        tracing::debug!(hook = self.name, reason, "bus hook decision");
    }
}

/// Policy called before the domain's command handler.
pub trait Interceptor<C: Message, E: Message>: Send {
    /// May rewrite the request in place or stop its execution.
    fn intercept(&mut self, command: &mut C, ctx: &mut HookCtx<'_, C, E>) -> Flow;
}

/// Listener called after a domain fact has happened.
pub trait Observer<C: Message, E: Message>: Send {
    /// Borrows a fact and may enqueue follow-up work.
    fn observe(&mut self, event: &E, ctx: &mut HookCtx<'_, C, E>);
    /// Returns a buffered fact after dispatch, for a platform-owned callback.
    /// Ordinary synchronous hooks have no outbox. A browser hook can own a
    /// plain Send queue and deliver its facts after releasing the core borrow,
    /// keeping JS handles and reentrant callbacks outside the bus (§5.4).
    fn take_event(&mut self) -> Option<E> {
        None
    }
}

/// Direction of a borrowed transport byte slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteDirection {
    /// Bytes arriving from the application.
    FromApp,
    /// Bytes sent toward the application.
    ToApp,
}

/// Opt-in zero-copy observer of raw transport data.
pub trait ByteTap: Send {
    /// Reads bytes only for the duration of this callback.
    fn tap(&mut self, direction: ByteDirection, bytes: &[u8]);
}

/// Borrowed built-in policy, so its domain owner retains introspection access.
pub struct BorrowedPolicy<'a, C: Message, E: Message> {
    /// Runs before registered policies at the same priority (built-ins are first).
    pub priority: i16,
    /// Mask avoids calls on unrelated command kinds.
    pub kinds: CommandKinds,
    /// Diagnostic identity in the cascade chain.
    pub name: &'static str,
    /// Domain-owned hook borrowed only for this synchronous drain.
    pub hook: &'a mut dyn Interceptor<C, E>,
}
