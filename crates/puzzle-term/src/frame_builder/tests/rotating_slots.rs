//! D-50 snapshots retain row data independently of the renderer's cache history.
use super::*;

/// Slots can skip publications; their copy flags cannot prescribe builder cache hits.
#[test]
fn three_slots_match_cold_rebuild_after_skipped_publications() {
    let mut context = TextContext::new(fonts(), 14.0, 2.0).expect("metrics");
    let mut terminal = Terminal::new(12, 4, 0, 0);
    terminal.advance(b"one\r\ntwo\r\nthree\r\nfour\x1b[?25l");
    let mut slots: [FrameSnapshot; 3] = std::array::from_fn(|_| Default::default());
    let mut builder = FrameBuilder::new(&context);
    // Each free slot has seen the grid, but none of these publications was drawn.
    for slot in &mut slots {
        slot.capture(&terminal, builder.style_epoch())
            .expect("warm slot");
    }
    let mut frame = RenderFrame::default();
    for turn in 0..12 {
        // Change only one row; other retained cells may still be absent in the builder.
        if turn % 3 == 0 {
            terminal.advance(format!("\x1b[2;1H{:02}\x1b[4;5H", turn).as_bytes());
        }
        let slot = &mut slots[turn % 3];
        slot.capture(&terminal, builder.style_epoch())
            .expect("capture");
        assert!(slot.copied_count() <= 1, "slot copied undamaged rows");
        // A dropped publication still advances that slot's local copy history.
        if turn % 4 == 0 {
            continue;
        }
        builder
            .build_snapshot(&mut context, slot, &mut frame)
            .expect("rotated frame");
        let mut cold = FrameBuilder::new(&context);
        let mut expected = RenderFrame::default();
        cold.build(&mut context, &terminal, &mut expected)
            .expect("cold frame");
        // Upload bookkeeping differs; the draw streams must be exactly identical.
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
