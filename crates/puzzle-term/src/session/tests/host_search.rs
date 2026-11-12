//! Search count delivery follows the owner turns, never a synchronous host query.
use super::*;
use crate::{HostEvent, search::SearchCommand, viewport::ScrollTarget};

/// Keep only public progress deliveries; other lifecycle facts remain independently tested.
fn states(session: &mut TestHost) -> Vec<crate::search::SearchState> {
    std::iter::from_fn(|| session.take_event())
        .filter_map(|event| match event {
            HostEvent::SearchState(state) => Some(state),
            _ => None,
        })
        .collect()
}

/// A large query yields, navigation updates the current count, and clear is observable.
#[test]
fn search_progress_navigation_and_clear_are_change_only() {
    let (mut session, _, _) = fixture();
    let now = Instant::now();
    for _ in 0..2500 {
        session.feed(b"hit\r\n", now);
    }
    session.enable_host_events().expect("host observation");
    let initial = states(&mut session);
    assert_eq!(initial.len(), 1);
    assert!(initial[0].complete);
    assert_eq!(initial[0].count, 0);
    session.submit(
        Command::Search(SearchCommand::Find {
            query: "hit".into(),
            options: Default::default(),
        }),
        Origin::Api,
    );
    session.pump(now);
    let started = states(&mut session);
    assert_eq!(started.len(), 1);
    assert!(!started[0].complete);
    assert_eq!(started[0].count, 0, "input must not scan history");
    session.publish(now);
    let first = states(&mut session);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].count, 2000);
    assert!(!first[0].complete);
    session.publish(now);
    let complete = states(&mut session);
    assert_eq!(complete.len(), 1);
    assert!(complete[0].complete);
    assert_eq!(complete[0].count, 2500);
    for (delta, current) in [(1, 1), (1, 2), (-1, 1)] {
        session.submit(Command::ScrollTo(ScrollTarget::Match(delta)), Origin::Api);
        session.pump(now);
        let changed = states(&mut session);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].current, Some(current));
        assert_eq!(changed[0].count, 2500);
    }
    assert!(states(&mut session).is_empty());
    session.submit(Command::Search(SearchCommand::Clear), Origin::Api);
    session.pump(now);
    let cleared = states(&mut session);
    assert_eq!(cleared.len(), 1);
    assert_eq!(cleared[0], initial[0]);
    assert!(states(&mut session).is_empty());
}
