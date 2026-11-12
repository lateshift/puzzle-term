//! Visible image publications retain source generations and exact grid geometry.
use super::*;
use crate::{
    PublishOutcome, SessionMessage,
    frame_snapshot::FrameSnapshot,
    images::{DecodeResult, DecodedImage},
};
use puzzle_frame::{PipelineKind, ZSlot};
/// Drive the public decode completion seam without a codec or GPU in the core.
pub(super) fn pixel(session: &mut Session, command: &str, now: Instant) {
    session.feed(
        format!("\x1b_Ga=T,f=32,s=1,v=1,C=1,{command};/wAA/w==\x1b\\").as_bytes(),
        now,
    );
    let request = session.take_image_decode_request().expect("decode request");
    session
        .apply(
            SessionMessage::ImageDecoded(DecodeResult {
                ticket: request.ticket,
                image: Ok(DecodedImage {
                    size: [1, 1],
                    rgba: Arc::from([255, 0, 0, 255]),
                }),
            }),
            now,
        )
        .expect("completion");
}
/// Snapshot pixels remain valid after parser-side deletion; image slots respect paper/text.
#[test]
fn published_images_seal_sources_and_order_all_three_layers() {
    let (mut session, _, _) = fixture();
    let mut context = super::overlays::text_context(2.0);
    session.set_text_context(&context);
    session.enable_images(4096);
    let now = Instant::now();
    session.feed(b"\x1b[?25l\x1b[41mabc\x1b[0m\r", now);
    for (id, z) in [(3, 0), (2, -1), (1, i32::MIN)] {
        pixel(&mut session, &format!("i={id},c=2,r=2,z={z}"), now);
    }
    let mut slot = FrameSnapshot::default();
    session
        .publish_frame(now, &mut slot, context.atlas_generation(), 0)
        .expect("publish");
    let sealed = slot.images.as_ref().expect("images");
    assert_eq!(
        sealed.placements.iter().map(|p| p.id).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(sealed.placements[0].anchor, [0, 0]);
    session.feed(b"\x1b_Ga=d,d=A\x1b\\", now);
    assert!(session.images().expect("store").images().is_empty());
    let mut builder = FrameBuilder::new(&context);
    let mut frame = RenderFrame::default();
    builder
        .build_published(&mut context, &mut slot, &mut frame)
        .expect("sealed build");
    assert_image_slots(&frame);
    assert!(
        frame
            .batches
            .windows(2)
            .all(|pair| pair[0].slot <= pair[1].slot)
    );
    assert_eq!(
        frame.images.as_ref().expect("frame images").sources.len(),
        3
    );
    let cell = context.metrics();
    assert_eq!(
        frame.rasters[0].rect,
        [
            16.0,
            16.0,
            (cell.width_px * 2) as f32,
            (cell.width_px * 2) as f32
        ]
    );
    // Sealing again after deletion removes all placements without retaining source ink.
    session
        .publish_frame(now, &mut slot, context.atlas_generation(), 0)
        .expect("republish");
    assert!(
        slot.images
            .as_ref()
            .expect("retained allocation")
            .placements
            .is_empty()
    );
}
/// Invisible/transmit-only sources never enter the lazy render payload; idle stays free.
#[test]
fn image_publications_are_idle_and_warm_captures_allocate_nothing() {
    let (mut session, _, _) = fixture();
    let context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    session.enable_images(4096);
    let now = Instant::now();
    pixel(&mut session, "i=1", now);
    let mut slot = FrameSnapshot::default();
    session
        .publish_frame(now, &mut slot, 0, 0)
        .expect("warm publication");
    // Direct capture forces metadata copies, avoiding a false zero-work allocator proof.
    let measured = allocation_counter::measure(|| {
        slot.capture_images(session.images(), session.terminal(), 0);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(
        session.publish_frame(now, &mut slot, 0, 0).expect("idle"),
        PublishOutcome::NothingNew
    );
}

/// Font/DPR changes move cell anchors and cell-sized images without scaling natural pixels.
#[test]
fn image_anchors_follow_current_font_metrics_and_dpr() {
    let (mut session, _, _) = fixture();
    let mut context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    session.enable_images(4096);
    let now = Instant::now();
    session.feed(b"\x1b[?25l\x1b[2;4H", now);
    pixel(&mut session, "i=1,c=2,r=2", now);
    pixel(&mut session, "i=2", now);
    let mut slot = FrameSnapshot::default();
    let mut frame = RenderFrame::default();
    let mut builder = FrameBuilder::new(&context);
    for dpr in [1.0, 2.0] {
        context = super::overlays::text_context(dpr);
        session.set_text_context(&context);
        session
            .publish_frame(
                now,
                &mut slot,
                context.atlas_generation(),
                builder.style_epoch(),
            )
            .expect("image frame operation");
        builder
            .build_published(&mut context, &mut slot, &mut frame)
            .expect("image frame operation");
        let metrics = context.metrics();
        let expected = [
            16.0 + 3.0 * metrics.width_px as f32,
            16.0 + metrics.height_px as f32,
        ];
        assert_eq!(frame.rasters[0].rect[..2], expected);
        assert_eq!(
            frame.rasters[0].rect[2..],
            [2.0 * metrics.width_px as f32; 2]
        );
        assert_eq!(frame.rasters[1].rect[..2], expected);
        assert_eq!(frame.rasters[1].rect[2..], [1.0; 2]);
    }
}

/// Images occupy three layers independently of the grid glyph representation.
fn assert_image_slots(frame: &RenderFrame) {
    let slots: Vec<_> = frame
        .batches
        .iter()
        .filter(|b| matches!(b.pipeline, PipelineKind::Image(_)))
        .map(|b| b.slot)
        .collect();
    assert_eq!(
        slots,
        [ZSlot::Background, ZSlot::BelowText, ZSlot::AboveText]
    );
}

/// Placeholder tests use the same Session publication seam as physical placements.
mod placeholders;

/// Exact legacy yazi puts retain every sibling and their integer source crops.
mod yazi;
