//! Contents and host facts agree without allocating snapshots for unobserved data.
use super::*;
use crate::HostEvent;

#[test]
fn contexts_coalesce_per_drain_survive_ris_and_end_with_session() {
    let (mut session, _, _) = fixture();
    session.enable_host_events().expect("host observation");
    let now = Instant::now();
    session.feed(
        b"\x1b]3008;start=root;type=shell\x1b\\\x1b]3008;start=ssh;type=remote\x1b\\\x1bc",
        now,
    );
    assert_eq!(session.buffer().contexts().len(), 2);
    let paths = std::iter::from_fn(|| session.take_event())
        .filter_map(|e| match e {
            crate::HostEvent::Fact(Event::ContextsChanged(path)) => Some(path),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(paths, [session.buffer().contexts()]);
    session.feed(b"\x1bc", now);
    assert!(
        !std::iter::from_fn(|| session.take_event())
            .any(|e| matches!(e, HostEvent::Fact(Event::ContextsChanged(_))))
    );
    session.on_exit = OnExit::Hold;
    session.submit(
        Command::ChildExited(ExitInfo {
            code: Some(0),
            signal: None,
            runtime_ms: 1000,
        }),
        Origin::Platform,
    );
    session.pump(now);
    assert!(session.buffer().contexts().is_empty());
    let facts = std::iter::from_fn(|| session.take_event())
        .filter_map(|e| match e {
            HostEvent::Fact(e) => Some(e),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(matches!(facts.first(), Some(Event::ContextsChanged(path)) if path.is_empty()));
    assert!(facts.iter().any(|e| matches!(e, Event::SessionExited(_))));
}

#[test]
fn unused_contexts_and_unobserved_payloads_do_not_allocate_during_warm_parse() {
    let (mut session, _, _) = fixture();
    let now = Instant::now();
    session.feed(b"ordinary\r", now);
    let result = allocation_counter::measure(|| {
        for _ in 0..100 {
            session.feed(b"ordinary\r", now);
        }
    });
    assert_eq!(result.count_total, 0);
    assert!(session.buffer().contexts().is_empty());
}
