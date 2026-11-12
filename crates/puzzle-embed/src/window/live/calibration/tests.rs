//! Idle calibration must fill slack, stop on a real sample and remain bounded.
use super::policy::Calibration;
use std::time::Duration;

/// Three free drawables require a fourth acquire; an occupied queue stops earlier.
#[test]
fn stops_at_first_qualifying_acquire_for_any_queue_occupancy() {
    for (latency, waits) in [(1, vec![0, 0, 2]), (2, vec![0, 0, 1, 2]), (2, vec![2])] {
        let mut calibration = Calibration::new(latency);
        for wait in &waits {
            assert!(calibration.can_present());
            calibration.acquired(Duration::from_millis(*wait));
        }
        assert!(calibration.qualified());
        assert!(!calibration.can_present());
        assert_eq!(calibration.presents(), waits.len() as u32);
    }
}

/// A compositor that never blocks cannot make an idle attempt loop indefinitely.
#[test]
fn unqualified_attempt_stops_at_drawable_count_plus_one() {
    for latency in [1, 2] {
        let mut calibration = Calibration::new(latency);
        while calibration.can_present() {
            calibration.acquired(Duration::from_micros(999));
        }
        assert!(!calibration.qualified());
        assert_eq!(calibration.presents(), latency + 2);
    }
}
