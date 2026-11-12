//! GPU pressure regression for all four validated startup packs, without TTFs.
use super::*;
use puzzle_term::text::{
    glyph::{GlyphKey, HintMode, slug::SlugBackend},
    pack::FontPack,
};

/// Unused pack ink survives a forced page eviction while the full font is absent.
#[test]
fn every_pack_glyph_draws_after_pre_font_page_eviction() {
    let _gpu = crate::gpu_test::lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).expect("native pack GPU fixture");
    let mut executor = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    for style in ["regular", "bold", "italic", "bold-italic"] {
        let path = format!(
            "{}/../../assets/fontpacks/jetbrains-{style}.pzfp",
            env!("CARGO_MANIFEST_DIR")
        );
        let pack =
            FontPack::decode(&std::fs::read(path).expect("approved pack")).expect("validated pack");
        let mut backend =
            SlugBackend::from_pack_with_budget(&pack, 10 * 1024 * 1024).expect("tiny atlas");
        let mut before_frame = specimen(&backend, &pack);
        uploads(backend.atlas_mut(), &mut before_frame);
        let (before, _) = executor
            .render_to_image(&renderer, &mut before_frame)
            .expect("initial pack ink");
        // No pack usage stamp is issued: even glyphs never drawn by a terminal
        // must remain recoverable while pack bytes are their only ink source.
        require_pre_font_eviction(&mut backend);
        let mut after_frame = specimen(&backend, &pack);
        uploads(backend.atlas_mut(), &mut after_frame);
        let (after, _) = executor
            .render_to_image(&renderer, &mut after_frame)
            .expect("retained pack ink");
        assert!(
            before.pixels == after.pixels,
            "{style}: packed pixels changed"
        );
        require_ink(&after, after_frame.slugs.len(), style);
        // Full-font takeover makes unused packs ordinary evictable shapes.
        require_takeover_eviction(&mut backend, &pack, style);
        // After takeover, validated pack bytes remain the source for cleared ink.
        // A deliberately unavailable font proves no cold outline work is needed.
        backend.begin_color_frame().expect("recovery draw boundary");
        recover(&mut backend, &pack);
        verify_members(&backend, &pack);
        let mut recovered_frame = specimen(&backend, &pack);
        uploads(backend.atlas_mut(), &mut recovered_frame);
        let (recovered, _) = executor
            .render_to_image(&renderer, &mut recovered_frame)
            .expect("recovered pack GPU ink");
        evidence(style, &before, &recovered);
        let difference = before
            .pixels
            .iter()
            .zip(&recovered.pixels)
            .enumerate()
            .find(|(_, (a, b))| a != b);
        assert!(
            difference.is_none(),
            "{style}: eviction recovery changed pack ink: {difference:?}"
        );
        require_ink(&recovered, recovered_frame.slugs.len(), style);
    }
}

/// Pressure must clear filler while unused startup pack glyphs remain pinned.
fn require_pre_font_eviction(backend: &mut SlugBackend) {
    exhaust(backend, 1);
    let generation = backend.atlas().generation();
    assert!(
        backend
            .atlas_mut()
            .begin_frame(3)
            .expect("pre-font page eviction")
    );
    assert_ne!(backend.atlas().generation(), generation);
    assert_eq!(backend.atlas().evictions(), 1);
}

/// Every drawable glyph is restored while admission refuses all real font work.
fn recover(backend: &mut SlugBackend, pack: &FontPack) {
    use puzzle_term::text::glyph::{GlyphBackend, GlyphPlacement, PreparedGlyph};
    for glyph in pack.glyphs.values().filter(|g| g.handle.is_some()) {
        let p = GlyphPlacement {
            key: GlyphKey {
                face: 0,
                glyph_id: glyph.glyph_id,
                instance: 0,
                hint_ppem: 14,
                hint_mode: HintMode::Light,
            },
            cell_column: 0,
            origin_px: [0, 28],
            baseline_px: 28.0,
            size_px: 28.0,
            advance_px: 40,
            ink_limit_px: 40,
            cell_height_px: 40,
            cell_span: 1,
            color: 0xffffffff,
            effect: false,
            color_glyph: false,
        };
        assert!(matches!(
            backend
                .prepare(&p, &mut Unavailable)
                .expect("pack source recovery"),
            PreparedGlyph::Vector(_)
        ));
    }
}

/// Font extraction is forbidden for a key that the validated pack covers.
struct Unavailable;
/// A source that rejects any accidental font extraction in pack recovery.
impl puzzle_term::text::glyph::GlyphOutlineSource for Unavailable {
    /// Attempting a cold font outline fails this witness immediately.
    fn outline(&mut self, _: GlyphKey) -> Result<puzzle_term::text::geometry::BezPath, String> {
        panic!("pack recovery attempted font preparation")
    }
}

/// Removing startup pins returns the same pack shapes to ordinary page LRU.
fn require_takeover_eviction(backend: &mut SlugBackend, pack: &FontPack, style: &str) {
    backend.release_pack_pins();
    exhaust(backend, 4);
    assert!(
        backend
            .atlas_mut()
            .begin_frame(5)
            .expect("post-takeover LRU")
    );
    assert!(
        pack.glyphs.values().any(|glyph| glyph.handle.is_some()
            && backend
                .get(GlyphKey {
                    face: 0,
                    glyph_id: glyph.glyph_id,
                    instance: 0,
                    hint_ppem: 0,
                    hint_mode: HintMode::None
                })
                .is_none()),
        "{style}: released pack ink must participate in LRU eviction"
    );
}

