//! Restart attribution retains native clocks and excludes warmup ticks (K26).
#![allow(
    clippy::expect_used,
    reason = "the diagnostic restart array is the fixture's required output"
)]
use super::*;
use std::time::Duration;

#[test]
fn first_selected_tick_keeps_the_restart_receipt_without_hiding_later_delay() {
    let origin = Instant::now();
    let mut journal = Journal::default();
    let before = origin - Duration::from_millis(10);
    journal.restarted(before, before + Duration::from_millis(3));
    // A callback before recording cannot become the measured leading edge.
    journal.tick(before, Some(origin));
    journal.restarted(origin, origin + Duration::from_millis(17));
    journal.tick(origin + Duration::from_millis(17), Some(origin));
    journal.tick(origin + Duration::from_millis(34), Some(origin));
    let value = journal.json(origin);
    assert_eq!(value["first_tick_ms"], 17.0);
    assert_eq!(
        value["restarts"].as_array().expect("restart array").len(),
        2
    );
    assert_eq!(value["restarts"][0]["start_ms"], -10.0);
    assert_eq!(value["restarts"][1]["duration_ms"], 17.0);
    // Presentation is deliberately absent: the consumer must still measure
    // the work after this callback, including any accidentally skipped tick.
    assert!(value.get("present_ms").is_none());
}

#[test]
fn ordinary_ticks_do_not_manufacture_a_restart() {
    let origin = Instant::now();
    let mut journal = Journal::default();
    journal.tick(origin, None);
    assert!(journal.json(origin)["first_tick_ms"].is_null());
    journal.tick(origin + Duration::from_millis(5), Some(origin));
    let value = journal.json(origin);
    assert_eq!(value["first_tick_ms"], 5.0);
    assert!(
        value["restarts"]
            .as_array()
            .expect("restart array")
            .is_empty()
    );
}
