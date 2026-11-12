//! Native input retains geometry and appearance during detached row building.
//!
//! A long glyph miss used to make cell_metrics return None and silently lose
//! focus/theme/blink edits. Input metadata must survive that handoff without
//! moving a glyph cache onto the input thread or changing the captured frame.
use super::*;
use crate::{frame::RenderFrame, frame_snapshot::FrameSnapshot};

/// Input changes apply to the following frame and preserve the old snapshot.
#[test]
fn input_state_survives_external_build() {
    let (mut session, writes, _) = fixture();
    let mut context = overlays::text_context(1.0);
    let now = Instant::now();
    session.set_text_context(&context);
    session.feed(b"old\x1b[?1004h", now);
    let metrics = session.cell_metrics();
    let mut snapshot = FrameSnapshot::default();
    let mut builder = FrameBuilder::new(&context);
    assert_eq!(
        session
            .publish_frame(
                now,
                &mut snapshot,
                context.atlas_generation(),
                builder.style_epoch()
            )
            .expect("capture"),
        crate::PublishOutcome::Published
    );
    assert_eq!(
        session.cell_metrics(),
        metrics,
        "pointer geometry disappeared during glyph work"
    );
    session.submit(Command::Focus(false), Origin::User);
    session.pump(now);
    session.set_surface_size([900, 700]);
    session.feed(b"new", now);
    assert_eq!(
        writes.lock().expect("writes").last(),
        Some(&b"\x1b[O".to_vec())
    );
    let mut frame = RenderFrame::default();
    builder
        .build_published(&mut context, &mut snapshot, &mut frame)
        .expect("old captured frame");
    assert_ne!(
        frame.target_size,
        [900, 700],
        "input changed a sealed frame"
    );
    session.builder = Some(builder);
    let next = session.draw(now, &mut context).expect("later damage");
    assert_eq!(next.target_size, [900, 700]);
    assert!(
        next.rects.iter().any(|rect| rect.stroke_width == 1.0),
        "focus loss did not draw a hollow cursor"
    );
}

/// A theme edit during construction must invalidate the next copied rows.
#[test]
fn theme_revision_survives_external_build() {
    let (mut session, _, _) = fixture();
    let mut context = overlays::text_context(1.0);
    let now = Instant::now();
    session.set_text_context(&context);
    session.feed(b"theme", now);
    let mut snapshot = FrameSnapshot::default();
    let mut builder = FrameBuilder::new(&context);
    assert_eq!(
        session
            .publish_frame(
                now,
                &mut snapshot,
                context.atlas_generation(),
                builder.style_epoch()
            )
            .expect("capture"),
        crate::PublishOutcome::Published
    );
    let mut settings = crate::settings::Settings::default();
    settings.theme_mode = crate::settings::ThemeMode::Light;
    settings.theme_transition_ms = 0;
    session.apply_settings(&settings, now);
    let mut frame = RenderFrame::default();
    builder
        .build_published(&mut context, &mut snapshot, &mut frame)
        .expect("captured old theme");
    let old_clear = frame.clear;
    assert_eq!(
        session
            .publish_frame(
                now,
                &mut snapshot,
                context.atlas_generation(),
                builder.style_epoch()
            )
            .expect("new theme capture"),
        crate::PublishOutcome::Published
    );
    builder
        .build_published(&mut context, &mut snapshot, &mut frame)
        .expect("recolored copied rows");
    assert_ne!(old_clear, frame.clear);
    session.builder = Some(builder);
}
