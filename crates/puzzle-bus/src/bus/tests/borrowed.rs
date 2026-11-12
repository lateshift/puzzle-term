//! Borrowed policy contracts tested without Session or terminal messages.
//!
//! These tests pin priority, rewrites, vetoes, FIFO and bounded continuations
//! at the generic bus surface. A missing trusted resume name warns and asserts
//! in debug; release drops it rather than bypassing the expected policies.
use super::*;
use crate::hooks::BorrowedPolicy;

/// Inserts owned policies around the domain's priority, including a tie.
fn surrounding(order: &Arc<Mutex<Vec<&'static str>>>) -> Bus<Command, Event> {
    let mut bus = Bus::default();
    for (name, priority) in [("earlier", 20), ("equal", 10), ("later", 0)] {
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
        .expect("policy registration");
    }
    bus
}

/// The built-in wins ties, rewrites before later hooks and can veto them.
#[test]
fn borrowed_priority_rewrite_and_veto() {
    for stop in [false, true] {
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut bus = surrounding(&order);
        let mut policy = Policy {
            name: "domain",
            order: order.clone(),
            stop,
        };
        bus.submit(Command::Text("original".into()), Origin::User);
        let mut result = Vec::new();
        let stats = bus.drain_with(
            Some(BorrowedPolicy {
                priority: 10,
                kinds: CommandKinds::ALL,
                name: "domain",
                hook: &mut policy,
            }),
            |cmd, _| {
                if let Command::Text(text) = cmd {
                    result.push(text.clone());
                }
            },
        );
        if stop {
            assert_eq!(*order.lock().expect("order"), ["earlier", "domain"]);
            assert_eq!(stats.commands, 0);
            assert!(result.is_empty());
        } else {
            assert_eq!(
                *order.lock().expect("order"),
                ["earlier", "domain", "equal", "later"]
            );
            assert_eq!(result, ["original!!!!"]);
            assert_eq!(stats.commands, 1);
        }
    }
}

/// External resume joins the same FIFO and skips the named policy inclusively.
#[test]
fn resumed_fifo_suffix_and_unknown_name() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut bus = surrounding(&order);
    let mut policy = Policy {
        name: "domain",
        order: order.clone(),
        stop: false,
    };
    bus.submit(Command::Text("first".into()), Origin::User);
    bus.resume(Command::Text("held".into()), Origin::Api, "domain");
    bus.submit(Command::Text("last".into()), Origin::User);
    let mut result = Vec::new();
    let stats = bus.drain_with(
        Some(BorrowedPolicy {
            priority: 10,
            kinds: CommandKinds::ALL,
            name: "domain",
            hook: &mut policy,
        }),
        |cmd, ctx| {
            if let Command::Text(text) = cmd {
                result.push((text.clone(), ctx.origin()));
            }
        },
    );
    assert_eq!(
        result,
        [
            ("first!!!!".into(), Some(Origin::User)),
            ("held!!".into(), Some(Origin::Api)),
            ("last!!!!".into(), Some(Origin::User)),
        ]
    );
    assert_eq!(
        *order.lock().expect("order"),
        [
            "earlier", "domain", "equal", "later", "equal", "later", "earlier", "domain", "equal",
            "later",
        ]
    );
    assert_eq!(stats.commands, 3);
    // Resuming an owned policy uses the same inclusive prefix rule.
    bus.resume(Command::Text("owned".into()), Origin::User, "equal");
    bus.drain(|cmd, _| assert!(matches!(cmd, Command::Text(s) if s == "owned!")));
}

