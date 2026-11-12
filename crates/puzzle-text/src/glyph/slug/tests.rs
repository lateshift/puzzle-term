//! D-39 current-frame overflow retains real ink instead of a diagnostic glyph.
#![allow(clippy::expect_used, reason = "finite pressure fixtures must prepare")]
use super::*;

/// Startup-only ink must participate in recency even before any font outline is available.
#[test]
fn packed_glyph_survives_pressure_without_font_tables() -> Result<(), String> {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fontpacks/jetbrains-regular.pzfp"
    ))
    .map_err(|e| e.to_string())?;
    let pack = crate::pack::FontPack::decode(&bytes)?;
    let glyph = pack.glyphs.get(&('B' as u32)).ok_or("packed B absent")?;
    let mut backend = SlugBackend::from_pack(&pack)?;
    let key = GlyphKey {
        face: 0,
        glyph_id: glyph.glyph_id,
        instance: 0,
        hint_ppem: 0,
        hint_mode: super::super::HintMode::None,
    };
    let old = backend.get(key).ok_or("initial pack ink absent")?;
    backend.begin_color_frame()?;
    let mut token = 0;
    backend.use_row_batch(
        &mut token,
        std::iter::once((puzzle_slug::encoding::encode_glyph_loc(old.glyph_loc), 0)),
        false,
    );
    // No cold page exists in this pack-only atlas. Even unused startup ink
    // remains available instead of being lost while its font is loading.
    backend.atlas.begin_frame(2).map_err(|e| e.to_string())?;
    assert_eq!(backend.get(key), Some(old));
    assert!(backend.atlas.contains_handle(old));
    Ok(())
}

/// A finite outline source supplies known rectangle pixels independently of fonts.
struct Rectangle;
impl GlyphOutlineSource for Rectangle {
    /// This covered glyph occupies the unit em square.
    fn outline(&mut self, _key: GlyphKey) -> Result<kurbo::BezPath, String> {
        Ok(puzzle_slug::synthetic::rectangle(0.0, 0.0, 1.0, 1.0))
    }
}

/// Filling a single live frame never moves its addresses; new covered ink becomes a mask.
#[test]
fn current_frame_overflow_uses_oracle_ink() {
    let (mut backend, first) = full_frame();
    let p = GlyphPlacement {
        key: GlyphKey {
            face: 0,
            glyph_id: 1,
            instance: 0,
            hint_ppem: 0,
            hint_mode: super::super::HintMode::None,
        },
        cell_column: 0,
        origin_px: [17, 23],
        baseline_px: 12.0,
        size_px: 10.0,
        advance_px: 12,
        ink_limit_px: 12,
        cell_height_px: 14,
        cell_span: 1,
        color: 0xffffffff,
        effect: false,
        color_glyph: false,
    };
    let origin = p.origin_px;
    let PreparedGlyph::Rasterized(mask) = backend
        .prepare(&p, &mut Rectangle)
        .expect("finite pressure fixture")
    else {
        panic!("covered overflow must rasterize");
    };
    assert!(
        backend.atlas.contains_handle(first),
        "live insertion moved earlier draws"
    );
    assert_eq!(mask.kind, super::super::BitmapKind::Mask);
    assert!(mask.pixels.iter().filter(|&&v| v == 255).count() >= 64);
    assert_eq!(p.origin_px, origin);
    assert_eq!(p.advance_px, 12);
}

/// Vector overflow into a full, referenced raster page must flush rather than abort.
#[test]
fn vector_overflow_into_full_raster_retains_both_kinds_of_ink() {
    use crate::raster::{PAGE, RasterAtlas, RasterKey};
    let (mut backend, _) = full_frame();
    let mut raster = RasterAtlas::new(1, 1);
    raster.set_synchronous();
    raster.begin_frame();
    let p = overflow_placement();
    let key = RasterKey {
        glyph: p.key,
        size: p.size_px.to_bits(),
        fit: [12, 14, 12_f32.to_bits()],
        dpr: 1_f32.to_bits(),
    };
    let mut old_key = key;
    old_key.glyph.glyph_id = 0;
    raster
        .prepare(
            old_key,
            super::super::RasterGlyph {
                kind: super::super::BitmapKind::Mask,
                width: PAGE - 2,
                height: PAGE - 2,
                offset_px: [0; 2],
                pixels: vec![77; ((PAGE - 2) * (PAGE - 2)) as usize],
            },
        )
        .expect("referenced full raster page");
    let PreparedGlyph::Rasterized(mask) = backend
        .prepare(&p, &mut Rectangle)
        .expect("covered vector overflow")
    else {
        panic!("full vector atlas must supply oracle ink");
    };
    let handle = raster
        .prepare(key, mask)
        .expect("full raster atlas must flush");
    assert_eq!(handle.epoch, 1);
    let versions = raster.take_versions();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].pixels[(PAGE + 1) as usize], 77);
    let at = ((handle.rect.y + handle.rect.height / 2) * PAGE
        + handle.rect.x
        + handle.rect.width / 2) as usize;
    assert_eq!(
        versions[1].pixels[at], 255,
        "overflow outline's opaque interior"
    );
    assert!(
        raster.get(old_key).is_some(),
        "earlier glyph still drawable this frame"
    );
}

