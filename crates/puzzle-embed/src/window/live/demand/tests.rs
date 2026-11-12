//! Injected ticks reproduce a source running while no snapshot reaches the builder.
use super::*;
use crate::{render_thread::surface::Refresh, window::pacing::Pacer};
use std::time::{Duration, Instant};

/// Taking a frame opportunity cannot stop the link while its owner is still answering.
#[test]
fn ticks_without_built_frames_retain_demand_until_submission() {
    let now = Instant::now();
    let mut demand = FrameDemand::default();
    let mut pacer = Pacer::new_linked(60_000, Duration::ZERO);
    pacer.request(now, false);
    assert!(pacer.take(now));
    // The old rule became idle here although output and its request were pending.
    demand.requested(0);
    assert!(demand.pending(100, 0));
    let period = Duration::from_nanos(16_666_667);
    for index in 1..=120 {
        let callback = now + period * index;
        pacer.output_pending(demand.pending(100, 0), callback);
        let tick = Refresh {
            display: 3,
            index: u64::from(index),
            callback,
            output: callback + period,
            period,
        };
        pacer.refreshed(tick, callback);
        assert!(pacer.take(callback));
        // A late older answer does not acknowledge the current request.
        demand.requested(u64::from(index));
        demand.answered(u64::from(index - 1), false, 0);
        assert!(demand.pending(100, 0));
    }
    demand.answered(120, true, 100);
    assert!(demand.pending(100, 0));
    // Even an answered dirty publication remains outstanding until actually drawn.
    demand.presented(100);
    assert!(!demand.pending(100, 0));
    assert!(!pacer.wants_refresh());
    assert!(demand.pending(100, 1));
    assert!(demand.pending(101, 0));
}

/// Nonvisual output stops idle wakes without acknowledging an undrawn dirty publication.
#[test]
fn unchanged_answers_retire_bytes_but_preserve_unsubmitted_ink() {
    let mut demand = FrameDemand::default();
    demand.requested(1);
    demand.answered(1, false, 20);
    assert!(!demand.pending(20, 0));
    demand.requested(2);
    demand.answered(2, true, 30);
    demand.requested(3);
    demand.answered(3, false, 40);
    assert!(demand.pending(40, 0));
    demand.presented(30);
    assert!(!demand.pending(40, 0));
}
