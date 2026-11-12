//! Page-budget witnesses protect stable frames, stale cache generations and CPU restore.
#![allow(
    clippy::expect_used,
    reason = "valid fixture bitmaps must pack for the budget witnesses"
)]
use super::*;
/// Source identity varies the complete request ID, without a platform fixture.
pub(super) fn key(id: u32) -> RasterKey {
    RasterKey {
        fit: [0; 3],
        glyph: GlyphKey {
            face: 0,
            glyph_id: id,
            instance: 0,
            hint_ppem: 0,
            hint_mode: crate::glyph::HintMode::None,
        },
        size: 24_f32.to_bits(),
        dpr: 2_f32.to_bits(),
    }
}
/// One bitmap fills a complete shelf page while retaining its transparent border.
pub(super) fn full(value: u8) -> RasterGlyph {
    RasterGlyph {
        offset_px: [0, -20],
        width: PAGE - 2,
        height: PAGE - 2,
        pixels: vec![value; ((PAGE - 2) * (PAGE - 2)) as usize],
        kind: BitmapKind::Mask,
    }
}
/// Insertion must not invalidate handles already assembled in the current frame.
#[test]
fn eviction_generation_and_cpu_restore_contract() {
    let mut atlas = RasterAtlas::new(1, 1);
    atlas.begin_frame();
    let first = atlas
        .prepare(key(1), full(91))
        .expect("valid bounded bitmap");
    assert!(atlas.prepare(key(2), full(123)).is_err());
    assert_eq!(atlas.generation(), first.generation);
    assert!(atlas.get(key(1)).is_some());
    atlas.take_dirty();
    assert!(atlas.begin_frame());
    assert!(atlas.generation() > first.generation);
    assert!(atlas.get(key(1)).is_none());
    let next = atlas
        .prepare(key(2), full(123))
        .expect("valid bounded bitmap");
    assert_eq!(next.page, first.page);
    atlas.take_dirty();
    atlas.restore();
    let restored = atlas.take_dirty();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].pixels.len(), (PAGE * PAGE) as usize);
    assert_eq!(restored[0].pixels[(PAGE + 1) as usize], 123);
    assert_eq!(restored[0].pixels[0], 0);
    assert_eq!(atlas.allocated_bytes(), 4 * 1024 * 1024);
    let measurement = allocation_counter::measure(|| {
        for _ in 0..100 {
            assert!(atlas.get(key(2)).is_some());
        }
    });
    assert_eq!(measurement.count_total, 0);
}
/// Cached-row usage, rather than insertion order, decides the page victim.
#[test]
fn least_recently_drawn_page_is_evicted() {
    let mut atlas = RasterAtlas::new(2, 1);
    atlas.begin_frame();
    let first = atlas
        .prepare(key(1), full(11))
        .expect("valid bounded bitmap");
    atlas.begin_frame();
    atlas
        .prepare(key(2), full(22))
        .expect("valid bounded bitmap");
    atlas.begin_frame();
    atlas.mark_used(BitmapKind::Mask, first.page);
    assert!(atlas.prepare(key(3), full(33)).is_err());
    assert!(atlas.begin_frame());
    assert!(atlas.get(key(1)).is_some());
    assert!(atlas.get(key(2)).is_none());
    atlas
        .prepare(key(3), full(33))
        .expect("valid bounded bitmap");
    assert_eq!(atlas.allocated_bytes(), 8 * 1024 * 1024);
}
/// Hostile dimensions and mismatched bytes cannot reach shelf arithmetic.
#[test]
fn invalid_bitmap_inputs_return_reports() {
    let mut atlas = RasterAtlas::default();
    for (width, height, pixels, kind) in [
        (u32::MAX, 1, vec![], BitmapKind::Mask),
        (1, u32::MAX, vec![], BitmapKind::Mask),
        (2, 2, vec![0], BitmapKind::Mask),
        (1, 1, vec![0; 3], BitmapKind::Subpixel),
    ] {
        assert!(
            atlas
                .prepare(
                    key(1),
                    RasterGlyph {
                        offset_px: [0; 2],
                        width,
                        height,
                        pixels,
                        kind
                    }
                )
                .is_err()
        );
    }
    assert_eq!(atlas.allocated_bytes(), 0);
}

/// Native page pressure must resolve within insertion while a current survivor keeps its ink.
#[test]
fn synchronous_eviction_preserves_current_page_and_ordered_restore() {
    let mut atlas = RasterAtlas::new(2, 1);
    atlas.set_synchronous();
    atlas.begin_frame();
    let survivor = atlas.prepare(key(1), full(11)).expect("first page");
    atlas.prepare(key(2), full(22)).expect("cold second page");
    atlas.take_dirty();
    atlas.begin_frame();
    atlas.mark_used(BitmapKind::Mask, survivor.page);
    let replacement = atlas.prepare(key(3), full(33)).expect("evict within build");
    assert_eq!(replacement.page, 1);
    assert_eq!(atlas.get(key(1)).expect("survivor").rect.x, survivor.rect.x);
    assert!(atlas.get(key(2)).is_none());
    assert!(!atlas.needs_eviction());
    let writes = atlas.take_dirty();
    assert_eq!(writes.len(), 2);
    assert!(writes[0].pixels.iter().all(|pixel| *pixel == 0));
    assert!(writes[1].pixels.iter().all(|pixel| *pixel == 33));
    atlas.restore();
    let pages = atlas.take_dirty();
    assert_eq!(pages[0].pixels[(PAGE + 1) as usize], 11);
    assert_eq!(pages[1].pixels[(PAGE + 1) as usize], 33);
    assert_eq!(atlas.allocated_bytes(), 8 * 1024 * 1024);
}

