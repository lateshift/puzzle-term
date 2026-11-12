//! Executable bus rules for policy order, origins and bounded cascades.
// Test assertions intentionally panic with case-specific context when an invariant fails.
#![allow(clippy::expect_used, reason = "test assertions report failed setup")]
use super::Bus;
/// Fixture domain proves the generic bus has no terminal dependency.
mod messages;
use crate::{
    hooks::{ByteDirection, CommandKinds, Flow, HookCtx, Interceptor},
    logging::MemoryLayer,
    message::Origin,
    value::{ValidationReport, Value},
    wire::{Extension, ToWire, WireExt},
};
use messages::{Command, Event};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tracing_subscriber::prelude::*;

/// First plugin type used to install a concrete listener.
struct FirstPlugin;

/// Second plugin type must not trigger the first one's wire conversion.
struct SecondPlugin;

/// Mutable plugin command used to prove typed rewrites refresh the wire view.
struct CounterPlugin {
    /// Count changed by one typed interceptor.
    count: u32,
}

/// A scalar wire view makes stale snapshots easy to detect.
impl WireExt for CounterPlugin {
    /// Stable fixture name for the command lane.
    const TYPE_NAME: &'static str = "ext:fixture/counter";
    /// Encode the current count after every accepted rewrite.
    fn to_value(&self) -> Value {
        Value::Number(f64::from(self.count))
    }
    /// External values must be whole, nonnegative counts.
    fn from_value(value: &Value) -> Result<Self, ValidationReport> {
        if let Value::Number(count) = value
            && count.is_finite()
            && *count >= 0.0
            && *count <= f64::from(u32::MAX)
            && count.fract() == 0.0
        {
            return Ok(Self {
                count: *count as u32,
            });
        }
        let mut report = ValidationReport::default();
        report.push("payload", "expected unsigned count");
        Err(report)
    }
}

/// Both fixtures use a payload with no nested allocation.
impl WireExt for FirstPlugin {
    /// Stable fixture namespace.
    const TYPE_NAME: &'static str = "ext:fixture/first";
    /// A unit fact needs no payload fields.
    fn to_value(&self) -> Value {
        Value::Null
    }
    /// Reject a non-unit wire value with a report.
    fn from_value(value: &Value) -> Result<Self, ValidationReport> {
        if matches!(value, Value::Null) {
            Ok(Self)
        } else {
            let mut report = ValidationReport::default();
            report.push("payload", "expected null");
            Err(report)
        }
    }
}

/// The second fixture has a distinct `TypeId` and stable wire name.
impl WireExt for SecondPlugin {
    /// Stable fixture namespace.
    const TYPE_NAME: &'static str = "ext:fixture/second";
    /// A unit fact needs no payload fields.
    fn to_value(&self) -> Value {
        Value::Null
    }
    /// Reject a non-unit wire value with a report.
    fn from_value(value: &Value) -> Result<Self, ValidationReport> {
        if matches!(value, Value::Null) {
            Ok(Self)
        } else {
            let mut report = ValidationReport::default();
            report.push("payload", "expected null");
            Err(report)
        }
    }
}

/// Policy fixture that records order and optionally stops or rewrites.
struct Policy {
    /// Name recorded when this policy runs.
    name: &'static str,
    /// Shared trace of the priority sequence.
    order: Arc<Mutex<Vec<&'static str>>>,
    /// Whether later policies and the handler should be skipped.
    stop: bool,
}

/// A fixture needs the same public hook contract as application policies.
impl Interceptor<Command, Event> for Policy {
    /// Records one invocation and optionally changes committed text.
    fn intercept(&mut self, command: &mut Command, _: &mut HookCtx<'_, Command, Event>) -> Flow {
        self.order.lock().expect("test lock").push(self.name);
        if let Command::Text(text) = command {
            text.push('!');
        }
        if self.stop {
            Flow::Stop("test veto")
        } else {
            Flow::Continue
        }
    }
}

/// Higher priorities run first and equal priorities keep registration order.
#[test]
fn priority_and_in_place_rewrite() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut bus = Bus::<Command, Event>::default();
    for (name, priority) in [("low", 0), ("first", 10), ("second", 10)] {
        bus.intercept(
            CommandKinds::ALL,
            priority,
            name,
            Policy {
                name,
                order: order.clone(),
                stop: false,
            },
        )
        .expect("hook");
    }
    bus.submit(Command::Text("a".into()), Origin::User);
    let mut result = String::new();
    bus.drain(|command, _| {
        if let Command::Text(text) = command {
            result = text.clone();
        }
    });
    assert_eq!(
        *order.lock().expect("test lock"),
        ["first", "second", "low"]
    );
    assert_eq!(result, "a!!!");
}

