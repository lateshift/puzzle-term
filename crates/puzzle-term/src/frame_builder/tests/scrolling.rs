//! Exposing a new history row exercises P1/P4, rather than timing an idle cache hit.
use super::*;

/// The first history cache miss uses warmed glyphs but must not allocate row scratch.
#[test]
fn first_exposed_history_row_reuses_warmed_frame_storage() {
    check_first_history_row(false);
}

/// D-47 cached masks must not allocate when scrolling first exposes a previously unseen row.
#[test]
fn bitmap_first_exposed_history_row_reuses_warmed_frame_storage() {
    check_first_history_row(true);
}

/// Both storage choices share the identical history miss and allocation boundary.
fn check_first_history_row(bitmap: bool) {
    let mut context = TextContext::new(fonts(), 14.0, 2.0).expect("context");
    if bitmap {
        context.set_grid_renderer(puzzle_text::glyph::RendererChoice::Auto);
    }
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(240, 70, 500, 4_000_000);
    terminal
        .advance(&b"scroll ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz\r\n".repeat(500));
    terminal.advance(b"\x1b[?25l");
    let mut frame = RenderFrame::default();
    // Glyphs, shaping and frame arrays are warm; no older history cache was built.
    for _ in 0..2 {
        builder
            .build(&mut context, &terminal, &mut frame)
            .expect("warm live grid");
        frame.uploads.clear();
    }
    if bitmap {
        assert!(
            !frame.rasters.is_empty(),
            "history regression must exercise cached bitmap ink"
        );
    }
    let top = terminal.screen().ring.scrollback_len() - 1;
    let measured = allocation_counter::measure(|| {
        builder
            .build_viewport(&mut context, &terminal, top, &mut frame)
            .expect("first history row");
    });
    assert_eq!(builder.rebuilt_rows(), 1);
    assert_eq!(
        measured.count_total, 0,
        "cold history scratch: {measured:?}"
    );
}

/// Fractional scrolling alternates 70/71 rows; dropping spare caches creates hot allocations.
#[test]
fn exposed_history_row_retains_capacity_across_edge_changes() {
    let mut context = TextContext::new(fonts(), 14.0, 2.0).expect("context");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(240, 70, 500, 4_000_000);
    terminal
        .advance(&b"scroll ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz\r\n".repeat(500));
    terminal.advance(b"\x1b[?25l");
    let mut frame = RenderFrame::default();
    // Fill both bounded cache banks with the same reusable glyph set before counting.
    for top in (200..350).rev() {
        builder
            .build_scrolled(&mut context, &terminal, top, -1, &mut frame)
            .expect("warm banks");
        frame.uploads.clear();
    }
    // An integral frame must not discard the spare row allocations or atlas tokens.
    builder
        .build_scrolled(&mut context, &terminal, 199, 0, &mut frame)
        .expect("integral edge");
    frame.uploads.clear();
    let measured = allocation_counter::measure(|| {
        builder
            .build_scrolled(&mut context, &terminal, 198, -1, &mut frame)
            .expect("new history row");
    });
    assert_eq!(
        builder.rebuilt_rows(),
        1,
        "only the newly exposed row changes"
    );
    assert_eq!(
        measured.count_total, 0,
        "scroll discarded warmed storage: {measured:?}"
    );
}
