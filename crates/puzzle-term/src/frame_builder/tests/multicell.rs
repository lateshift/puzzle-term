//! OSC 66 uses the ordinary glyph pipeline and physical host coordinates.
use super::*;
/// Sized pixel-only fallback must retain source sampling and cursor maps.
mod bitmap;

/// One natural source is scaled once, with no ink for its continuation cells.
#[test]
fn text_sizing_heading_vectors_cursor_and_physical_hit() {
    for dpr in [1.0, 2.0] {
        let mut context = TextContext::new(fonts(), 14.0, dpr).expect("metrics");
        let mut builder = FrameBuilder::new(&context);
        let mut terminal = Terminal::new(12, 5, 0, 0);
        // CUP deliberately puts the physical caret in the lower-right fragment.
        terminal.advance(b"\x1b]66;s=2;A\x07\x1b[2;2H");
        let mut snapshot = FrameSnapshot::default();
        snapshot
            .capture(&terminal, builder.style_epoch())
            .expect("snapshot");
        let mut frame = RenderFrame::default();
        builder
            .build_snapshot(&mut context, &snapshot, &mut frame)
            .expect("heading");
        assert_eq!(builder.rows[0].sources.len(), 1);
        assert!(builder.rows[1].sources.is_empty());
        let source = builder.rows[0].sources[0];
        assert_eq!(source.placement.key.hint_ppem, 0);
        assert!(source.placement.effect);
        assert_eq!(builder.rows[0].block_transforms.len(), 1);
        assert_eq!(
            builder.rows[0].cursor_glyphs.len(),
            builder.rows[0].glyphs.len()
        );
        let rect = builder.cursor_extent(&snapshot).expect("IME extent");
        let cell = builder.geometry.cell_size();
        assert_eq!(rect[2..], [2.0 * cell[0], 2.0 * cell[1]]);
        let hit = builder
            .geometry
            .hit_test([rect[0] + 1.5 * cell[0], rect[1] + 1.5 * cell[1]]);
        assert_eq!(hit.cell.expect("physical position").column, 1);
        assert_eq!(
            hit.block.expect("block extent").rect.size,
            [rect[2], rect[3]]
        );
    }
}

/// Fractional rendering aligns within the footprint and does not move later cells.
#[test]
fn text_sizing_fractional_alignment_and_mixed_cells() {
    let mut context = TextContext::new(fonts(), 14.0, 2.0).expect("metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(16, 5, 0, 0);
    terminal.advance(b"\x1b]66;s=2:n=1:d=2:h=1:v=1;A\x07BC\x1b[?25l");
    let mut frame = RenderFrame::default();
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("mixed ink");
    let c = builder.rows[0].block_transforms[0].map.as_coeffs();
    // Uniform fitting accounts for fractional natural advances at integer cell sizes.
    assert!(c[0] > 0.9 && c[0] <= 1.0);
    assert_eq!(c[0], c[3]);
    assert_eq!(c[4], f64::from(builder.metrics.width_px));
    assert_eq!(c[5], f64::from(builder.metrics.height_px));
    assert!(
        builder.rows[0]
            .sources
            .iter()
            .any(|s| s.placement.cell_column == 2)
    );
    // Re-enter through a lower fragment; a retained cache must equal cold assembly.
    terminal.advance(b"\x1b[2;2H\x1b[?25h");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("caret moved");
    let mut cold = FrameBuilder::new(&context);
    let mut expected = RenderFrame::default();
    cold.build(&mut context, &terminal, &mut expected)
        .expect("cold");
    assert_eq!(
        format!("{:?}", frame.slugs),
        format!("{:?}", expected.slugs)
    );
}

/// All three publication slots can skip edits while a lower fragment becomes its owner.
#[test]
fn text_sizing_rotating_slots_and_clipped_cursor_match_cold() {
    for dpr in [1.0, 2.0] {
        let mut context = TextContext::new(fonts(), 14.0, dpr).expect("metrics");
        let mut builder = FrameBuilder::new(&context);
        let mut terminal = Terminal::new(12, 5, 8, 32 * 1024);
        terminal.advance(b"\x1b]66;s=3;ABC\x07\x1b[?25l");
        let mut slots: [FrameSnapshot; 3] = std::array::from_fn(|_| FrameSnapshot::default());
        let mut frame = RenderFrame::default();
        for index in 0..9 {
            let snapshot = &mut slots[index % 3];
            // Scroll a viewport, not the stored rectangle. Its VT versions stay unchanged.
            snapshot
                .capture_viewport(&terminal, 0, index % 3)
                .expect("publication");
            builder
                .build_snapshot(&mut context, snapshot, &mut frame)
                .expect("incremental");
            let mut cold = FrameBuilder::new(&context);
            let mut expected = RenderFrame::default();
            cold.build_snapshot(&mut context, snapshot, &mut expected)
                .expect("cold");
            assert_eq!(
                format!("{:?}", frame.slugs),
                format!("{:?}", expected.slugs)
            );
            assert_eq!(builder.geometry.blocks.len(), 3);
            assert!(builder.rows[1].sources.is_empty());
        }
    }
}

/// Font overhangs and overlong explicit payloads cannot paint another rectangle.
#[test]
fn text_sizing_explicit_run_ink_fits_one_rectangle() {
    for dpr in [1.0, 2.0] {
        let mut context = TextContext::new(fonts(), 14.0, dpr).expect("metrics");
        let mut builder = FrameBuilder::new(&context);
        let mut terminal = Terminal::new(20, 6, 0, 0);
        terminal.advance(b"\x1b[3m\x1b]66;s=2:w=1;ffffjjjjMMMM\x07\x1b[?25l");
        let mut frame = RenderFrame::default();
        builder
            .build(&mut context, &terminal, &mut frame)
            .expect("fitted run");
        let size = [
            2.0 * builder.metrics.width_px as f32,
            2.0 * builder.metrics.height_px as f32,
        ];
        for glyph in &builder.rows[0].placements {
            for (axis, bound) in size.iter().enumerate() {
                assert!(glyph.ink.origin[axis] >= -0.001, "{:?}", glyph.ink);
                assert!(
                    glyph.ink.origin[axis] + glyph.ink.size[axis] <= *bound + 0.001,
                    "{:?}",
                    glyph.ink
                );
            }
        }
        assert!(builder.rows[0].sources.len() > 1);
        assert_eq!(builder.rows[0].block_transforms.len(), 1);
    }
}

/// Startup packs still draw supported headings before any full font arrives.
#[test]
fn text_sizing_pack_first_frame_has_unhinted_heading_ink() {
    let bytes = std::fs::read(format!(
        "{}/../../assets/fontpacks/jetbrains-regular.pzfp",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("pack");
    let pack = puzzle_text::pack::FontPack::decode(&bytes).expect("directory");
    let mut context = TextContext::from_pack(pack, 14.0, 2.0).expect("metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(12, 5, 0, 0);
    terminal.advance(b"\x1b]66;s=3;A\x07\x1b[?25l");
    let mut frame = RenderFrame::default();
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("pack heading");
    assert_eq!(frame.slugs.len(), 1);
    assert_eq!(builder.rows[0].sources.len(), 1);
    assert!(builder.rows[0].sources[0].placement.effect);
    assert!(frame.slugs[0].m[0] > 2.0 * builder.metrics.device_size);
}
