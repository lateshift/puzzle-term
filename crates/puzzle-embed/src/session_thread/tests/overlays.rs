//! Idle acknowledgements return storage, never a new overlay registration set.
use super::*;
use crate::session_thread::endpoint::FrameRequest;
use puzzle_term::{
    PublishOutcome,
    frame_builder::{FrameBuilder, context::TextContext},
    frame_snapshot::FrameSnapshot,
};

/// Three slots begin empty; an idle answer may return one that was never captured.
#[test]
fn nothing_new_free_slots_cannot_remove_presented_command_controls() {
    let fonts = crate::font_data::bundled_font_set().unwrap_or_else(|e| panic!("{e}"));
    let mut context = TextContext::new(fonts, 14.0, 2.0).unwrap_or_else(|e| panic!("{e}"));
    let (writes, _) = mpsc::channel();
    let mut session = Session::new(
        SessionConfig::default(),
        Services::new(Box::new(Writer(writes)), Box::new(Quiet)),
    );
    session.set_text_context(&context);
    session.install_model_overlays();
    let now = Instant::now();
    session.feed(b"\x1b]133;C\x07one\r\ntwo\r\n\x1b]133;D;0\x07", now);
    let mut builder = FrameBuilder::new(&context);
    let mut slot = FrameSnapshot::default();
    assert_eq!(
        session
            .publish_frame(
                now,
                &mut slot,
                context.atlas_generation(),
                builder.style_epoch()
            )
            .unwrap_or_else(|e| panic!("{e}")),
        PublishOutcome::Published
    );
    let mut publication = reply(slot, true, now);
    publication.accept_views(&mut builder);
    let mut frame = puzzle_term::frame::RenderFrame::default();
    builder
        .build_published(&mut context, &mut publication.slot, &mut frame)
        .unwrap_or_else(|e| panic!("{e}"));
    let geometry = builder.text_geometry();
    let point = geometry.grid.origin.map(|v| v + 1.0);
    let mut regions = puzzle_term::overlay::regions::PresentedRegions::default();
    builder.capture_presented_regions(&mut regions);
    assert!(regions.hit(point).is_some(), "presented command observer");
    // Exactly the native begin_snapshot path: adopt an idle reply before drawing
    // the retained publication again. Its fresh slot has no overlay membership.
    for _ in 0..3 {
        let mut idle = reply(FrameSnapshot::default(), false, now);
        idle.accept_views(&mut builder);
        builder
            .build_published(&mut context, &mut publication.slot, &mut frame)
            .unwrap_or_else(|e| panic!("{e}"));
        builder.capture_presented_regions(&mut regions);
        assert!(
            regions.hit(point).is_some(),
            "idle slot destroyed one-time view ownership"
        );
    }
}

/// Queue metadata is independent of the source inside a reusable free slot.
fn reply(slot: FrameSnapshot, published: bool, now: Instant) -> FrameReply {
    let request = FrameRequest {
        id: 1,
        sent: now,
        now,
        deadline: now,
        applied: Some((now, None)),
        atlas_generation: 0,
        style_epoch: 0,
    };
    FrameReply {
        id: 1,
        timing: PublicationTiming::completed(&request),
        grid: (80, 24),
        slot,
        published,
        parsed: 0,
    }
}
