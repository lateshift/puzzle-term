//! Consumed controls cannot leak key stamps into later parser replies.
use super::*;

#[test]
fn consumed_keys_and_nonkey_controls_do_not_label_later_replies() {
    let trace = Controls::default();
    let now = Instant::now();
    trace.begin(now, now, true);
    trace.finish();
    trace.queued();
    trace.begin(now, now, false);
    trace.queued();
    trace.finish();
    assert_eq!(trace.report()["queued_keys"], 0);
    trace.begin(now, now, true);
    trace.queued();
    // One key action may write several transactions; only its first queue endpoint counts.
    trace.queued();
    trace.finish();
    let report = trace.report();
    assert_eq!(report["control_count"], 3);
    assert_eq!(report["queued_keys"], 1);
    assert_eq!(
        report["worst_five_key_wall_and_applied_to_queue_cpu"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn timed_input_preserves_each_clock_boundary_and_consumed_keys_clear_it()
-> Result<(), crate::ApiError> {
    let trace = Controls::default();
    let api = Instant::now();
    let mut timing = crate::InputTiming::new(700.0, 700.125)?;
    timing.api = api;
    timing.sent = api + Duration::from_micros(20);
    timing.owner = Some((api + Duration::from_micros(50), None));
    trace.begin_timed(timing, true);
    assert_eq!(trace.host_residence.load(Ordering::Relaxed), 125_000_000);
    assert_eq!(trace.api_to_channel.load(Ordering::Relaxed), 20_000);
    assert_eq!(trace.wait.load(Ordering::Relaxed), 30_000);
    assert_eq!(trace.key_wait.load(Ordering::Relaxed), 50_000);
    // Search/shortcut consumption cannot lend this clock pair to a later reply.
    trace.finish();
    trace.queued();
    assert_eq!(trace.report()["queued_keys"], 0);
    timing.residence = None;
    trace.begin_timed(timing, true);
    assert_eq!(trace.host_residence.load(Ordering::Relaxed), u64::MAX);
    trace.finish();
    Ok(())
}
