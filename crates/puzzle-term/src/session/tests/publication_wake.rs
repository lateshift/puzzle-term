//! Publication consumes the owner's damage wake; output during drawing rearms it.
use super::*;
use crate::{
    HostEvent,
    frame_snapshot::FrameSnapshot,
    platform::{FrameArrival, WakeRequests},
};

/// Deterministic output timestamps make suppressed writes observable without a real clock.
struct LatchWaker {
    /// One owner retains timing even after the host has drained the notification.
    queued: bool,
    /// Each request advances this injected clock, including suppressed requests.
    now: Instant,
    /// Earlier output distinguishes a flood from a fresh burst.
    last: Option<Instant>,
    /// Deferred values leave only after the core turn has ended.
    pending: WakeRequests,
}
impl Waker for LatchWaker {
    /// Keep output history while emitting only the first request of a publication interval.
    fn request_frame(&mut self) {
        self.now += Duration::from_micros(1);
        let previous = self.last.replace(self.now);
        if !self.queued {
            self.queued = true;
            self.pending.frame = true;
            self.pending.arrival = Some(FrameArrival {
                first: self.now,
                previous,
            });
        }
    }
    /// Timer facts do not release or rearm the damage notification.
    fn wake_at(&mut self, instant: Instant) {
        self.pending.deadline = Some(instant);
    }
    /// Snapshot ownership, rather than event delivery, consumes earlier output.
    fn acknowledge_frame(&mut self) {
        self.queued = false;
    }
    /// Draining values leaves the outstanding damage latch intact.
    fn take_requests(&mut self) -> WakeRequests {
        std::mem::take(&mut self.pending)
    }
}

/// Output's final write after a publication must wake without waiting for presentation.
#[test]
fn host_delivery_keeps_latch_but_publication_rearms_final_output() {
    let (mut session, _, _) = fixture();
    let context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    session.enable_host_events().expect("host");
    while session.take_event().is_some() {}
    let now = Instant::now();
    session.services.waker = Box::new(LatchWaker {
        queued: false,
        now,
        last: None,
        pending: WakeRequests::default(),
    });
    session.feed(b"A", now);
    let first = arrival(&mut session).expect("first wake");
    assert_eq!(first.first, now + Duration::from_micros(1));
    assert!(first.previous.is_none());
    for _ in 0..100 {
        session.feed(b"\rA", now);
        assert!(
            arrival(&mut session).is_none(),
            "UI delivery cannot rearm output"
        );
    }
    let mut slot = FrameSnapshot::default();
    session
        .publish_frame(now, &mut slot, context.atlas_generation(), 0)
        .expect("publish");
    assert!(arrival(&mut session).is_none());
    session.feed(b"\rB", now);
    let next = arrival(&mut session).expect("output during old frame build");
    assert_eq!(next.previous, Some(now + Duration::from_micros(101)));
    assert_eq!(next.first, now + Duration::from_micros(102));
    assert_eq!(
        slot.retained_row(0).expect("sealed row").cells[0].ch,
        u32::from('A')
    );
}

/// Host wake values retain both timestamps rather than substituting delivery time.
fn arrival(session: &mut TestHost) -> Option<FrameArrival> {
    let mut arrival = None;
    while let Some(event) = session.take_event() {
        if let HostEvent::Wake(wake) = event {
            arrival = wake.arrival.or(arrival);
        }
    }
    arrival
}