/// A complete native build may need more glyph ink than one physical page can hold.
#[test]
fn synchronous_full_frame_retains_all_incarnations_without_capacity_failure() {
    let mut atlas = RasterAtlas::new(1, 1);
    atlas.set_synchronous();
    atlas.begin_frame();
    let mut epochs = Vec::new();
    for id in 1..=5 {
        let handle = atlas
            .prepare(key(id), full(id as u8 * 40))
            .expect("flush within build");
        epochs.push(handle.epoch);
    }
    for id in 1..=5 {
        assert_eq!(
            atlas.get(key(id)).expect("earlier ink retained").epoch,
            epochs[id as usize - 1]
        );
    }
    let versions = atlas.take_versions();
    assert_eq!(versions.len(), 5);
    for (index, version) in versions.iter().enumerate() {
        assert_eq!(version.pixels[(PAGE + 1) as usize], (index as u8 + 1) * 40);
    }
    assert!(!atlas.needs_eviction());
    assert_eq!(atlas.allocated_bytes(), 4 * 1024 * 1024);
    atlas.begin_frame();
    assert!(
        atlas.get(key(1)).is_none(),
        "retired keys must not escape this frame"
    );
    assert!(atlas.get(key(5)).is_some());
}

/// CPU vector overflow is regenerable; only image-only neighbors restore from mirrors.
#[test]
fn outline_overflow_has_no_mirror_and_loss_preserves_authored_neighbors() {
    let mut atlas = RasterAtlas::new(1, 1);
    atlas.set_synchronous();
    atlas.begin_frame();
    atlas
        .prepare_outline_mask(key(1), full(91))
        .expect("CPU overflow");
    assert_eq!(atlas.cpu_mirror_bytes(), 0);
    let writes = atlas.take_dirty();
    assert_eq!(writes.len(), 1);
    assert!(writes[0].pixels.iter().all(|pixel| *pixel == 91));
    atlas.restore();
    assert!(
        atlas.get(key(1)).is_none(),
        "outline must regenerate after loss"
    );
    assert!(
        atlas.take_dirty().is_empty(),
        "no generated mirror may upload"
    );
    atlas
        .prepare_outline_mask(key(1), full(91))
        .expect("regenerated overflow");
    assert_eq!(atlas.cpu_mirror_bytes(), 0);

    // A mixed page restores authored pixels but never the disposable coverage beside them.
    let mut mixed = RasterAtlas::new(1, 1);
    mixed.set_synchronous();
    mixed.begin_frame();
    let small = |value| RasterGlyph {
        offset_px: [0; 2],
        width: 1,
        height: 1,
        pixels: vec![value],
        kind: BitmapKind::Mask,
    };
    mixed
        .prepare_outline_mask(key(2), small(91))
        .expect("generated crop");
    let authored = mixed.prepare(key(3), small(127)).expect("image-only crop");
    mixed.take_dirty();
    mixed.restore();
    assert!(mixed.get(key(2)).is_none());
    assert_eq!(
        mixed.get(key(3)).expect("authored survivor").rect,
        authored.rect
    );
    let writes = mixed.take_dirty();
    assert_eq!(writes.len(), 1);
    assert_eq!(
        writes[0].pixels.iter().filter(|pixel| **pixel != 0).count(),
        1
    );
    assert_eq!(
        writes[0].pixels[(authored.rect.y * PAGE + authored.rect.x) as usize],
        127
    );
}

/// Nearest-only grid masks fill their byte budget instead of reserving unusable borders.
#[test]
fn grid_crops_use_full_page_area_without_eviction() {
    let mut atlas = RasterAtlas::new(1, 1);
    atlas.set_synchronous();
    atlas.begin_frame();
    // 1024 exact 64-square crops fill 2048² bytes. A two-texel border
    // would evict the first page even though its ink fits in the budget.
    for id in 0..1024 {
        let glyph = RasterGlyph {
            offset_px: [0; 2],
            width: 64,
            height: 64,
            pixels: vec![255; 64 * 64],
            kind: BitmapKind::Mask,
        };
        atlas
            .prepare_grid_mask(key(id), glyph)
            .expect("grid crop fits");
    }
    assert_eq!(atlas.evictions(), 0);
    assert_eq!(atlas.entry_count(), 1024);
    assert_eq!(atlas.allocated_bytes(), (PAGE * PAGE) as usize);
    assert!(atlas.get(key(0)).is_some());
    // A transformed request must prepare its vector instead of sampling adjacent ink.
    assert!(atlas.get_for_placement(key(0), true).is_none());
}

/// Earlier draw batches still answer lookups even when all live entries are gone.
#[test]
fn retired_ink_keeps_dispatch_populated_until_frame_end() {
    let mut atlas = RasterAtlas::new(1, 1);
    atlas.set_synchronous();
    atlas.begin_frame();
    assert!(atlas.is_empty());
    atlas.prepare(key(1), full(41)).expect("first batch");
    atlas.prepare(key(2), full(82)).expect("reuse page");
    // Model a cold-page removal with only this frame's older ink still retained.
    atlas.entries.clear();
    assert!(!atlas.is_empty());
    assert!(atlas.get_for_placement(key(1), false).is_some());
    atlas.begin_frame();
    assert!(atlas.is_empty());
}