/// Fills both large and small slots, retaining a witness to an earlier draw.
fn full_frame() -> (SlugBackend, puzzle_slug::ShapeHandle) {
    let mut backend = SlugBackend {
        atlas: SlugAtlas::with_budget(5 * 1024 * 1024).expect("finite pressure fixture"),
        ..SlugBackend::default()
    };
    backend
        .begin_color_frame()
        .expect("finite pressure fixture");
    // A dense covered corpus reaches capacity quickly without making one giant band.
    let mut path = kurbo::BezPath::new();
    for i in 0..128 {
        let angle = i as f64 * std::f64::consts::TAU / 128.0;
        let point = (angle.cos(), angle.sin());
        if i == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close_path();
    let frame = backend.colors.frame;
    let first = backend
        .atlas
        .prepare(ShapeKey(10_000), &path, FillRule::NonZero, frame)
        .expect("finite pressure fixture");
    let mut id = 10_001;
    while backend
        .atlas
        .prepare(ShapeKey(id), &path, FillRule::NonZero, frame)
        .is_ok()
    {
        id += 1;
    }
    // A large path can fail while smaller ink still fits the final reserved rows.
    // Exhaust the rectangle-sized slots before asserting this rectangle overflows.
    let rectangle = puzzle_slug::synthetic::rectangle(0.0, 0.0, 1.0, 1.0);
    id += 1;
    while backend
        .atlas
        .prepare(ShapeKey(id), &rectangle, FillRule::NonZero, frame)
        .is_ok()
    {
        id += 1;
    }
    (backend, first)
}

/// Device geometry shared by the combined vector/raster pressure witness.
fn overflow_placement() -> GlyphPlacement {
    GlyphPlacement {
        key: GlyphKey {
            face: 0,
            glyph_id: 1,
            instance: 0,
            hint_ppem: 0,
            hint_mode: super::super::HintMode::None,
        },
        cell_column: 0,
        origin_px: [17, 23],
        baseline_px: 12.0,
        size_px: 10.0,
        advance_px: 12,
        ink_limit_px: 12,
        cell_height_px: 14,
        cell_span: 1,
        color: 0xffffffff,
        effect: false,
        color_glyph: false,
    }
}

/// D-47 reevaluates bytes at each size and never lets grid masks replace effect vectors.
#[test]
fn desktop_grid_reselects_smaller_storage_and_keeps_effect_vectors() {
    let mut backend = SlugBackend::default();
    backend.set_grid_renderer(super::super::RendererChoice::Auto);
    let mut p = overflow_placement();
    p.size_px = 400.0;
    let PreparedGlyph::Vector(large) = backend.prepare(&p, &mut Rectangle).expect("large vector")
    else {
        panic!("large crop must choose fewer vector bytes");
    };
    p.size_px = 4.0;
    assert!(backend.get_for_placement(&p, [false; 3]).is_none());
    assert!(matches!(
        backend.prepare(&p, &mut Rectangle).expect("small grid"),
        PreparedGlyph::GridOutline { .. }
    ));
    p.effect = true;
    let PreparedGlyph::Vector(effect) = backend.prepare(&p, &mut Rectangle).expect("effect vector")
    else {
        panic!("effects must use retained vectors");
    };
    assert_eq!(large, effect);
    p.effect = false;
    assert!(matches!(
        backend
            .prepare_fitted(&p, &mut Rectangle, false, true, false)
            .expect("synthetic shear"),
        PreparedGlyph::Vector(_)
    ));
    backend.set_grid_renderer(super::super::RendererChoice::Slug);
    assert!(matches!(
        backend.prepare(&p, &mut Rectangle).expect("explicit slug"),
        PreparedGlyph::Vector(_)
    ));
    // Disabling grid masks must retain the same small-size vector even though
    // its recorded bitmap would be cheaper. The descriptor cannot override policy.
    assert_eq!(backend.get_for_placement(&p, [false; 3]), Some(large));
}
