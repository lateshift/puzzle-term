//! Stored VT cell spans are authoritative for text and block-cursor ink.
use super::*;

/// A cursor over a wide leading cell spans both terminal columns.
#[test]
fn wide_cursor_and_unfocused_outline() {
    let mut context = TextContext::new(fonts(), 14.0, 1.0).expect("valid cell metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(8, 1, 0, 0);
    let mut frame = RenderFrame::default();
    terminal.advance("界\x1b[1G\x1b[2 q".as_bytes());
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("wide cursor");
    let block = frame.rects.last().expect("cursor rectangle");
    assert_eq!(block.rect[2], 2.0 * builder.metrics().width_px as f32);
    builder.set_focus(false);
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("unfocused cursor");
    let hollow = frame.rects.last().expect("cursor rectangle");
    assert_eq!(hollow.stroke_width, 1.0);
}

/// A trailing prepend mark stays attached to the VT cell, even at delayed wrap.
#[test]
fn trailing_prepend_cluster_cannot_create_an_out_of_grid_glyph() {
    // Reproduces presentation 129's U+070F at column 282 without a PTY,
    // parser thread or mutex: string re-segmentation is sufficient to fail.
    let mut context = TextContext::new(fonts(), 15.0, 2.0).expect("metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(282, 1, 0, 0);
    terminal.advance(format!("{}\u{070f}", "漢".repeat(141)).as_bytes());
    let last = terminal.screen().row(0).expect("row");
    assert_eq!(last.extras.last().expect("cluster").column, 280);
    assert_eq!(
        last.extras.last().expect("cluster").cluster.as_deref(),
        Some("漢\u{070f}")
    );
    builder
        .build(&mut context, &terminal, &mut RenderFrame::default())
        .expect("frame");
    assert!(
        builder.rows[0]
            .placements
            .iter()
            .all(|glyph| glyph.cell.column < 282),
        "re-segmenting VT clusters created an out-of-grid draw"
    );
    assert!(
        builder.rows[0]
            .placements
            .iter()
            .any(|glyph| glyph.cell.column == 280 && glyph.cells == 2),
        "the stored two-cell cluster lost its ownership"
    );
}