/// A stopped command never reaches the handler.
#[test]
fn veto_prevents_execution() {
    let mut bus = Bus::<Command, Event>::default();
    bus.intercept(
        CommandKinds::ALL,
        0,
        "veto",
        Policy {
            name: "veto",
            order: Arc::new(Mutex::new(Vec::new())),
            stop: true,
        },
    )
    .expect("hook");
    bus.submit(Command::Text("hidden".into()), Origin::User);
    let stats = bus.drain(|_, _| panic!("stopped request reached handler"));
    assert_eq!(stats.commands, 0);
}

/// The cascade warning carries the hook chain before debug builds assert.
#[test]
fn cascade_limit_logs_hook_chain() {
    struct Loop;
    impl Interceptor<Command, Event> for Loop {
        /// Each bell requests another bell, deliberately exceeding the limit.
        fn intercept(&mut self, _: &mut Command, ctx: &mut HookCtx<'_, Command, Event>) -> Flow {
            ctx.submit(Command::RingBell, Origin::Plugin);
            Flow::Continue
        }
    }
    let memory = MemoryLayer::default();
    let subscriber = tracing_subscriber::registry().with(memory.clone());
    tracing::subscriber::with_default(subscriber, || {
        let mut bus = Bus::<Command, Event>::default();
        bus.intercept(CommandKinds::at(1), 0, "loop", Loop)
            .expect("hook");
        bus.submit(Command::RingBell, Origin::User);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bus.drain(|_, _| {})));
    });
    let records = memory.records();
    assert!(records.iter().any(|record| {
        record
            .fields
            .get("event")
            .is_some_and(|value| value.contains("bus.cascade_limit"))
    }));
    assert!(records.iter().any(|record| {
        record
            .fields
            .get("chain")
            .is_some_and(|value| value.contains("loop"))
    }));
}

/// Unsubscribed facts and disabled taps perform zero allocations (P1, P8).
#[test]
fn idle_publish_and_tap_allocate_nothing() {
    let mut bus = Bus::<Command, Event>::default();
    let published = allocation_counter::measure(|| bus.publish(Event::Bell));
    let tapped = allocation_counter::measure(|| bus.tap(ByteDirection::FromApp, b"abc"));
    // A handler-generated unobserved fact must also avoid allocating its
    // follow-up lane or growing the diagnostic hook chain.
    bus.submit(Command::RingBell, Origin::User);
    let handler_fact = allocation_counter::measure(|| {
        bus.drain(|_, ctx| ctx.publish(Event::Bell));
    });
    assert_eq!(published.count_total, 0);
    assert_eq!(tapped.count_total, 0);
    assert_eq!(handler_fact.count_total, 0);
}

/// An unrelated typed extension listener must not make publication allocate.
#[test]
fn unrelated_extension_publish_allocates_nothing() {
    let mut bus = Bus::<Command, Event>::default();
    let seen = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&seen);
    bus.observe_ext::<FirstPlugin>(0, "first", move |_, _| {
        counter.fetch_add(1, Ordering::Relaxed);
    })
    .expect("fixture registration");
    let allocation = allocation_counter::measure(|| {
        bus.publish_ext(SecondPlugin)
            .expect("unobserved publication");
    });
    assert_eq!(allocation.count_total, 0);
    // A matching event still reaches the registered typed callback.
    bus.publish_ext(FirstPlugin).expect("observed publication");
    bus.drain(|_, _| {});
    assert_eq!(seen.load(Ordering::Relaxed), 1);
}

/// Typed plugin policy rewrites reach later consumers in both Rust and wire form.
#[test]
fn typed_extension_interceptor_refreshes_wire_view() {
    let mut bus = Bus::<Command, Event>::default();
    bus.intercept_ext::<CounterPlugin>(0, "counter", |counter, _| {
        counter.count += 1;
        Flow::Continue
    })
    .expect("plugin policy");
    bus.submit(
        Command::Ext(Extension::new(CounterPlugin { count: 4 }).expect("plugin value")),
        Origin::Plugin,
    );
    let mut seen = None;
    bus.drain(|command, _| {
        if let Command::Ext(extension) = command {
            seen = Some(extension.to_value().expect("updated wire view"));
        }
    });
    assert_eq!(seen, Some(Value::Number(5.0)));
}

/// Direct witnesses for domain-owned policies and deferred commands (§5.3 rules 6–7).
mod borrowed;

/// Typed event buffering stays mask-filtered and releases with its hook.
mod outbox;
