//! Live COLR cache: fixed palette solids and allocation-free foreground tint.
#![allow(
    clippy::unwrap_used,
    reason = "approved font graphs must prepare authored layers"
)]
use crate::{
    FontData,
    font_set::FontSet,
    glyph::{GlyphKey, GlyphPlacement, HintMode, slug::SlugBackend},
};
#[test]
fn palette_solids_and_warm_tint_contract() {
    let data = FontData::new(
        include_bytes!("../../../../../../assets/fonts/test/test_glyphs-glyf_colr_1.ttf").to_vec(),
        0,
    );
    let fonts = FontSet::new(data, None, None).unwrap();
    let mut backend = SlugBackend::default();
    let mut seen = [false; 2];
    // Search authored graphs for both kinds of solid, without assuming palette IDs.
    for id in 0..221 {
        let Ok(Some(graph)) = fonts.face(0).unwrap().color_layers(id, 0xffffffff) else {
            continue;
        };
        if !graph
            .layers
            .iter()
            .any(|l| matches!(l.paint.brush, puzzle_slug::paint::Brush::Solid(_)))
        {
            continue;
        }
        let mut placement = placement(id);
        let prepared = backend.prepare_color(&placement, &fonts).unwrap().unwrap();
        for (source, draw) in graph.layers.iter().zip(&prepared.layers) {
            if let puzzle_slug::paint::Brush::Solid(rgba) = source.paint.brush {
                seen[usize::from(source.paint.foreground)] = true;
                if source.paint.foreground {
                    assert!(draw.solid_color.is_none());
                    assert_eq!(draw.opacity, rgba[3]);
                } else {
                    assert!(draw.solid_color.is_some());
                }
            }
        }
        let addresses: Vec<_> = prepared.layers.iter().map(|l| l.paint.index).collect();
        backend.take_paint_uploads();
        placement.color = 0xff00ff00;
        let measure = allocation_counter::measure(|| {
            let hit = backend.prepare_color(&placement, &fonts).unwrap().unwrap();
            assert!(
                hit.layers
                    .iter()
                    .map(|l| l.paint.index)
                    .eq(addresses.iter().copied())
            );
        });
        assert_eq!(measure.count_total, 0);
        assert!(backend.take_paint_uploads().is_empty());
        if seen == [true; 2] {
            break;
        }
    }
    assert_eq!(seen, [true; 2]);
}

/// A two-cell placement fixes layout while the cache changes foreground ink.
fn placement(id: u32) -> GlyphPlacement {
    GlyphPlacement {
        cell_column: 0,
        key: GlyphKey {
            face: 0,
            glyph_id: id,
            instance: 0,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        origin_px: [0, 0],
        baseline_px: 16.0,
        size_px: 16.0,
        advance_px: 20,
        ink_limit_px: 20,
        cell_height_px: 20,
        cell_span: 2,
        color: 0xff0000ff,
        effect: false,
        color_glyph: true,
    }
}
