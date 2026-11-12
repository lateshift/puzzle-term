//! Basic touch contract without DOM timing or hardware assumptions (§12.2).
#![allow(
    clippy::expect_used,
    reason = "authored touch timelines must yield momentum samples"
)]
use super::Touch;

/// rAF reports the refresh start, which can precede a release in that turn.
#[test]
fn refresh_before_release_defers_without_canceling_momentum() {
    let mut touch = Touch::default();
    assert!(touch.press(1, 0.0, 100.0));
    assert_eq!(touch.moved(1, 20.0, 110.0), Some(-20.0));
    touch.release(1, 111.0, false);
    assert_eq!(touch.advance(108.0), None);
    assert!(touch.animating(), "early refresh must preserve the tail");
    assert!(touch.advance(116.0).expect("later refresh") < 0.0);
}
#[test]
fn drag_momentum_cancellation_and_idle() {
    let mut touch = Touch::default();
    assert!(touch.press(1, 100.0, 0.0));
    assert!(!touch.press(2, 200.0, 1.0));
    assert_eq!(touch.moved(2, 250.0, 10.0), None);
    assert_eq!(touch.moved(1, 120.0, 10.0), Some(-20.0));
    touch.release(1, 12.0, false);
    assert!(touch.animating());
    let first = touch.advance(20.0).expect("authored retained fixture");
    let second = touch.advance(28.0).expect("authored retained fixture");
    assert!(first < second && second < 0.0);
    for frame in 0..300 {
        let _ = touch.advance(36.0 + frame as f64 * 8.0);
    }
    assert!(!touch.animating());
    assert_eq!(touch.advance(3000.0), None);
    assert!(touch.press(1, 100.0, 4000.0));
    let _ = touch.moved(1, 130.0, 4010.0);
    touch.release(1, 4012.0, true);
    assert!(!touch.animating());
    touch.cancel();
    // A tap and a stale release launch neither motion nor an idle loop.
    assert!(touch.press(1, 100.0, 5000.0));
    touch.release(1, 5010.0, false);
    assert!(!touch.animating());
}