/// An absent trusted target warns before asserting and never bypasses policies.
#[test]
fn unknown_resume_target_warns_and_asserts_in_debug() {
    let memory = MemoryLayer::default();
    let subscriber = tracing_subscriber::registry().with(memory.clone());
    tracing::subscriber::with_default(subscriber, || {
        let mut bus = Bus::<Command, Event>::default();
        bus.resume(Command::Text("missing".into()), Origin::Api, "unknown");
        let mut handled = false;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bus.drain(|_, _| handled = true)
        }));
        // The same test runs in release: only the diagnostic survives there.
        assert_eq!(result.is_err(), cfg!(debug_assertions));
        assert!(!handled, "unknown policy must not grant a handler bypass");
        if let Ok(stats) = result {
            assert_eq!(stats.commands, 0);
        }
        assert_eq!(
            bus.drain(|_, _| panic!("dropped request retained")).rounds,
            0
        );
    });
    let records = memory.records();
    let warnings: Vec<_> = records
        .iter()
        .filter(|r| {
            r.fields
                .get("event")
                .is_some_and(|v| v.contains("bus.resume_unknown_target"))
        })
        .collect();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].level, tracing::Level::WARN);
    assert!(warnings[0].fields["hook"].contains("unknown"));
}

/// A domain can defer a command without granting it another trip through itself.
struct Defer;
/// The deferred copy keeps its trusted origin and waits for the next round.
impl Interceptor<Command, Event> for Defer {
    fn intercept(&mut self, cmd: &mut Command, ctx: &mut HookCtx<'_, Command, Event>) -> Flow {
        if let Command::Text(text) = cmd {
            ctx.resume(
                Command::Text(text.clone()),
                ctx.origin().expect("command origin"),
            );
        }
        Flow::Stop("held by fixture domain")
    }
}

/// HookCtx resume waits behind every command already queued for that next round.
#[test]
fn hook_resume_fifo_and_later_handler() {
    let mut bus = Bus::<Command, Event>::default();
    let mut domain = Defer;
    for text in ["one", "two", "three"] {
        bus.submit(Command::Text(text.into()), Origin::User);
    }
    let mut result = Vec::new();
    let stats = bus.drain_with(
        Some(BorrowedPolicy {
            priority: 10,
            kinds: CommandKinds::ALL,
            name: "domain",
            hook: &mut domain,
        }),
        |cmd, _| {
            if let Command::Text(s) = cmd {
                result.push(s.clone());
            }
        },
    );
    assert_eq!(result, ["one", "two", "three"]);
    assert_eq!(stats.rounds, 2);
    assert_eq!(stats.commands, 3);
}

/// A downstream policy deliberately generates new work after each continuation.
struct Again(Arc<AtomicUsize>);
/// Fresh submissions re-enter the domain, creating an alternating two-round cycle.
impl Interceptor<Command, Event> for Again {
    fn intercept(&mut self, _: &mut Command, ctx: &mut HookCtx<'_, Command, Event>) -> Flow {
        self.0.fetch_add(1, Ordering::Relaxed);
        ctx.submit(Command::Text("cycle".into()), Origin::Plugin);
        Flow::Continue
    }
}

/// Resume consumes a round and leaves its domain identity in the cascade report.
#[test]
fn borrowed_resume_cascade_limit_and_chain() {
    let memory = MemoryLayer::default();
    let subscriber = tracing_subscriber::registry().with(memory.clone());
    let seen = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(subscriber, || {
        let mut bus = Bus::<Command, Event>::default();
        let mut domain = Defer;
        bus.intercept(CommandKinds::ALL, 0, "again", Again(seen.clone()))
            .expect("later hook");
        bus.submit(Command::Text("cycle".into()), Origin::User);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bus.drain_with(
                Some(BorrowedPolicy {
                    priority: 10,
                    kinds: CommandKinds::ALL,
                    name: "domain",
                    hook: &mut domain,
                }),
                |_, _| {},
            );
        }));
        assert_eq!(result.is_err(), cfg!(debug_assertions));
        // The overflow lanes are cleared even if the debug assertion unwinds.
        assert_eq!(
            bus.drain(|_, _| panic!("overflow request retained")).rounds,
            0
        );
    });
    assert_eq!(seen.load(Ordering::Relaxed), 8);
    let records = memory.records();
    assert!(records.iter().any(|r| {
        r.fields
            .get("event")
            .is_some_and(|v| v.contains("bus.cascade_limit"))
    }));
    assert!(records.iter().any(|r| {
        r.fields
            .get("chain")
            .is_some_and(|v| v.contains("domain") && v.contains("again"))
    }));
}
