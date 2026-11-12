//! Manual-clock checks for the shared burst rule on every refresh source.
use super::*;

/// Erases inside eight milliseconds cannot be exposed by a refresh tick.
#[test]
fn short_mark_and_erase_share_one_snapshot() {
    for gap in [2, 5, 7] {
        let mut settle = SettleWindow::new(Duration::from_millis(8));
        settle.presented(Duration::ZERO);
        settle.request(Duration::from_millis(20), Duration::from_millis(16));
        for tick in 20..28 {
            assert!(!settle.may_snapshot(Duration::from_millis(tick)));
        }
        settle.request(Duration::from_millis(20 + gap), Duration::from_millis(16));
        assert!(settle.may_snapshot(Duration::from_millis(28)));
    }
}

/// Continuous output does not pay another idle-burst delay after every frame.
#[test]
fn continuous_frames_remain_refresh_driven() {
    let mut settle = SettleWindow::new(Duration::from_millis(8));
    let period = Duration::from_millis(16);
    // A five-ms stream has no period of output silence, even after each
    // presented frame resets the pending burst.
    for millisecond in [0, 5, 10, 15] {
        settle.request(Duration::from_millis(millisecond), period);
    }
    assert!(settle.may_snapshot(Duration::from_millis(16)));
    settle.presented(Duration::from_millis(16));
    assert!(!settle.may_snapshot(Duration::from_millis(17)));
    for millisecond in [20, 25, 30] {
        settle.request(Duration::from_millis(millisecond), period);
    }
    // Backdating makes this stream eligible at its next refresh tick,
    // rather than adding another eight-ms wait after the 20-ms output.
    assert_eq!(settle.earliest(), Some(Duration::from_millis(20)));
    assert!(settle.may_snapshot(Duration::from_millis(32)));
}

/// Every 33.4-ms repeat starts a fresh burst across all 60-Hz phases.
#[test]
fn auto_repeat_never_presents_its_short_lived_mark() {
    let refresh_us = 16_667;
    let refresh = Duration::from_micros(refresh_us);
    for gap_ms in [2, 5] {
        for phase_us in (0..refresh_us).step_by(500) {
            let mut settle = SettleWindow::new(Duration::from_millis(8));
            let mut next_tick = phase_us;
            for repeat in 0..60 {
                let mark = 20_000 + repeat * 33_400;
                let erase = mark + gap_ms * 1_000;
                // Finish any prior frame before the next simulated keydown;
                // the next request must classify silence by last output.
                while next_tick < mark {
                    if settle.may_snapshot(Duration::from_micros(next_tick)) {
                        settle.presented(Duration::from_micros(next_tick));
                    }
                    next_tick += refresh_us;
                }
                settle.request(Duration::from_micros(mark), refresh);
                while next_tick < erase {
                    assert!(
                        !settle.may_snapshot(Duration::from_micros(next_tick)),
                        "mark visible at phase={phase_us}, repeat={repeat}, gap={gap_ms}"
                    );
                    next_tick += refresh_us;
                }
                settle.request(Duration::from_micros(erase), refresh);
            }
        }
    }
}

/// The same refresh tick admits motion while retaining the output deadline.
#[test]
fn animation_bypasses_output_settle() {
    let mut rule = SettleWindow::new(Duration::from_millis(8));
    rule.request(Duration::from_millis(100), Duration::from_micros(8333));
    for tick in [100, 102, 107] {
        assert!(!rule.may_present(Duration::from_millis(tick), false));
        assert!(rule.may_present(Duration::from_millis(tick), true));
        assert_eq!(rule.earliest(), Some(Duration::from_millis(108)));
    }
    assert!(rule.may_present(Duration::from_millis(108), false));
}
