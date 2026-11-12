//! Every changing Unicode screen must be complete before its first presentation.
//!
//! The viewport rotates 5,120 distinct installed-font CJK glyphs at 60 Hz.
//! No worker or completion drain may rescue an earlier provisional frame.
#![allow(
    clippy::expect_used,
    reason = "fixture setup and frame errors must fail this regression"
)]
use super::*;
use puzzle_term::{frame::RenderFrame, frame_builder::FrameBuilder, vt::terminal::Terminal};

/// Every covered glyph must reach its atlas while its original row keeps disappearing.
#[test]
fn changing_cjk_screen_caches_completed_ink() {
    let mut context = TextContext::new(
        crate::font_data::bundled_font_set().expect("bundled faces"),
        14.0,
        1.0,
    )
    .expect("context");
    install(&mut context, &[]).expect("source installation");
    let mut builder = FrameBuilder::new(&context);
    let mut vt = Terminal::new(160, 64, 0, 0);
    let mut frame = RenderFrame::default();
    let mut scalars: Vec<char> = (0x4e00..0x6200).filter_map(char::from_u32).collect();
    // Every replacement is complete immediately, not eventually after a frozen tail.
    for _ in 0..4 {
        vt.advance(b"\x1bc\x1b[?25l");
        scalars.rotate_left(127);
        let text: String = scalars.iter().collect();
        vt.advance(text.as_bytes());
        builder
            .build(&mut context, &vt, &mut frame)
            .expect("changing frame");
        assert!(!context.sources_pending());
        assert!(super::flood::complete_ink(&builder, &mut context));
    }
    assert_warm(&mut context, &vt, &mut builder, &mut frame);
}

/// A forced full redraw proves the changing screen retained reusable ink rather than stale rows.
fn assert_warm(
    context: &mut TextContext,
    vt: &Terminal,
    builder: &mut FrameBuilder,
    frame: &mut RenderFrame,
) {
    builder.set_context(context);
    let prepared = context.prepared_glyphs();
    let measured =
        allocation_counter::measure(|| builder.build(context, vt, frame).expect("warm frame"));
    assert_eq!(measured.count_total, 0);
    assert_eq!(context.prepared_glyphs(), prepared);
    assert!(super::flood::complete_ink(builder, context));
    assert!(!context.sources_pending());
}
