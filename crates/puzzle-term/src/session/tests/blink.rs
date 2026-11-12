//! A host clock schedules cursor frames without spinning while idle.
//!
//! The text context remains with this harness, outside Session. A blink
//! changes only presentation phase and eventually stops after 15 seconds;
//! the already prepared glyph and atlas never need replacement for it.

use super::*;
use puzzle_text::{FontData, font_set::FontSet};

/// A scheduled blink stops after fifteen seconds and leaves ink visible.
#[test]
fn blinking_stops_without_input() {
    /// Counts timer registrations without opening a display window.
    struct TimedWaker(Arc<AtomicUsize>);
    impl Waker for TimedWaker {
        /// A timer test needs no actual window redraw.
        fn request_frame(&mut self) {}
        /// Each deadline should be registered at most once per phase.
        fn wake_at(&mut self, _instant: Instant) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let timers = Arc::new(AtomicUsize::new(0));
    let services = Services::new(
        Box::new(RecordingTransport(Arc::new(Mutex::new(Vec::new())))),
        Box::new(TimedWaker(timers.clone())),
    );
    let mut session = TestHost::new(SessionConfig::default(), services);
    // The approved fixture is read by a test only. Session receives just
    // cell metrics; font bytes remain owned by this separate context.
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    ))
    .expect("bundled font fixture");
    let fonts = FontSet::new(FontData::new(bytes, 0), None, None).expect("valid font");
    let mut context =
        crate::frame_builder::context::TextContext::new(fonts, 14.0, 1.0).expect("frame metrics");
    session.set_text_context(&context);
    let start = Instant::now();
    assert!(session.draw(start, &mut context).is_some());
    assert_eq!(timers.load(Ordering::Relaxed), 1);
    // One interval hides the cursor; the idle cutoff restores it and
    // cancels further work so the host can sleep when no bytes arrive.
    assert!(
        session
            .draw(start + Duration::from_millis(530), &mut context)
            .is_some()
    );
    assert!(!session.builder.as_ref().expect("fonts").blink_phase());
    assert!(
        session
            .draw(start + Duration::from_secs(15), &mut context)
            .is_some()
    );
    assert!(session.builder.as_ref().expect("fonts").blink_phase());
    assert!(
        session
            .draw(start + Duration::from_secs(16), &mut context)
            .is_none()
    );
}
