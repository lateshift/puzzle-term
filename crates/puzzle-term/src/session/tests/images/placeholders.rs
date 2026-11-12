//! Text-backed image tiles preserve publications and never enter glyph fallback.
use super::*;

/// Known sources must suppress every reserved text style, for indexed and RGB IDs.
#[test]
fn known_placeholder_ids_ignore_all_text_decorations() {
    for (id, colour) in [
        (1, "38;5;1"),
        (42, "38;5;42"),
        (255, "38;5;255"),
        (42, "38;2;0;0;42"),
    ] {
        for style in 0..=5 {
            let (mut host, _, _) = fixture();
            let mut context = super::super::overlays::text_context(2.0);
            host.set_text_context(&context);
            host.enable_images(4096);
            let now = Instant::now();
            pixel(&mut host, &format!("U=1,i={id},p=7,c=2,r=2"), now);
            // SGR 58 is placement metadata. Other styles cannot hide or decorate it.
            host.feed(format!("\x1b[?25l\x1b[{colour};58;5;7;4:{style};1;2;3;5;7;8;9;53m\u{10eeee}\u{0305}\u{0305}").as_bytes(), now);
            let frame = host.draw(now, &mut context).expect("known placeholder");
            assert!(frame.rects.is_empty(), "id={id}, style={style}: text bars");
            assert!(frame.slugs.is_empty(), "id={id}, style={style}: glyph ink");
            assert_eq!(
                frame.rasters.len(),
                1,
                "id={id}, style={style}: tile absent"
            );
            assert_eq!(frame.images.as_ref().expect("image").sources.len(), 1);
        }
    }
}

