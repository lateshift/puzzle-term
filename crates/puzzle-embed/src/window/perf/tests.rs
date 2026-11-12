//! Callback residence uses injected durations; idle and mouse pauses are not work.
use super::*;

#[test]
fn idle_and_mid_drag_event_gaps_are_not_main_busy_time() {
    let began = Instant::now();
    let mut log = PerfLog::new(began);
    log.event(began, true);
    log.busy(Duration::from_millis(2));
    log.event(began + Duration::from_millis(900), true);
    log.busy(Duration::from_millis(3));
    log.event(began + Duration::from_secs(3), false);
    assert_eq!(log.max_main_busy, Duration::from_millis(3));
}

#[test]
fn callback_residence_includes_the_native_display_wait() {
    let mut log = PerfLog::new(Instant::now());
    log.busy(Duration::from_millis(1));
    log.busy(Duration::from_millis(17));
    log.busy(Duration::from_millis(2));
    assert_eq!(log.max_main_busy, Duration::from_millis(17));
}

/// Never pair the longest wall callback with CPU from a shorter callback.
#[test]
fn max_callback_retains_its_own_cpu_and_unknown_is_not_zero() {
    let mut log = PerfLog::new(Instant::now());
    log.busy_observed(Duration::from_millis(12), Some(Duration::from_millis(2)));
    log.busy_observed(Duration::from_millis(3), Some(Duration::from_millis(3)));
    assert_eq!(log.max_main_busy_cpu, Some(Duration::from_millis(2)));
    log.busy(Duration::from_millis(17));
    assert_eq!(log.max_main_busy_cpu, None);
}
