//! FIFO drain rounds and the cascade safety limit (design §5.3).
//!
//! Commands submitted by hooks enter the next round. Facts produced by a
//! handler are delivered after that handler returns, while facts produced by
//! observers enter the next round. This prevents synchronous re-entry and
//! gives a bound of sixteen rounds to a mistaken self-republishing hook.
//!
//! A borrowed interceptor has the same mask and priority as an owned hook.
//! The domain can retain its state for introspection without shared mutable
//! pointers; the borrow lasts only until this synchronous drain returns.
//! This seam does not expose domain messages or keybinding state to the bus.
//! The optional policy is inserted into the ordered stream once per command.
//! A registered higher-priority policy therefore can veto before the built-in;
//! a lower-priority one sees the built-in's accepted or rewritten message.
//! Built-ins win equal priority because they exist before host registration.
//! A deferred command remembers the name of the policy which suspended it.
//! On resumption the prefix of policies is skipped, then the suffix executes.
//! This keeps a buffered domain event publicly typed without repeating the
//! same veto or rewrite decisions made before it was held.
//! The origin remains the trusted host's field, independent of that marker.
//! No wire payload can supply a continuation name or bypass a policy chain.
//! A continuation whose policy vanished is not executed through a new prefix;
//! the domain normally resumes while the same built-in policy is borrowed.
//! The hook context points only at queues and a copied read-only snapshot.
//! It cannot borrow the handler's state or recursively drain the bus.
//! Commands from one round are taken as a batch so newly generated commands
//! cannot extend that round indefinitely. This is the cascade safety boundary.
//! Facts emitted by handlers are delivered before the next original command,
//! while observer follow-ups wait for the next round to prevent recursion.
//! The same ready-event lane is shared across policy and handler work, so a
//! policy-produced fact is not delayed merely because the handler was vetoed.
//! Extension filters check concrete type identity in addition to kind masks.
//! An Ext listener for one plugin must not run for another plugin's payload.
//! A mask is evaluated using the command's current kind after earlier rewrites;
//! turning a key into paste therefore selects paste policies from then on.
//! Statistics count successful handler executions rather than submissions.
//! A veto still allows already-published facts to be delivered consistently.
//! The diagnostic chain records callbacks that queued more work, making a
//! cascade-limit report explain who caused the loop instead of just its count.
//! An absent borrowed policy leaves ordinary drain behavior unchanged.
//! Listener unions keep unobserved facts cheap even when command policy exists.
//! The bus has no clock, terminal modes, transport or platform dependencies.
//! Its fairness is deterministic round ordering, not a runtime thread lock.
//! All mutable queues belong to this Bus throughout dispatch; the domain only
//! borrows its own policy alongside its handler's disjoint state fields.
use super::Bus;
use crate::{
    hooks::{BorrowedPolicy, CommandKinds, EventKinds, Flow, HookCtx, QueuedCommand},
    message::Message,
};
use std::collections::VecDeque;

/// Dispatch summary useful to the session's pump and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrainStats {
    /// Requests executed by the handler after policy.
    pub commands: usize,
    /// Facts delivered to at least one observer.
    pub events: usize,
    /// Number of cascade rounds used.
    pub rounds: usize,
}

