//! Injected-clock regression for D-50: an older queued reply cannot add a refresh.
use super::*;
use crate::session_thread::endpoint::{FrameRequest, select_replies};
use std::{cell::Cell, collections::VecDeque};

/// Ready old ink must not suppress the current owner's ordinary 30-us answer.
#[test]
fn current_request_answer_is_used_in_its_own_refresh() {
    let zero = Instant::now();
    let current = zero + Duration::from_micros(30);
    let deadline = zero + Duration::from_millis(1);
    let clock = Cell::new(zero);
    let mut replies = VecDeque::from([(zero, reply(0, zero)), (current, reply(1, zero))]);
    let mut observed = Vec::new();
    let (newest, answered) = select_replies(
        1,
        |wait| {
            let (at, _) = replies.front()?;
            let allowed = if wait { deadline } else { clock.get() };
            if *at > allowed {
                clock.set(allowed);
                return None;
            }
            clock.set(clock.get().max(*at));
            replies.pop_front().map(|(_, answer)| answer)
        },
        |_| {},
        |answer| observed.push(answer.id),
    );
    assert!(answered);
    assert_eq!(newest.unwrap_or_else(|| panic!("sealed source")).id, 1);
    assert_eq!(clock.get(), current);
    assert_eq!(observed, [0, 1]);
}

/// A genuinely late current answer stays queued for the next refresh (D-50 exception).
#[test]
fn current_answer_after_deadline_preserves_the_previous_complete_source() {
    let zero = Instant::now();
    let deadline = zero + Duration::from_millis(1);
    let mut replies = VecDeque::from([
        (zero, reply(0, zero)),
        (deadline + Duration::from_nanos(1), reply(1, zero)),
    ]);
    let clock = Cell::new(zero);
    let (newest, _) = select_replies(
        1,
        |wait| {
            let (at, _) = replies.front()?;
            let allowed = if wait { deadline } else { clock.get() };
            if *at > allowed {
                clock.set(allowed);
                return None;
            }
            clock.set(clock.get().max(*at));
            replies.pop_front().map(|(_, answer)| answer)
        },
        |_| {},
        |_| {},
    );
    assert_eq!(
        newest
            .unwrap_or_else(|| panic!("previous complete source"))
            .id,
        0
    );
    assert_eq!(replies.len(), 1);
}

/// Clock-independent source data exercises the production queue-selection rule.
fn reply(id: u64, now: Instant) -> FrameReply {
    let request = FrameRequest {
        id,
        sent: now,
        deadline: now + Duration::from_secs(1),
        applied: Some((now, None)),
        now,
        atlas_generation: 0,
        style_epoch: 0,
    };
    FrameReply {
        id,
        timing: PublicationTiming::completed(&request),
        grid: (0, 0),
        slot: puzzle_term::frame_snapshot::FrameSnapshot::default(),
        published: true,
        parsed: id,
    }
}