/// Every drawable cmap scalar must have ink independently of whole-image equality.
fn require_ink(image: &puzzle_render::RgbaImage, count: usize, style: &str) {
    for i in 0..count {
        let (x, y) = ((i % 24) as u32 * 40, (i / 24) as u32 * 40);
        let ink = (y..y + 40).any(|row| {
            (x..x + 40).any(|col| image.pixels[((row * image.width + col) * 4) as usize] != 0)
        });
        assert!(ink, "{style}: pack glyph {i} has no GPU ink");
    }
}

/// Draws every cmap entry with a contour in its own isolated tile.
fn specimen(backend: &SlugBackend, pack: &FontPack) -> RenderFrame {
    let count = pack
        .glyphs
        .values()
        .filter(|glyph| glyph.handle.is_some())
        .count();
    let mut frame = RenderFrame {
        target_size: [960, count.div_ceil(24) as u32 * 40],
        ..RenderFrame::default()
    };
    for glyph in pack.glyphs.values().filter(|glyph| glyph.handle.is_some()) {
        let key = GlyphKey {
            face: 0,
            glyph_id: glyph.glyph_id,
            instance: 0,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        };
        let handle = backend
            .get(key)
            .expect("every unused pack key remains available");
        let bounds = handle.bbox;
        let scale = (28.0 / (bounds[2] - bounds[0])).min(28.0 / (bounds[3] - bounds[1]));
        let i = frame.slugs.len();
        frame.slugs.push(SlugInstance {
            m: [scale, 0.0, 0.0, -scale],
            t: [
                (i % 24) as f32 * 40.0 + 6.0 - bounds[0] * scale,
                (i / 24) as f32 * 40.0 + 6.0 + bounds[3] * scale,
            ],
            bbox: bounds,
            glyph_loc: puzzle_term::slug::encoding::encode_glyph_loc(handle.glyph_loc),
            band_info: handle.band_info,
            color: 0xffffffff,
            paint: 0,
        });
    }
    frame.batches.push(Batch {
        slot: ZSlot::Text,
        pipeline: PipelineKind::Slug,
        instances: 0..frame.slugs.len() as u32,
        scissor: None,
    });
    frame
}

/// Dense independent filler consumes the small budget without font loading.
fn exhaust(backend: &mut SlugBackend, frame: u64) {
    let mut path = puzzle_term::slug::synthetic::rectangle(0.0, 0.0, 1.0, 1.0);
    for i in 0..128 {
        let angle = i as f64 * std::f64::consts::TAU / 128.0;
        if i == 0 {
            path.move_to((angle.cos(), angle.sin()));
        } else {
            path.line_to((angle.cos(), angle.sin()));
        }
    }
    path.close_path();
    let mut key = 10_000 * frame;
    while backend
        .atlas_mut()
        .prepare(ShapeKey(key), &path, FillRule::NonZero, frame)
        .is_ok()
    {
        key += 1;
    }
    assert!(backend.atlas().needs_eviction());
    assert!(backend.atlas().allocated_bytes() <= 10 * 1024 * 1024);
}

/// Recovery preserves the exact directed curve sequence of every sorted band.
fn verify_members(backend: &SlugBackend, pack: &FontPack) {
    for glyph in pack.glyphs.values().filter(|g| g.handle.is_some()) {
        let old = glyph.handle.expect("drawable pack entry");
        let new = backend
            .get(GlyphKey {
                face: 0,
                glyph_id: glyph.glyph_id,
                instance: 0,
                hint_ppem: 0,
                hint_mode: HintMode::None,
            })
            .expect("recovered pack handle");
        assert_eq!(old.band_info, new.band_info);
        let (x, y, _) = puzzle_term::slug::encoding::decode_band_info(old.band_info);
        let n = usize::from(x) + usize::from(y) + 2;
        let old_base = old.glyph_loc[1] as usize * 4096 + old.glyph_loc[0] as usize;
        let new_base = new.glyph_loc[1] as usize * 4096 + new.glyph_loc[0] as usize;
        for band in 0..n {
            let a = pack.bands[old_base + band];
            let b = backend.atlas().band_texels()[new_base + band];
            assert_eq!(a[0], b[0]);
            for i in 0..a[0] as usize {
                let aa = pack.bands[old_base + a[1] as usize + i];
                let bb = backend.atlas().band_texels()[new_base + b[1] as usize + i];
                let ca = aa[1] as usize * 4096 + aa[0] as usize;
                let cb = bb[1] as usize * 4096 + bb[0] as usize;
                assert_eq!(
                    pack.curves[ca],
                    backend.atlas().curve_texels()[cb],
                    "glyph {} band {band} curve {i} first",
                    glyph.glyph_id
                );
                assert_eq!(
                    &pack.curves[ca + 1][..2],
                    &backend.atlas().curve_texels()[cb + 1][..2],
                    "glyph {} band {band} curve {i} end",
                    glyph.glyph_id
                );
            }
        }
    }
}

/// Keep viewed GPU recovery specimens outside tracked source and reference assets.
fn evidence(style: &str, before: &puzzle_render::RgbaImage, after: &puzzle_render::RgbaImage) {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pack-repair");
    std::fs::create_dir_all(&out).expect("pack evidence directory");
    crate::png::write(&out.join(format!("{style}-before.png")), before)
        .expect("original pack screenshot");
    crate::png::write(&out.join(format!("{style}-after.png")), after)
        .expect("recovered pack screenshot");
}