/// Drains the queued lanes without letting a hook re-enter the bus.
impl<C: Message, E: Message> Bus<C, E> {
    /// Executes requests and delivers their facts in bounded FIFO rounds.
    pub fn drain(&mut self, handler: impl FnMut(&C, &mut HookCtx<'_, C, E>)) -> DrainStats {
        self.drain_with(None, handler)
    }

    /// Borrows one domain-owned policy while preserving global priority order.
    pub fn drain_with(
        &mut self,
        mut policy: Option<BorrowedPolicy<'_, C, E>>,
        mut handler: impl FnMut(&C, &mut HookCtx<'_, C, E>),
    ) -> DrainStats {
        // Keep the diagnostic chain for a possible cascade warning. It is
        // grown only by hooks that queue follow-up work, outside hot paths.
        let mut stats = DrainStats::default();
        let mut chain = Vec::new();
        // Sixteen rounds bound feedback loops without limiting the number
        // of independent commands in one input batch (design §5.3).
        for round in 0..16 {
            // An empty pair of lanes means there is no deferred work.
            if self.commands.is_empty() && self.events.is_empty() {
                break;
            }
            stats.rounds = round + 1;
            // Take only work present at this round's start; follow-ups wait.
            let mut commands = std::mem::take(&mut self.commands);
            let mut facts = std::mem::take(&mut self.events);
            // Taking the command lane leaves a fresh queue for hook output;
            // hooks cannot recursively execute what they just submitted.
            while let Some(pending) = commands.pop_front() {
                self.execute(
                    pending,
                    &mut facts,
                    &mut chain,
                    &mut handler,
                    &mut stats,
                    &mut policy,
                );
            }
            self.deliver_ready(&mut facts, &mut chain, &mut stats);
        }
        // A hook that continually republishes would otherwise trap the UI
        // thread forever. Report the responsible hook names before dropping.
        if !self.commands.is_empty() || !self.events.is_empty() {
            tracing::warn!(
                event = "bus.cascade_limit",
                ?chain,
                "dropping follow-up work after 16 rounds"
            );
            self.commands.clear();
            self.events.clear();
            // Clear both lanes before the debug assertion so a caller that
            // catches it in a test cannot accidentally replay stale work.
            // In debug builds this signals a programming error immediately;
            // release builds retain the bounded drop-and-warn behaviour.
            debug_assert!(false, "bus.cascade_limit: {chain:?}");
        }
        stats
    }

    /// Runs matching policies before handing one request to the session.
    fn execute(
        &mut self,
        mut pending: QueuedCommand<C>,
        facts: &mut VecDeque<E>,
        chain: &mut Vec<&'static str>,
        handler: &mut impl FnMut(&C, &mut HookCtx<'_, C, E>),
        stats: &mut DrainStats,
        policy: &mut Option<BorrowedPolicy<'_, C, E>>,
    ) {
        // The handler runs only after the ordered policy phase accepts the
        // possibly rewritten command; facts still wait until it completes.
        let accepted = self.apply_policies(&mut pending, facts, chain, policy);
        if let Some(after) = pending.resume_after {
            // INVARIANT: continuation names come from trusted domain code,
            // never external input. Report the bug before debug builds assert;
            // release still drops the command rather than bypassing policies.
            tracing::warn!(
                event = "bus.resume_unknown_target",
                hook = after,
                "dropping command whose resume policy is absent"
            );
            debug_assert!(false, "bus.resume_unknown_target: {after}");
        }
        if accepted && pending.resume_after.is_none() {
            // Only a command surviving the whole policy chain is executed.
            // Preserve its original source for handler-side policy choices.
            let mut ctx = HookCtx {
                commands: &mut self.commands,
                events: facts,
                snapshot: self.snapshot,
                origin: Some(pending.origin),
                chain,
                name: "handler",
                observed: self.observed,
            };
            handler(&pending.command, &mut ctx);
            // A vetoed command is not counted as executed work.
            stats.commands += 1;
        }
        // A handler's events are observed before the next command executes.
        self.deliver_ready(facts, chain, stats);
    }

    /// Interleaves the borrowed built-in with registered policies by priority.
    fn apply_policies(
        &mut self,
        pending: &mut QueuedCommand<C>,
        facts: &mut VecDeque<E>,
        chain: &mut Vec<&'static str>,
        policy: &mut Option<BorrowedPolicy<'_, C, E>>,
    ) -> bool {
        let mut borrowed = policy.as_mut();
        for slot in &mut self.interceptors {
            // The domain retains its hook's state. Only this drain borrows it,
            // avoiding a shared pointer or type-erased downcast in the core.
            if borrowed
                .as_ref()
                .is_some_and(|hook| hook.priority >= slot.priority)
                && let Some(hook) = borrowed.take()
                && !run_policy(
                    hook,
                    pending,
                    facts,
                    &mut self.commands,
                    chain,
                    self.snapshot,
                    self.observed,
                )
            {
                return false;
            }
            // Concrete extension filters and current kind masks both apply.
            if !slot_accepts(slot, &pending.command) {
                continue;
            }
            let mut hook = owned_policy(slot);
            if !run_policy(
                &mut hook,
                pending,
                facts,
                &mut self.commands,
                chain,
                self.snapshot,
                self.observed,
            ) {
                return false;
            }
        }
        // A lowest-priority borrowed hook still runs after all registered ones.
        borrowed.is_none_or(|hook| {
            run_policy(
                hook,
                pending,
                facts,
                &mut self.commands,
                chain,
                self.snapshot,
                self.observed,
            )
        })
    }

    /// Delivers the ready lane; observers queue their own follow-ups for later.
    fn deliver_ready(
        &mut self,
        facts: &mut VecDeque<E>,
        chain: &mut Vec<&'static str>,
        stats: &mut DrainStats,
    ) {
        while let Some(event) = facts.pop_front() {
            // A fact may have been queued before its last observer was
            // removed, so check the current mask again at delivery time.
            let bit = EventKinds::at(event.kind_index());
            if !self.observed.intersects(bit) {
                continue;
            }
            for slot in &mut self.observers {
                // Preserve the global priority order while avoiding a
                // callback for every unrelated concrete plugin type.
                if slot
                    .extension
                    .is_some_and(|id| !event.extension().is_some_and(|ext| ext.type_id() == id))
                {
                    continue;
                }
                // Priority order is fixed at registration; each listener
                // receives the same borrowed, immutable event value.
                if !slot.kinds.intersects(bit) {
                    continue;
                }
                // Observer-generated facts enter the bus's next-round lane,
                // keeping the current fact's delivery atomic.
                let mut ctx = HookCtx {
                    commands: &mut self.commands,
                    events: &mut self.events,
                    snapshot: self.snapshot,
                    origin: None,
                    chain,
                    name: slot.name,
                    observed: self.observed,
                };
                slot.hook.observe(&event, &mut ctx);
            }
            // Count delivered facts once, regardless of listener count.
            stats.events += 1;
        }
    }
}

/// Applies the same mask and context contract to owned and borrowed hooks.
fn run_policy<C: Message, E: Message>(
    hook: &mut BorrowedPolicy<'_, C, E>,
    pending: &mut QueuedCommand<C>,
    facts: &mut VecDeque<E>,
    commands: &mut VecDeque<QueuedCommand<C>>,
    chain: &mut Vec<&'static str>,
    snapshot: crate::hooks::Snapshot,
    observed: EventKinds,
) -> bool {
    // A suspended command has already passed earlier policies. Resume only
    // after its suspending hook, retaining the original public message kind.
    if let Some(after) = pending.resume_after {
        if after == hook.name {
            pending.resume_after = None;
        }
        return true;
    }
    if !hook
        .kinds
        .intersects(CommandKinds::at(pending.command.kind_index()))
    {
        return true;
    }
    let mut ctx = HookCtx {
        commands,
        events: facts,
        snapshot,
        origin: Some(pending.origin),
        chain,
        name: hook.name,
        observed,
    };
    if let Flow::Stop(reason) = hook.hook.intercept(&mut pending.command, &mut ctx) {
        tracing::debug!(hook = hook.name, reason, "command vetoed");
        false
    } else {
        true
    }
}

/// Adapts an owned slot without changing its name or priority.
fn owned_policy<C: Message, E: Message>(
    slot: &mut super::InterceptorSlot<C, E>,
) -> BorrowedPolicy<'_, C, E> {
    BorrowedPolicy {
        priority: slot.priority,
        kinds: slot.kinds,
        name: slot.name,
        hook: slot.hook.as_mut(),
    }
}

/// A concrete plugin listener cannot receive another plugin's payload.
fn slot_accepts<C: Message, E: Message>(slot: &super::InterceptorSlot<C, E>, command: &C) -> bool {
    // Extension identity refines the coarse Ext bit in the kind mask.
    slot.extension
        .is_none_or(|id| command.extension().is_some_and(|ext| ext.type_id() == id))
        && slot
            .kinds
            .intersects(CommandKinds::at(command.kind_index()))
}
