//! Throttle windows fire throughout the storm; its final notification is not duplicated.
use super::*;

#[test]
fn throttle_notifications_are_consumed_once_before_or_after_close() {
    let origin = Instant::now();
    // Exercise the real 180-event stream with both possible frame-completion orders.
    for frame_cost in [Duration::ZERO, Duration::from_millis(30)] {
        let mut quiet = QuietResize::default();
        let mut calls = Vec::new();
        let mut final_size = [0, 0];
        for event in 0..180 {
            let now = origin + Duration::from_secs_f64(event as f64 / 60.0);
            final_size = if event % 2 == 0 {
                [1024, 800]
            } else {
                [1280, 960]
            };
            quiet.update(final_size, now);
            // Poll at the next control opportunity; only the first event armed the timer.
            if event < 179 {
                calls.extend(quiet.take(now + Duration::from_secs_f64(1.0 / 60.0), false));
            } else {
                // A final GPU submission used to expire the timer, after which
                // finish unconditionally sent another identical resize.
                calls.extend(quiet.take(now + frame_cost, false));
                calls.extend(quiet.take(now + frame_cost, true));
                calls.extend(quiet.take(now + frame_cost, true));
            }
        }
        // At 60-Hz control opportunities, each 25-ms window spans two events.
        // Close consumes the last pending value once, with either frame ordering.
        assert_eq!(calls.len(), 180 / 2);
        assert_eq!(calls.last(), Some(&final_size));
    }
}

#[test]
fn completed_group_does_not_hide_a_later_resize() {
    let now = Instant::now();
    let mut quiet = QuietResize::default();
    quiet.update([1024, 800], now);
    assert_eq!(
        quiet.take(now + Duration::from_millis(25), false),
        Some([1024, 800])
    );
    assert_eq!(quiet.take(now, true), None);
    quiet.update([1280, 960], now);
    assert_eq!(quiet.take(now, true), Some([1280, 960]));
    assert_eq!(quiet.take(now, true), None);
}
