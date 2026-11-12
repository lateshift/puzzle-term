//! Zero-contrast diagnostics follow resolved cell colors, not corpus coordinates.
use super::*;
use crate::{frame_snapshot::FrameSnapshot, selection::SelectionRange};
use puzzle_vt::selection::SelectPoint;

/// Literal, inverse/dim and OSC palette colors use the same final policy as drawing.
#[test]
fn zero_contrast_uses_sealed_palette_and_contrast_policy() {
    let mut context = TextContext::new(fonts(), 15.0, 2.0).expect("metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut term = Terminal::new(8, 1, 0, 0);
    term.advance(b"\x1b[38;2;0;0;0;48;2;0;0;0mA\x1b[7;2mB\x1b[0;31;42mC");
    let mut frame = RenderFrame::default();
    builder
        .build(&mut context, &term, &mut frame)
        .expect("frame");
    let samples = builder.retained_ink_samples(&mut context);
    let mut slot = FrameSnapshot::default();
    slot.capture(&term, 0).expect("snapshot");
    assert!(samples.iter().all(|sample| sample.ready));
    assert!(builder.zero_contrast(&slot, &samples[0]));
    assert!(builder.zero_contrast(&slot, &samples[1]));
    assert!(!builder.zero_contrast(&slot, &samples[2]));
    // Updating live OSC colors cannot retroactively change the old frame's verdict.
    term.advance(b"\x1b]4;1;#112233;2;#112233\x1b\\");
    assert!(!builder.zero_contrast(&slot, &samples[2]));
    slot.capture(&term, 0).expect("new palette");
    assert!(builder.zero_contrast(&slot, &samples[2]));
    builder.set_color_options(false, 4.5);
    assert!(!builder.zero_contrast(&slot, &samples[0]));
}

/// Selection/search replace paper; the exemption cannot hide missing visible ink.
#[test]
fn interaction_paper_removes_the_equal_color_exemption() {
    let mut context = TextContext::new(fonts(), 15.0, 2.0).expect("metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut term = Terminal::new(4, 1, 0, 0);
    term.advance(b"\x1b[38;2;0;0;0;48;2;0;0;0mA");
    let mut frame = RenderFrame::default();
    builder
        .build(&mut context, &term, &mut frame)
        .expect("frame");
    let sample = builder.retained_ink_samples(&mut context).remove(0);
    let mut slot = FrameSnapshot::default();
    slot.capture(&term, 0).expect("snapshot");
    let point = SelectPoint {
        line: sample.line,
        column: 0,
    };
    let range = SelectionRange {
        start: point,
        end: point,
        block: false,
    };
    assert!(builder.zero_contrast(&slot, &sample));
    slot.set_selection(Some(range));
    assert!(!builder.zero_contrast(&slot, &sample));
    slot.set_selection(None);
    slot.set_search(&[crate::search::SearchFill {
        range,
        current: true,
    }]);
    assert!(!builder.zero_contrast(&slot, &sample));
}
