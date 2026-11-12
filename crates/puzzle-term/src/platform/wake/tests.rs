//! Real owner-local wake implementation retains final output and actual silence.
#![allow(
    clippy::expect_used,
    reason = "fixtures require the wake whose timing they assert"
)]
use super::*;
use std::time::Duration;

/// Delivering a host event cannot multiply wakes during a flood or lose the final write.
#[test]
fn delivery_and_publication_have_distinct_ownership() {
    let mut wake = PublicationWaker::default();
    let now = Instant::now();
    wake.request_at(now);
    let first = wake.take_requests();
    assert!(first.frame);
    assert_eq!(first.arrival.expect("output wake").first, now);
    for step in 1..10_000 {
        wake.request_at(now + Duration::from_micros(step));
        assert!(!wake.take_requests().frame);
    }
    wake.acknowledge_frame();
    wake.request_at(now + Duration::from_millis(10));
    let next = wake.take_requests().arrival.expect("output wake");
    assert_eq!(next.previous, Some(now + Duration::from_micros(9_999)));
    wake.acknowledge_frame();
    wake.request_at(now + Duration::from_millis(100));
    assert_eq!(
        wake.take_requests().arrival.expect("output wake").previous,
        Some(next.first)
    );
    wake.wake_at(now + Duration::from_millis(200));
    wake.wake_at(now + Duration::from_millis(150));
    assert_eq!(
        wake.take_requests().deadline,
        Some(now + Duration::from_millis(150))
    );
    let empty = wake.take_requests();
    assert!(!empty.frame && empty.deadline.is_none() && empty.arrival.is_none());
}
