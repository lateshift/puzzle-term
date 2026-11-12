//! §7.3 timelines exercise public viewport behavior without sleeps.
use super::*;
use std::time::Duration;
/// Twenty history rows leave enough travel for retargeting and snapping.
fn fixture() -> (Terminal, Viewport, Instant) {
    let mut term = Terminal::new(12, 3, 100, 1_000_000);
    for _ in 0..23 {
        term.advance(b"row\r\n");
    }
    let mut view = Viewport::default();
    view.set_row_height(20);
    view.configure(1.0, true, true);
    view.reconcile(&mut term, true);
    (term, view, Instant::now())
}
/// A second notch retains continuity and accumulates the pending destination.
#[test]
fn notch_retarget_and_anchor() {
    let (mut term, mut view, start) = fixture();
    view.wheel(&mut term, ScrollInput::Rows(-3.0), start);
    view.advance(&mut term, start + Duration::from_millis(60));
    assert_eq!(view.scroll_px(), 52.5);
    view.wheel(
        &mut term,
        ScrollInput::Rows(-1.0),
        start + Duration::from_millis(60),
    );
    assert_eq!(view.scroll_px(), 52.5); // Retarget cannot jump to the old destination.
    view.advance(&mut term, start + Duration::from_millis(120));
    assert_eq!(view.scroll_px(), 76.5625);
    let top = view.top_index();
    term.advance(b"new\r\n");
    view.reconcile(&mut term, true);
    assert_eq!(view.top_index(), top);
    assert_eq!(view.scroll_px(), 96.5625);
    view.advance(&mut term, start + Duration::from_millis(180));
    assert_eq!(view.scroll_px(), 100.0);
    view.advance(&mut term, start + Duration::from_millis(210));
    view.presented();
    assert!(!view.animation_frame(start + Duration::from_secs(1)));
    assert!(view.snap_deadline().is_none());
}
/// Pixels have no invented inertia; silence alone begins a finite row snap.
#[test]
fn pixel_snap_and_idle() {
    let (mut term, mut view, start) = fixture();
    view.wheel(&mut term, ScrollInput::Pixels(-27.0), start);
    assert_eq!(view.scroll_px(), 27.0);
    assert_eq!(view.device_shift(), -13);
    view.presented();
    assert!(!view.animating(start + Duration::from_millis(149)));
    view.advance(&mut term, start + Duration::from_millis(150));
    assert_eq!(view.scroll_px(), 27.0);
    assert!(view.animating(start + Duration::from_millis(150)));
    view.advance(&mut term, start + Duration::from_millis(200));
    assert_eq!(view.scroll_px(), 20.875);
    view.advance(&mut term, start + Duration::from_millis(250));
    assert_eq!(view.scroll_px(), 20.0);
    assert_eq!(view.device_shift(), 0);
    assert!(!view.advance(&mut term, start + Duration::from_secs(1)));
    assert!(!view.animating(start + Duration::from_secs(1)));
    assert!(view.snap_deadline().is_none());
}
/// Accessibility and explicit policies cannot leave hidden animation clocks.
#[test]
fn reduced_motion_and_no_snap() {
    for reduced in [false, true] {
        let (mut term, mut view, start) = fixture();
        view.configure(1.0, reduced, true);
        view.reduced_motion(reduced);
        view.wheel(&mut term, ScrollInput::Rows(-2.0), start);
        assert_eq!(view.scroll_px(), 40.0);
        view.presented();
        assert!(!view.animating(start));
        assert!(view.snap_deadline().is_none());
    }
    let (mut term, mut view, start) = fixture();
    view.configure(2.0, true, false);
    view.wheel(&mut term, ScrollInput::Pixels(-13.5), start);
    assert_eq!(view.scroll_px(), 27.0);
    view.presented();
    assert!(!view.advance(&mut term, start + Duration::from_secs(1)));
    assert!(view.snap_deadline().is_none());
}

/// A gesture clamped at the live boundary creates no silence or frame work.
#[test]
fn boundary_input_is_idle() {
    let (mut term, mut view, start) = fixture();
    for input in [
        ScrollInput::Pixels(3.0),
        ScrollInput::Rows(1.0),
        ScrollInput::Pixels(f64::NAN),
    ] {
        view.wheel(&mut term, input, start);
        assert!(view.following());
        assert!(view.snap_deadline().is_none());
        assert!(!view.animation_frame(start));
    }
}