/// Invisible prototypes produce no frame payload until matching cells exist.
#[test]
fn virtual_tiles_are_sealed_batched_and_never_glyphs() {
    for dpr in [1.0, 2.0] {
        let (mut host, _, _) = fixture();
        let mut context = super::super::overlays::text_context(dpr);
        host.set_text_context(&context);
        host.enable_images(4096);
        let now = Instant::now();
        host.feed(b"\x1b[?25l", now);
        pixel(&mut host, "U=1,i=42,p=7,c=4,r=4", now);
        let frame = host.draw(now, &mut context).expect("prototype publication");
        assert!(frame.images.as_ref().is_none_or(|i| i.sources.is_empty()));
        host.feed(
            "\x1b[38;5;42;58;5;7m\u{10eeee}\u{0305}\u{0305}\u{10eeee}\u{10eeee}\u{10eeee}"
                .as_bytes(),
            now,
        );
        let frame = host.draw(now, &mut context).expect("tiles");
        let batches: Vec<_> = frame
            .batches
            .iter()
            .filter(|b| matches!(b.pipeline, PipelineKind::Image(_)))
            .collect();
        assert_eq!(batches.len(), 1, "tiles must share one instanced draw");
        // A contiguous run clips one placement-wide transform, never four UV fits.
        assert_eq!(batches[0].instances.len(), 1);
        assert_eq!(frame.images.as_ref().expect("source").sources.len(), 1);
        assert!(
            frame.slugs.is_empty(),
            "placeholder must not request font ink"
        );
        let quad = frame.rasters[batches[0].instances.start as usize];
        assert_eq!(quad.uv, [0.0, 0.0, 1.0, 1.0]);
        let cell = context.metrics();
        assert_eq!(
            quad.rect,
            [
                16.0,
                16.0,
                (4 * cell.width_px) as f32,
                (4 * cell.width_px) as f32
            ]
        );
        assert_eq!(
            batches[0].scissor,
            Some(puzzle_frame::RectU32 {
                x: 16,
                y: 16,
                width: 4 * cell.width_px,
                height: cell.height_px
            })
        );
        // Publishing with an unchanged prototype and row reuses every allocation.
        let mut slot = FrameSnapshot::default();
        host.publish_frame(now, &mut slot, context.atlas_generation(), 0)
            .expect("warm slot");
        let allocations =
            allocation_counter::measure(|| slot.capture_images(host.images(), host.terminal(), 0));
        assert_eq!(allocations.count_total, 0);
    }
}
/// A deleted tile remains a hole; other spans retain the placement's full transform.
#[test]
fn placeholder_clips_preserve_holes_and_share_the_full_transform() {
    let (mut host, _, _) = fixture();
    let mut context = super::super::overlays::text_context(2.0);
    host.set_text_context(&context);
    host.enable_images(4096);
    let now = Instant::now();
    pixel(&mut host, "U=1,i=42,p=7,c=4,r=2", now);
    let marks = ['\u{0305}', '\u{030d}', '\u{030e}', '\u{0310}'];
    for row in 0..2 {
        for column in 0..4 {
            // Explicit coordinates ensure the hole cannot change left inheritance.
            host.feed(
                format!(
                    "\x1b[{};{}H\x1b[38;5;42;58;5;7m\u{10eeee}{}{}",
                    row + 1,
                    column + 1,
                    marks[row],
                    marks[column]
                )
                .as_bytes(),
                now,
            );
        }
    }
    host.feed(b"\x1b[?25l\x1b[1;2H\x1b[X", now);
    let frame = host.draw(now, &mut context).expect("edited tiles");
    let batches: Vec<_> = frame
        .batches
        .iter()
        .filter(|b| matches!(b.pipeline, PipelineKind::Image(_)))
        .collect();
    assert_eq!(batches.len(), 3, "two broken spans plus one whole row");
    let cell = context.metrics();
    let expected = [(0, 0, 1), (2, 0, 2), (0, 1, 4)];
    let first = frame.rasters[batches[0].instances.start as usize];
    for (batch, (x, y, columns)) in batches.iter().zip(expected) {
        let quad = frame.rasters[batch.instances.start as usize];
        assert_eq!(quad.rect, first.rect, "clipping cannot re-fit the source");
        assert_eq!(quad.uv, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            batch.scissor,
            Some(puzzle_frame::RectU32 {
                x: 16 + x * cell.width_px,
                y: 16 + y * cell.height_px,
                width: columns * cell.width_px,
                height: cell.height_px
            })
        );
    }
}
/// Unknown image/placement IDs, bad marks and disabled images all stay blank.
#[test]
fn unknown_placeholders_never_prepare_tofu() {
    for enable in [false, true] {
        let (mut host, _, _) = fixture();
        let mut context = super::super::overlays::text_context(2.0);
        host.set_text_context(&context);
        let now = Instant::now();
        if enable {
            host.enable_images(4096);
            pixel(&mut host, "U=1,i=42,p=7,c=4,r=4", now);
        }
        host.feed(
            concat!(
                "\x1b[?25l\x1b[38;5;99m\u{10eeee}",
                "\x1b[38;5;42;58;5;8m\u{10eeee}",
                "\x1b[58;5;7m\u{10eeee}\u{0300}",
                "\x1b[48;2;1;2;3;38;2;18;52;86;7;4;9m\u{10eeee}"
            )
            .as_bytes(),
            now,
        );
        let frame = host.draw(now, &mut context).expect("unknown placeholders");
        assert!(frame.slugs.is_empty());
        // Reserved inverse/underline/strike attributes must not turn ID metadata
        // into paper or decorations; the explicit background still paints.
        assert_eq!(frame.rects.len(), 1);
        assert_eq!(frame.rects[0].color, 0xff030201);
        assert!(frame.rasters.is_empty());
        assert!(frame.images.as_ref().is_none_or(|i| i.sources.is_empty()));
    }
}
/// Tiles move with text under wrap reflow and scrolling; ED removes ink, not prototypes.
#[test]
fn placeholder_tiles_follow_reflow_scroll_and_clear() {
    let (mut host, _, _) = fixture();
    let context = super::super::overlays::text_context(1.0);
    host.set_text_context(&context);
    host.enable_images(4096);
    let now = Instant::now();
    host.resize_grid(4, 3, [64, 96]).expect("initial geometry");
    pixel(&mut host, "U=1,i=42,c=4,r=4", now);
    // Explicit columns survive wrapping; inherited columns are intentionally row-local.
    host.feed("\x1b[38;5;42m\u{10eeee}\u{0305}\u{0305}\u{10eeee}\u{0305}\u{030d}\u{10eeee}\u{0305}\u{030e}\u{10eeee}\u{0305}\u{0310}X".as_bytes(), now);
    host.resize_grid(2, 3, [48, 96]).expect("reflow");
    let mut slot = FrameSnapshot::default();
    host.publish_frame(now, &mut slot, context.atlas_generation(), 0)
        .expect("reflow publication");
    let placements = &slot.images.as_ref().expect("tiles").placements;
    assert_eq!(placements.len(), 4);
    assert!(
        placements
            .iter()
            .any(|p| p.anchor == [0, 1] && p.tile == Some([0, 2]))
    );
    host.feed(b"\r\n\r\n\r\n", now);
    // Follow the new live viewport: old placeholders now live only in history.
    let top = host.terminal().screen().ring.scrollback_len();
    slot.capture_images(host.images(), host.terminal(), top);
    assert!(
        slot.images
            .as_ref()
            .expect("reused storage")
            .placements
            .is_empty()
    );
    // Looking back into history restores the same prototype and source coordinates.
    slot.capture_images(host.images(), host.terminal(), 0);
    assert_eq!(
        slot.images
            .as_ref()
            .expect("history tiles")
            .placements
            .len(),
        4
    );
    host.feed(b"\x1b[3J\x1b[2J", now);
    slot.capture_images(host.images(), host.terminal(), 0);
    assert!(slot.images.as_ref().expect("cleared").placements.is_empty());
    assert_eq!(
        host.images()
            .expect("prototype survives text erase")
            .placements()
            .len(),
        1
    );
}
