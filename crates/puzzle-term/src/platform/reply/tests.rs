//! A non-reading transport cannot manufacture credit by replaying completion IDs.
#![allow(
    clippy::unwrap_used,
    reason = "test failures identify broken reply admission invariants"
)]
use super::*;

#[test]
fn exact_allowance_is_retained_until_fifo_completion() {
    let mut lane = ReplyCredit::default();
    let first = lane.admit(17).unwrap();
    let last = lane.admit(PENDING_BYTE_ALLOWANCE - 17).unwrap();
    assert_eq!(lane.pending(), PENDING_BYTE_ALLOWANCE);
    assert!(lane.admit(1).is_none());
    assert!(lane.complete(last).is_err());
    assert_eq!(lane.pending(), PENDING_BYTE_ALLOWANCE);
    lane.complete(first).unwrap();
    assert!(lane.complete(first).is_err());
    assert_eq!(lane.available(), 17);
    lane.complete(last).unwrap();
    assert_eq!(lane.pending(), 0);
    assert_eq!(lane.available(), PENDING_BYTE_ALLOWANCE);
    assert!(lane.admit(0).is_none());
    assert!(lane.admit(PENDING_BYTE_ALLOWANCE + 1).is_none());
}

#[test]
fn completion_prefixes_do_not_alias_after_turns_or_counter_exhaustion() {
    let mut lane = ReplyCredit::default();
    let stale = lane.admit(10).unwrap();
    lane.complete(stale).unwrap();
    let current = lane.admit(10).unwrap();
    assert_ne!(stale.id(), current.id());
    assert!(lane.complete(stale).is_err());
    lane.complete(current).unwrap();
    lane.issued = u64::MAX - 1;
    lane.completed = lane.issued;
    assert!(lane.admit(2).is_none());
    assert_eq!(lane.pending(), 0);
    let final_byte = lane.admit(1).unwrap();
    lane.complete(final_byte).unwrap();
    assert!(lane.admit(1).is_none());
}
