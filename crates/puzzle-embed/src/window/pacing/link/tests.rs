//! Injected physical clocks preserve settling, refresh identity and current-request slack.
use super::*;

/// Supplies native clock facts without OS timers or a surface-acquisition guess.
fn tick(pacer: &Pacer, index: u64, callback_ms: u64) -> Refresh {
    Refresh {
        display: 1,
        index,
        output: pacer.origin + Duration::from_millis(callback_ms + 16),
        callback: pacer.origin + Duration::from_millis(callback_ms),
        period: Duration::from_nanos(16_666_667),
    }
}

/// A 30-µs owner answer fits this same physical refresh, including warm build cost.
#[test]
fn same_refresh_reply_has_its_own_build_deadline() {
    let mut pacer = Pacer::new_linked(60_000, Duration::ZERO);
    let origin = pacer.origin;
    pacer.request(origin, false);
    assert!(pacer.take(origin));
    pacer.snapshot_received(origin, [Duration::ZERO; 2]);
    pacer.presented(
        origin,
        origin,
        Duration::ZERO,
        origin + Duration::from_millis(2),
    );
    pacer.request(origin + Duration::from_millis(5), false);
    assert_eq!(pacer.due(), None);
    let refresh = tick(&pacer, 100, 16);
    pacer.refreshed(refresh, refresh.callback);
    let deadline = pacer.build_deadline(refresh.callback);
    assert!(refresh.callback + Duration::from_micros(30) < deadline);
    assert_eq!(deadline, refresh.output - Duration::from_millis(2));
    assert!(pacer.take(refresh.callback));
    // A duplicate output identity after another damage event cannot redraw twice.
    pacer.request(origin + Duration::from_millis(18), false);
    pacer.refreshed(refresh, origin + Duration::from_millis(18));
    assert_eq!(pacer.due(), None);
    let next = tick(&pacer, 101, 33);
    pacer.refreshed(next, next.callback);
    assert!(pacer.take(next.callback));
}

/// A future scanout time never licenses an early snapshot inside the settle window.
#[test]
fn link_tick_settles_at_snapshot_time_and_idle_has_no_deadline() {
    let mut pacer = Pacer::new_linked(60_000, Duration::from_millis(8));
    let origin = pacer.origin;
    pacer.request(origin, false);
    assert!(pacer.take(origin));
    pacer.presented(origin, origin, Duration::ZERO, origin);
    let initial = tick(&pacer, 1, 16);
    pacer.refreshed(initial, initial.callback);
    assert!(!pacer.wants_refresh());
    assert_eq!(pacer.due(), None);
    pacer.request(origin + Duration::from_millis(100), false);
    let early = tick(&pacer, 2, 104);
    pacer.refreshed(early, early.callback);
    assert_eq!(pacer.due(), None);
    assert!(pacer.wants_refresh());
    let legal = tick(&pacer, 3, 120);
    pacer.refreshed(legal, legal.callback);
    assert!(pacer.take(legal.callback));
    pacer.presented(
        legal.callback,
        legal.callback,
        Duration::ZERO,
        legal.callback,
    );
    assert!(!pacer.wants_refresh());
}

/// FIFO wait cannot overwrite a native clock or reintroduce 19.3-ms timer drift.
#[test]
fn link_clock_keeps_every_supplied_refresh_despite_fifo_wait() {
    let mut pacer = Pacer::new_linked(60_000, Duration::ZERO);
    let origin = pacer.origin;
    pacer.request(origin, false);
    assert!(pacer.take(origin));
    let period = Duration::from_nanos(16_666_667);
    for index in 1..=120 {
        let callback = origin + period * index;
        let refresh = Refresh {
            display: 1,
            index: u64::from(index),
            output: callback + period,
            period,
            callback,
        };
        pacer.request_animation(refresh.callback);
        pacer.refreshed(refresh, refresh.callback);
        assert_eq!(pacer.refresh, Some((u64::from(index), refresh.output)));
        assert!(pacer.take(refresh.callback));
        pacer.presented(
            refresh.callback,
            refresh.callback + Duration::from_millis(5),
            Duration::from_millis(5),
            refresh.callback + Duration::from_millis(6),
        );
        assert_eq!(pacer.phase, Some(refresh.output));
        assert_eq!(pacer.due(), None);
    }
}

/// Taking over an existing native clock must neither inherit its index namespace
/// nor submit twice for a refresh which was already consumed by that source.
#[test]
fn host_takeover_preserves_output_barrier_and_disables_timer_calibration() {
    let mut pacer = Pacer::new_linked(60_000, Duration::ZERO);
    let origin = pacer.origin;
    pacer.request(origin, false);
    assert!(pacer.take(origin));
    pacer.request_animation(origin);
    let native = tick(&pacer, 900_000, 16);
    pacer.refreshed(native, native.callback);
    assert!(pacer.take(native.callback));
    pacer.presented(
        native.callback,
        native.callback,
        Duration::ZERO,
        native.callback,
    );
    pacer.use_host_refresh();
    pacer.request_animation(native.callback);
    let duplicate = Refresh { index: 1, ..native };
    pacer.refreshed(duplicate, native.callback);
    assert!(!pacer.take(native.callback));
    let host = tick(&pacer, 2, 33);
    pacer.refreshed(host, host.callback);
    assert!(pacer.take(host.callback));
    // Sticky selection does not reset the host's own duplicate barrier each turn.
    pacer.use_host_refresh();
    pacer.request_animation(host.callback);
    pacer.refreshed(host, host.callback);
    assert!(!pacer.take(host.callback));
    assert_eq!(pacer.calibration_due(host.callback, true), None);
}
