//! Caller-owned publications preserve sealed values, failures and skipped view transfers.
use super::*;
use crate::{FrameResult, PublishOutcome, SessionMessage, frame_snapshot::FrameSnapshot};

/// A host-owned builder renders the old sealed frame while Session advances separately.
#[test]
fn published_rows_and_surface_policy_are_sealed_before_later_input() {
    let (mut session, _, _) = fixture();
    let mut context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    session.set_surface_size([320, 100]);
    let now = Instant::now();
    session.feed(b"before", now);
    let mut slot = FrameSnapshot::default();
    assert_eq!(
        publish(&mut session, &mut slot, &context, now),
        PublishOutcome::Published
    );
    session.feed(b"after", now);
    session.set_surface_size([640, 200]);
    let mut builder = FrameBuilder::new(&context);
    let mut frame = RenderFrame::default();
    builder
        .build_published(&mut context, &mut slot, &mut frame)
        .expect("sealed build");
    assert_eq!(frame.target_size, [320, 100]);
    assert_eq!(slot.retained_row(0).expect("row").cells[6].ch, 0);
    assert_eq!(builder.text_geometry().placements.len(), 6);
    assert_eq!(
        publish(&mut session, &mut slot, &context, now),
        PublishOutcome::Published
    );
    builder
        .build_published(&mut context, &mut slot, &mut frame)
        .expect("new build");
    assert_eq!(frame.target_size, [640, 200]);
    assert_eq!(builder.text_geometry().placements.len(), 11);
}

/// Failed rendering revises every rotating slot, not just the slot that failed.
#[test]
fn failed_build_forces_all_three_slots_to_recapture() {
    let (mut session, _, _) = fixture();
    let context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    let now = Instant::now();
    let mut slots: [FrameSnapshot; 3] = std::array::from_fn(|_| Default::default());
    for slot in &mut slots {
        session.set_surface_size([700, 400]);
        assert_eq!(
            publish(&mut session, slot, &context, now),
            PublishOutcome::Published
        );
    }
    session.set_surface_size([700, 400]);
    publish(&mut session, &mut slots[0], &context, now);
    assert_eq!(slots[0].copied_count(), 0);
    session
        .apply(SessionMessage::FrameResult(FrameResult::BuildFailed), now)
        .expect("failure");
    for slot in &mut slots {
        session.set_surface_size([700, 400]);
        assert_eq!(
            publish(&mut session, slot, &context, now),
            PublishOutcome::Published
        );
        assert_eq!(slot.copied_count(), session.terminal.size().1);
    }
}

/// An idle turn does not publish, and synchronized updates keep their prior ink hidden.
#[test]
fn publication_idle_and_sync_timeout_keep_existing_scheduling_rules() {
    let (mut session, _, _) = fixture();
    let context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    let now = Instant::now();
    let mut slot = FrameSnapshot::default();
    assert_eq!(
        publish(&mut session, &mut slot, &context, now),
        PublishOutcome::Published
    );
    assert_eq!(
        publish(&mut session, &mut slot, &context, now),
        PublishOutcome::NothingNew
    );
    session.feed(b"\x1b[?2026hpartial", now);
    assert_eq!(
        publish(
            &mut session,
            &mut slot,
            &context,
            now + Duration::from_millis(100)
        ),
        PublishOutcome::NothingNew
    );
    assert_eq!(
        publish(
            &mut session,
            &mut slot,
            &context,
            now + Duration::from_millis(201)
        ),
        PublishOutcome::Published
    );
}

/// Registrations in a dropped publication still reach the permanent view owner.
#[test]
fn skipped_publication_delivers_views_for_next_slot() {
    use crate::overlay::{
        anchor::Anchor,
        draw::{DrawList, Primitive, RectSpec},
        geometry::Rect,
        model::ViewState,
        provider::EventKinds,
        view::{OverlayView, ViewCtx},
    };
    /// Painted source values make a skipped registration or stale BufferView visible.
    struct Panel;
    impl OverlayView for Panel {
        fn build(&mut self, _: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
            let row = ctx
                .buffer
                .line(ctx.geometry.visible_lines().next().expect("line"))
                .expect("source");
            assert_eq!(row.cell(0).expect("cell").raw.ch, u32::from('B'));
            out.push(
                Anchor::Viewport(Rect::default()),
                puzzle_frame::ZSlot::Top,
                Primitive::Rect(RectSpec {
                    rect: Rect {
                        origin: [1.0, 2.0],
                        size: [3.0, 4.0],
                    },
                    color: 0xffccbbaa,
                    radius: 0.0,
                    stroke: 0.0,
                }),
            );
        }
    }
    let (mut session, _, _) = fixture();
    let mut context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    session
        .overlays_mut()
        .add_view("skip-panel", EventKinds::GRID, Box::new(Panel));
    let now = Instant::now();
    session.feed(b"A", now);
    let mut skipped = FrameSnapshot::default();
    publish(&mut session, &mut skipped, &context, now);
    let mut builder = FrameBuilder::new(&context);
    builder.accept_snapshot_views(&mut skipped);
    session.feed(b"\rB", now);
    let mut latest = FrameSnapshot::default();
    publish(&mut session, &mut latest, &context, now);
    let mut frame = RenderFrame::default();
    builder
        .build_published(&mut context, &mut latest, &mut frame)
        .expect("latest");
    assert!(frame.rects.iter().any(|rect| rect.color == 0xffccbbaa));
}

