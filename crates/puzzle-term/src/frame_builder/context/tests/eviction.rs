//! Former pack pages may be evicted only with generation-safe ASCII rebuilding.
use super::*;
use puzzle_slug::{ShapeKey, atlas::FillRule, reference::RasterTransform};
use puzzle_text::glyph::{GlyphKey, HintMode};

/// Every printable ASCII cell regains actual font ink after its pack page is reused.
#[test]
fn takeover_eviction_rebuilds_every_cached_ascii_cell() {
    for dpr in [1.0, 2.0] {
        let pack = FontPack::decode(&fixture("fontpacks/jetbrains-regular.pzfp")).unwrap();
        let mut context = TextContext::from_pack(pack, 14.0, dpr).unwrap();
        context.backend =
            SlugBackend::from_pack_with_budget(context.pack.as_ref().unwrap(), 10 * 1024 * 1024)
                .unwrap();
        let mut builder = FrameBuilder::new(&context);
        let mut vt = Terminal::new(192, 2, 0, 0);
        let ascii: String = ('!'..='~').flat_map(|ch| [ch, ' ']).collect();
        vt.advance(ascii.as_bytes());
        let mut frame = RenderFrame::default();
        builder.build(&mut context, &vt, &mut frame).unwrap();
        let old = packed_handle(&context, '!');
        // The pack was drawn in frame one, then unused in frame two. Pins
        // protect that idle page until this validated full-font takeover.
        context.begin_frame().unwrap();
        let requests = takeover(&mut context);
        let (key, path) = fill_pages(&mut context);
        // Frame three has no glyph draws; frame four may reclaim frame-two filler.
        context.begin_frame().unwrap();
        // The same rejected insertion requests the next eligible boundary.
        assert!(
            context
                .backend
                .atlas_mut()
                .prepare(key, &path, FillRule::NonZero, 3)
                .is_err()
        );
        let epoch = context.atlas_generation();
        // This build's boundary clears the unused former pack page, before
        // any cached row captures addresses. Its unchanged VT row must rebuild.
        builder.build(&mut context, &vt, &mut frame).unwrap();
        assert_ne!(context.atlas_generation(), epoch);
        assert!(!context.backend.atlas().contains_handle(old));
        assert_eq!(context.backend.atlas().evictions(), 1);
        assert_ascii_ink(&context, &frame, &ascii);
        assert_eq!(
            requests.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "pack recovery scheduled font preparation"
        );
        // A subsequent unchanged-row draw must retain those rebuilt addresses.
        let addresses: Vec<_> = frame.slugs.iter().map(|s| s.glyph_loc).collect();
        builder.build(&mut context, &vt, &mut frame).unwrap();
        assert_eq!(
            frame.slugs.iter().map(|s| s.glyph_loc).collect::<Vec<_>>(),
            addresses
        );
        assert_ascii_ink(&context, &frame, &ascii);
    }
}

/// Installs validated full fonts while refusing every subsequent cold font request.
fn takeover(context: &mut TextContext) -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
    context
        .load_fonts(
            FontData::new(fixture("fonts/JetBrainsMono-Variable.ttf"), 0),
            None,
            None,
        )
        .unwrap();
    context
        .set_cluster_sources(Box::new(Pixels(50)), Box::new(Pixels(50)))
        .unwrap();
    let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    context
        .set_preparation_driver(Box::new(super::deferred::Decline(requests.clone())))
        .unwrap();
    requests
}

/// Finds the original unhinted glyph, independent of the eventual DPR policy.
fn packed_handle(context: &TextContext, ch: char) -> puzzle_slug::ShapeHandle {
    context
        .backend
        .get(GlyphKey {
            face: 0,
            glyph_id: context.pack.as_ref().unwrap().glyphs[&(ch as u32)].glyph_id,
            instance: 0,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        })
        .unwrap()
}

/// Filler requests pressure; actual page clearing remains in the build boundary.
fn fill_pages(context: &mut TextContext) -> (ShapeKey, kurbo::BezPath) {
    let mut path = puzzle_slug::synthetic::rectangle(0.0, 0.0, 1.0, 1.0);
    for i in 0..128 {
        let angle = i as f64 * std::f64::consts::TAU / 128.0;
        if i == 0 {
            path.move_to((angle.cos(), angle.sin()));
        } else {
            path.line_to((angle.cos(), angle.sin()));
        }
    }
    path.close_path();
    let mut id = 10_000;
    while context
        .backend
        .atlas_mut()
        .prepare(ShapeKey(id), &path, FillRule::NonZero, 2)
        .is_ok()
    {
        id += 1;
    }
    assert!(context.backend.atlas().needs_eviction());
    (ShapeKey(id), path)
}

/// Samples each emitted ASCII instance from the same texels the GPU consumes.
fn assert_ascii_ink(context: &TextContext, frame: &RenderFrame, ascii: &str) {
    assert_eq!(
        frame.slugs.len(),
        ascii.len() / 2,
        "missing ASCII instances"
    );
    for (draw, ch) in frame
        .slugs
        .iter()
        .zip(ascii.chars().filter(|ch| *ch != ' '))
    {
        let handle = packed_handle(context, ch);
        assert_eq!(
            draw.glyph_loc,
            puzzle_slug::encoding::encode_glyph_loc(handle.glyph_loc)
        );
        let image = puzzle_slug::reference::rasterize(
            context.backend.atlas(),
            handle,
            context.metrics.width_px,
            context.metrics.height_px,
            RasterTransform {
                scale: [draw.m[0], draw.m[3]],
                translate: [
                    draw.t[0]
                        - (ch as u32 - '!' as u32) as f32 * 2.0 * context.metrics.width_px as f32
                        - 16.0,
                    draw.t[1] - 16.0,
                ],
            },
        )
        .unwrap();
        assert!(
            image.pixels.chunks_exact(4).filter(|p| p[0] > 8).count() >= 3,
            "ASCII {ch} has no ink at DPR {}",
            context.dpr
        );
    }
}
