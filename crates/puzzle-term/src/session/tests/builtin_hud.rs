//! Opt-in frame observation must not introduce hidden work or idle feedback.
use super::*;
use crate::{FrameResult, SessionMessage, messages::FrameStats};
use puzzle_bus::hooks::{EventKinds, HookCtx, Observer};

/// A public subscriber records the same values used by the built-in painter.
struct Frames(Arc<Mutex<Vec<FrameStats>>>);
impl Observer<Command, Event> for Frames {
    fn observe(&mut self, event: &Event, _: &mut HookCtx<'_, Command, Event>) {
        if let Event::FrameRendered(stats) = event {
            self.0.lock().expect("HUD witness").push(*stats);
        }
    }
}
/// No observer means no timing payload; hidden idle turns allocate nothing.
#[test]
fn hidden_hud_has_no_frame_statistics_or_idle_work() {
    let (mut session, _, _) = fixture();
    let mut context = super::overlays::text_context(1.0);
    let now = Instant::now();
    session.set_text_context(&context);
    session.install_model_overlays();
    session.feed(b"\x1b[?25lhidden", now);
    session.draw(now, &mut context).expect("hidden frame");
    assert!(!session.snapshot.observe_frame);
    assert!(
        session
            .builder
            .as_mut()
            .expect("builder")
            .take_frame_statistics()
            .is_none()
    );
    // Warm all scheduling/output queues before counting unchanged owner turns.
    session.draw(now, &mut context);
    let measured = allocation_counter::measure(|| {
        for _ in 0..100 {
            assert!(session.draw(now, &mut context).is_none());
        }
    });
    assert_eq!(measured.count_total, 0);
}
/// Submitted counts/rate reach the public observer and HUD without waking an idle frame.
#[test]
fn shown_hud_observes_real_frame_and_never_self_schedules() {
    for dpr in [1.0, 2.0] {
        shown_at_density(dpr);
    }
}
/// Both densities exercise the same presented statistics and observer teardown.
fn shown_at_density(dpr: f32) {
    let (mut session, _, wakes) = fixture();
    let mut context = super::overlays::text_context(dpr);
    let now = Instant::now();
    session.set_text_context(&context);
    session.install_model_overlays();
    session.feed(b"\x1b[?25l", now);
    session.set_performance_hud(true).expect("enable HUD");
    let frames = Arc::new(Mutex::new(Vec::new()));
    let id = session
        .bus_mut()
        .observe(EventKinds::at(16), 0, "HUD-test", Frames(frames.clone()))
        .expect("observe");
    session.feed(b"actual frame", now);
    let frame = session.draw(now, &mut context).expect("observed frame");
    let counts = [frame.rects.len(), frame.slugs.len(), frame.rasters.len()];
    let stats = session
        .builder
        .as_mut()
        .expect("builder")
        .take_frame_statistics()
        .expect("measured");
    assert_eq!(stats.instances, counts);
    assert!(stats.glyph_bytes > 0 && stats.upload_bytes > 0);
    let before = wakes.load(Ordering::Relaxed);
    session
        .apply(
            SessionMessage::FrameResult(FrameResult::Statistics(stats)),
            now + Duration::from_secs(1),
        )
        .expect("present");
    assert_eq!(wakes.load(Ordering::Relaxed), before);
    assert!(
        session
            .draw(now + Duration::from_secs(1), &mut context)
            .is_none()
    );
    let seen = frames.lock().expect("frames");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].parse_bytes_per_second, 12.0);
    assert_eq!(
        session.model_overlays.hud.value.cpu_ms,
        stats.total_cpu_ms()
    );
    drop(seen);
    session.bus_mut().remove(id);
    hide_observation(&mut session, &mut context, now);
}
/// Hiding releases observation and restores the clock-free builder branch.
fn hide_observation(
    session: &mut TestHost,
    context: &mut crate::frame_builder::context::TextContext,
    now: Instant,
) {
    session.set_performance_hud(false).expect("hide HUD");
    session
        .draw(now + Duration::from_secs(1), context)
        .expect("hidden frame");
    assert!(!session.snapshot.observe_frame);
    assert!(
        session
            .builder
            .as_mut()
            .expect("builder")
            .take_frame_statistics()
            .is_none()
    );
}
