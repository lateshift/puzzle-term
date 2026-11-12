//! Tab activity must follow accepted output and explicit per-session seen messages.
use super::*;
use crate::SessionMessage;

/// Chunking, idle redraws and another pane cannot change the unread lifecycle.
#[test]
fn activity_is_one_output_edge_per_host_acknowledgement() {
    let (mut first, _, _) = fixture();
    let (mut second, _, _) = fixture();
    first.enable_host_events().expect("first host");
    second.enable_host_events().expect("second host");
    let now = Instant::now();
    first.feed(b"first", now);
    second.feed(b"other pane", now);
    assert_eq!(activities(&mut first), 1);
    assert_eq!(activities(&mut second), 1);
    for _ in 0..100 {
        first.feed(b"more", now);
    }
    assert_eq!(activities(&mut first), 0);
    first
        .apply(SessionMessage::ActivitySeen, now)
        .expect("seen");
    first.feed(&[], now);
    first
        .apply(SessionMessage::SurfaceSize([800, 600]), now)
        .expect("size");
    first.pump(now);
    assert_eq!(activities(&mut first), 0);
    // The borrowed synchronized-output route is a separate admission path.
    assert_eq!(first.feed_to_sync_end(b"fresh", now), 5);
    second.feed(b"still unread", now);
    assert_eq!(activities(&mut first), 1);
    assert_eq!(activities(&mut second), 0);
    first
        .apply(SessionMessage::ActivitySeen, now)
        .expect("seen again");
    first.feed(b"\x1b[?2026hframe\x1b[?2026l", now);
    assert_eq!(activities(&mut first), 1);
}

/// Draining an event is delivery only, never the host's seen acknowledgement.
fn activities(session: &mut TestHost) -> usize {
    std::iter::from_fn(|| session.take_event())
        .filter(|event| matches!(event, crate::HostEvent::Activity))
        .count()
}
