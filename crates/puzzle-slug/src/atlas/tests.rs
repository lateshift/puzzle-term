//! Packing properties for C1, B1 and B2, including row-end starts.
// Test setup assertions identify the shape that could not be packed.
#![allow(clippy::expect_used, reason = "finite generated geometry should pack")]
use super::{AtlasError, FillRule, ShapeKey, SlugAtlas, TEXTURE_WIDTH};
use crate::{bands::BandSet, synthetic::rectangle};
use kurbo::BezPath;
use proptest::prelude::*;

/// Reads all headers and verifies both shader non-wrapping assumptions.
fn check_headers(atlas: &SlugAtlas, handle: super::ShapeHandle) {
    let (x, y, _) = crate::encoding::decode_band_info(handle.band_info);
    let h = usize::from(y) + 1;
    let v = usize::from(x) + 1;
    let glyph_x = usize::from(handle.glyph_loc[0]);
    let glyph_y = usize::from(handle.glyph_loc[1]);
    assert!(glyph_x + h + v <= TEXTURE_WIDTH); // B1
    for index in 0..h + v {
        let header = atlas.band_texels()[glyph_y * TEXTURE_WIDTH + glyph_x + index];
        let linear = glyph_y * TEXTURE_WIDTH + glyph_x + usize::from(header[1]);
        let base_x = linear % TEXTURE_WIDTH;
        assert!(base_x + usize::from(header[0]) <= TEXTURE_WIDTH);
        assert!(usize::from(header[1]) + usize::from(header[0]) <= u16::MAX as usize); // B2
        for i in 0..usize::from(header[0]) {
            let location = atlas.band_texels()[linear + i];
            let x = usize::from(location[0]);
            let y = usize::from(location[1]);
            assert!(x + 1 < TEXTURE_WIDTH); // C1
            assert!(y * TEXTURE_WIDTH + x + 1 < atlas.curve_texels().len());
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// Random subpixel rectangles beginning near row ends preserve all rules.
    #[test]
    fn random_packing_obeys_c1_b1_b2(
        x0 in -1.0_f64..0.0, y0 in -1.0_f64..0.0,
        width in 0.01_f64..2.0, height in 0.01_f64..2.0,
        curve_column in 4090_usize..4096, band_column in 4090_usize..4096,
    ) {
        let path = rectangle(x0, y0, x0 + width, y0 + height);
        let mut atlas = SlugAtlas::default();
        atlas.curves.cursor = curve_column;
        atlas.bands.cursor = band_column;
        atlas.pages[0].curve_end = curve_column;
        atlas.pages[0].band_end = band_column;
        let handle = atlas.prepare(ShapeKey(1), &path, FillRule::NonZero, 0).expect("small shape");
        check_headers(&atlas, handle);
        prop_assert!(atlas.contains_handle(handle));
    }

    /// Curved random contours stress many shared endpoints across a row edge.
    #[test]
    fn random_quadratic_packing_preserves_shader_addresses(
        segments in prop::collection::vec(
            ((-1.0_f64..1.0, -1.0_f64..1.0), (-1.0_f64..1.0, -1.0_f64..1.0)),
            2..12,
        ),
        start in 4088_usize..4096,
    ) {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        for (control, end) in segments {
            path.quad_to(control, end);
        }
        path.close_path();
        let mut atlas = SlugAtlas::default();
        atlas.curves.cursor = start;
        atlas.bands.cursor = start;
        atlas.pages[0].curve_end = start;
        atlas.pages[0].band_end = start;
        let handle = atlas.prepare(ShapeKey(1), &path, FillRule::NonZero, 0)
            .expect("finite quadratic contour");
        check_headers(&atlas, handle);
        prop_assert!(atlas.contains_handle(handle));
    }
}

/// Equal band lists reuse the same offset instead of duplicating texels.
#[test]
fn identical_lists_share_storage() {
    let mut atlas = SlugAtlas::default();
    let lists = BandSet {
        horizontal: vec![vec![0], vec![0]],
        vertical: vec![vec![0], vec![0]],
    };
    let origin = atlas.pack_bands(&lists, &[[0, 0]]).expect("small lists");
    let index = usize::from(origin[1]) * TEXTURE_WIDTH + usize::from(origin[0]);
    let offset = atlas.band_texels()[index][1];
    assert!(
        atlas.band_texels()[index..index + 4]
            .iter()
            .all(|header| header[1] == offset)
    );
}

/// Header writes occur after list writes and can extend a dirty span left.
#[test]
fn dirty_band_span_includes_headers_written_after_lists() {
    let mut atlas = SlugAtlas::default();
    let path = rectangle(0.0, 0.0, 1.0, 1.0);
    let handle = atlas
        .prepare(ShapeKey(1), &path, FillRule::NonZero, 0)
        .expect("rectangle");
    let (_, dirty) = atlas.take_dirty();
    let header = handle.glyph_loc;
    // The first band's count controls every later texture load. Uploading
    // only the list makes a healthy shape appear entirely blank on GPU.
    assert!(
        dirty.iter().any(|span| {
            span.row == header[1] && span.start <= header[0] && span.end > header[0]
        })
    );
}

/// A list longer than one shader row is rejected before producing a handle.
#[test]
fn oversized_list_needs_chunking() {
    let mut atlas = SlugAtlas::default();
    let lists = BandSet {
        horizontal: vec![vec![0; TEXTURE_WIDTH + 1]],
        vertical: vec![vec![]],
    };
    assert_eq!(
        atlas.pack_bands(&lists, &[[0, 0]]),
        Err(AtlasError::BandBlockTooLarge)
    );
}

/// A whole-context reset invalidates handles even when addresses are reused.
#[test]
fn reset_invalidates_old_handle() {
    let mut atlas = SlugAtlas::default();
    let path = rectangle(0.0, 0.0, 1.0, 1.0);
    let old = atlas
        .prepare(ShapeKey(1), &path, FillRule::NonZero, 1)
        .expect("first");
    atlas.reset().expect("reset");
    atlas
        .prepare(ShapeKey(1), &path, FillRule::NonZero, 2)
        .expect("replacement");
    assert!(!atlas.contains_handle(old));
    assert!(atlas.contains_handle(atlas.get(ShapeKey(1)).expect("retained")));
}

/// Clearing a cold page must never relocate a glyph in another page (D-39).
#[test]
fn pressure_evicts_lru_without_moving_current_ink() {
    let mut atlas = SlugAtlas::with_budget(10 * 1024 * 1024).expect("two pages");
    let path = rectangle(0.0, 0.0, 1.0, 1.0);
    let mut count = 0;
    while atlas
        .prepare(ShapeKey(count), &path, FillRule::NonZero, 1)
        .is_ok()
    {
        count += 1;
    }
    // The last page belongs to a drawn row, while the first page is cold.
    let live = ShapeKey(count - 1);
    let cleared = atlas.get(ShapeKey(0)).expect("first page ink");
    let old = atlas.get(live).expect("last successful insertion");
    atlas.take_dirty();
    atlas.mark_used(live, 2);
    assert!(atlas.begin_frame(3).expect("between-frame eviction"));
    let retained = atlas.get(live).expect("current frame is pinned");
    assert_eq!(old, retained, "LRU eviction relocated a different page");
    assert!(atlas.contains_handle(old));
    assert!(
        !atlas.contains_handle(cleared),
        "cleared page kept its old generation"
    );
    let (curves, bands) = atlas.take_dirty();
    assert!(
        curves.is_empty() && bands.is_empty(),
        "eviction uploaded unchanged texels"
    );
    assert!(
        atlas.get(ShapeKey(0)).is_none(),
        "fixed-window retention kept old ink"
    );
    // The exact same handle must still sample the original curve addresses.
    let ink = crate::reference::sample(&atlas, retained, [0.5, 0.5], [0.1, 0.1])
        .expect("unchanged band/curve addresses");
    assert_eq!(ink, 1.0);
    assert!(
        atlas
            .prepare(ShapeKey(count), &path, FillRule::NonZero, 3)
            .is_ok()
    );
}

/// An unchanged row's stamp protects its entire page without enumerating instances.
#[test]
fn cached_batch_is_stamped_without_visiting_instances() {
    let mut atlas = SlugAtlas::with_budget(10 * 1024 * 1024).expect("two pressure pages");
    let path = rectangle(0.0, 0.0, 1.0, 1.0);
    let mut count = 0;
    while atlas
        .prepare(ShapeKey(count), &path, FillRule::NonZero, 1)
        .is_ok()
    {
        count += 1;
    }
    let handle = atlas
        .get(ShapeKey(count - 1))
        .expect("finite pressure fixture");
    let location = crate::encoding::encode_glyph_loc(handle.glyph_loc);
    let mut token = 0;
    atlas.use_batch(&mut token, std::iter::once(location), 2);
    // Iteration on a warm row would turn the promised O(rows) operation into O(instances).
    atlas.use_batch(
        &mut token,
        std::iter::from_fn(|| panic!("warm batch enumerated instances")),
        602,
    );
    assert!(atlas.begin_frame(603).expect("finite pressure fixture"));
    let retained = atlas
        .get(ShapeKey(count - 1))
        .expect("finite pressure fixture");
    assert_eq!(retained, handle);
    assert!(atlas.contains_handle(handle));
    assert!(atlas.get(ShapeKey(0)).is_none());
}
/// Imported planes must upload their interiors, not just two endpoint texels.
#[test]
fn imported_planes_upload_every_occupied_texel() {
    // A full row and partial final row exercise the frame upload's row-local contract.
    let curves = vec![[1.0; 4]; TEXTURE_WIDTH + 17];
    let bands = vec![[1; 2]; TEXTURE_WIDTH * 2 + 3];
    let mut atlas = SlugAtlas::from_texels(&curves, &bands).expect("bounded imported planes");
    let (curve_spans, band_spans) = atlas.take_dirty();
    for (spans, len) in [(curve_spans, curves.len()), (band_spans, bands.len())] {
        let mut uploaded = vec![false; len];
        for span in spans {
            let row = usize::from(span.row) * TEXTURE_WIDTH;
            for texel in &mut uploaded[row + usize::from(span.start)..row + usize::from(span.end)] {
                *texel = true;
            }
        }
        assert!(
            uploaded.into_iter().all(|texel| texel),
            "pack texels omitted from upload"
        );
    }
}

/// Style packs imported through the ordinary allocator still acquire startup pins.
#[test]
fn registering_an_existing_shape_pins_its_page_until_takeover() {
    let mut atlas = SlugAtlas::with_budget(5 * 1024 * 1024).expect("one pack page");
    let path = rectangle(0.0, 0.0, 1.0, 1.0);
    let handle = atlas
        .prepare(ShapeKey(0), &path, FillRule::NonZero, 1)
        .expect("style ink");
    atlas
        .register_packed(ShapeKey(0), handle, 1)
        .expect("pin allocated pack ink");
    let mut count = 1;
    while atlas
        .prepare(ShapeKey(count), &path, FillRule::NonZero, 1)
        .is_ok()
    {
        count += 1;
    }
    // No row ever used the style. Only startup protection can keep its ink alive.
    assert!(!atlas.begin_frame(3).expect("pre-takeover boundary"));
    assert!(atlas.contains_handle(handle));
    atlas.release_pack_pins();
    assert!(
        atlas
            .prepare(ShapeKey(count), &path, FillRule::NonZero, 3)
            .is_err()
    );
    assert!(atlas.begin_frame(4).expect("post-takeover boundary"));
    assert!(!atlas.contains_handle(handle));
}