/// Warm damaged-row publication retains cells, outbox and model-source capacity (P1).
#[test]
fn warm_publication_allocates_nothing() {
    let (mut session, _, _) = fixture();
    let context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    let now = Instant::now();
    let mut slot = FrameSnapshot::default();
    // VT retains 64 journal entries; warm the whole existing ring, not only row cells.
    for _ in 0..64 {
        session.feed(b"\rX", now);
        publish(&mut session, &mut slot, &context, now);
        while session.take_event().is_some() {}
    }
    let feed_allocations = allocation_counter::measure(|| {
        session.feed(b"\rY", now);
    });
    let allocations = allocation_counter::measure(|| {
        assert_eq!(
            publish(&mut session, &mut slot, &context, now),
            PublishOutcome::Published
        );
        assert_eq!(slot.copied_count(), 1);
        while session.take_event().is_some() {}
    });
    assert_eq!(
        (feed_allocations.count_total, allocations.count_total),
        (0, 0)
    );
}

/// Both host and test use generation values rather than borrowing fonts during capture.
fn publish(
    session: &mut TestHost,
    slot: &mut FrameSnapshot,
    context: &TextContext,
    now: Instant,
) -> PublishOutcome {
    session
        .publish_frame(now, slot, context.atlas_generation(), 0)
        .expect("publication")
}

/// The publication API rotates slots independently of the builder's consumed frames.
#[test]
fn published_rotating_slots_match_cold_rows_and_keep_copies_proportional() {
    let (mut session, _, _) = fixture();
    let mut context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    let now = Instant::now();
    session.resize_grid(12, 4, [140, 100]).expect("small grid");
    session.feed(b"one\r\ntwo\r\nthree\r\nfour\x1b[?25l", now);
    let mut slots: [FrameSnapshot; 3] = std::array::from_fn(|_| Default::default());
    for slot in &mut slots {
        session.set_surface_size([140, 100]);
        publish(&mut session, slot, &context, now);
    }
    let mut builder = FrameBuilder::new(&context);
    let mut frame = RenderFrame::default();
    for turn in 0..12 {
        if turn % 3 == 0 {
            session.feed(format!("\x1b[2;1H{turn:02}\x1b[4;5H").as_bytes(), now);
        }
        session.set_surface_size([140, 100]);
        let slot = &mut slots[turn % 3];
        assert_eq!(
            publish(&mut session, slot, &context, now),
            PublishOutcome::Published
        );
        assert!(slot.copied_count() <= 1);
        if turn % 4 == 0 {
            builder.accept_snapshot_views(slot);
            continue;
        }
        builder
            .build_published(&mut context, slot, &mut frame)
            .expect("rotated build");
        let mut cold = FrameBuilder::new(&context);
        cold.set_surface_size([140, 100]);
        let mut expected = RenderFrame::default();
        cold.build(&mut context, session.terminal(), &mut expected)
            .expect("cold build");
        assert_eq!(frame.clear, expected.clear);
        assert_eq!(
            format!("{:?}", frame.rects),
            format!("{:?}", expected.rects)
        );
        assert_eq!(
            format!("{:?}", frame.slugs),
            format!("{:?}", expected.slugs)
        );
        assert_eq!(
            format!("{:?}", frame.rasters),
            format!("{:?}", expected.rasters)
        );
    }
}

/// Source geometry, focus and blink remain usable before any host has built ink.
#[test]
fn publication_and_input_need_no_session_owned_drawing_cache() {
    let (mut session, writes, _) = fixture();
    let mut context = super::overlays::text_context(1.0);
    let now = Instant::now();
    session.set_text_context(&context);
    session.feed(b"\x1b[?1004h\x1b[?25l\x1b[5mX", now);
    session
        .apply(
            SessionMessage::Command(Command::Focus(false), Origin::Platform),
            now,
        )
        .expect("focus");
    // Input uses source preferences, not a dummy drawing owner or a frame loan.
    assert_eq!(
        writes.lock().expect("writer").last().expect("report"),
        b"\x1b[O"
    );
    assert!(session.builder.is_none());
    let mut slot = FrameSnapshot::default();
    publish(&mut session, &mut slot, &context, now);
    assert!(
        session.blink_deadline.is_some(),
        "visible SGR-5 ink schedules a timer"
    );
    assert!(session.builder.is_none());
    // Independent drawing adopts the frozen policy without installing a cache in Session.
    let mut builder = FrameBuilder::new(&context);
    let mut frame = RenderFrame::default();
    builder
        .build_published(&mut context, &mut slot, &mut frame)
        .expect("draw");
    assert!(!frame.slugs.is_empty());
    publish(
        &mut session,
        &mut slot,
        &context,
        now + Duration::from_millis(530),
    );
    builder
        .build_published(&mut context, &mut slot, &mut frame)
        .expect("blink");
    assert!(frame.slugs.is_empty());
    assert!(session.builder.is_none());
}
