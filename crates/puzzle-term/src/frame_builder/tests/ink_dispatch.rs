//! Warm backend dispatch does only work which can produce ink (§8.9, M15.4b).
use super::*;

/// Vector-only contexts have no raster candidates, regardless of Unicode content.
#[test]
fn vector_only_rebuild_skips_raster_lookups_without_changing_ink() {
    let mut context = TextContext::new(fonts(), 14.0, 2.0).expect("metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(16, 2, 0, 0);
    terminal.advance("Abc é ß →\r\nXYZ".as_bytes());
    let mut frame = RenderFrame::default();
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("cold ink");
    let ink: Vec<_> = frame.slugs.iter().map(slug).collect();
    assert!(!ink.is_empty());
    context.measure_rows();
    let before = context.cache_metrics()[4];
    builder.set_context(&context);
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("warm ink");
    assert_eq!(
        context.cache_metrics()[4],
        before,
        "empty raster store cannot answer a probe"
    );
    assert_eq!(frame.slugs.iter().map(slug).collect::<Vec<_>>(), ink);
}

/// Turning on grid masks must populate and then reuse the same raster route.
#[test]
fn populated_masks_are_reused_once_per_placement() {
    let mut context = TextContext::new(fonts(), 14.0, 2.0).expect("metrics");
    context
        .backend
        .set_grid_renderer(puzzle_text::glyph::RendererChoice::Atlas);
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(4, 1, 0, 0);
    terminal.advance(b"\x1b[?25lABCD");
    let mut frame = RenderFrame::default();
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("cold masks");
    assert_eq!(frame.rasters.len(), 4);
    let ink: Vec<_> = frame.rasters.iter().map(raster).collect();
    context.measure_rows();
    let before = context.cache_metrics()[4];
    builder.set_context(&context);
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("retained masks");
    let after = context.cache_metrics()[4];
    assert_eq!(after[0] - before[0], 4, "one raster hit per grid glyph");
    assert_eq!(after[1] - before[1], 0);
    assert_eq!(frame.rasters.iter().map(raster).collect::<Vec<_>>(), ink);
}

/// Every vector draw word contributes to the unchanged output contract.
fn slug(s: &puzzle_frame::SlugInstance) -> ([f32; 4], [f32; 2], [f32; 4], [u32; 4]) {
    (
        s.m,
        s.t,
        s.bbox,
        [s.glyph_loc, s.band_info, s.color, s.paint],
    )
}

/// Authored UVs and tint must survive the single retained lookup unchanged.
fn raster(s: &puzzle_frame::RasterInstance) -> ([f32; 4], [f32; 4], u32, u32) {
    (s.rect, s.uv, s.tint, s.flags)
}

/// M15.6's row summary removes every placeholder probe from ordinary full rebuilds.
#[test]
fn ordinary_rows_perform_zero_placeholder_checks() {
    for dpr in [1.0, 2.0] {
        let mut context = TextContext::new(fonts(), 14.0, dpr).expect("metrics");
        let mut builder = FrameBuilder::new(&context);
        let mut terminal = Terminal::new(120, 40, 0, 0);
        terminal.advance("\x1b[?25lASCII é ß →\r\nordinary Unicode".as_bytes());
        let mut frame = RenderFrame::default();
        let before = super::super::row::placeholder_checks();
        let decorations = super::super::row::decoration_cells();
        builder
            .build(&mut context, &terminal, &mut frame)
            .expect("ordinary frame");
        assert_eq!(super::super::row::placeholder_checks(), before);
        assert_eq!(super::super::row::decoration_cells(), decorations);
        assert!(!frame.slugs.is_empty());
        // Unknown image IDs must still suppress glyphs and decorations on flagged rows.
        terminal.advance("\x1b[1;1H\x1b[4m\u{10eeee}".as_bytes());
        builder
            .build(&mut context, &terminal, &mut frame)
            .expect("placeholder frame");
        assert_eq!(super::super::row::placeholder_checks() - before, 120);
    }
}
