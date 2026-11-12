//! Integral cell dimensions and D-27 cap-height boundaries (design §8.2).
use super::{CellMetrics, CellOptions, MetricAdjustment, MetricInput};

/// JetBrains Mono's 0.73-em capitals cannot snap at 10 and 13 px within 3%.
#[test]
fn unsnappable_sizes_stay_unchanged() {
    let input = MetricInput {
        advance_em: 0.6,
        ascent_em: 0.8,
        descent_em: -0.2,
        gap_em: 0.0,
        cap_em: Some(0.73),
        underline_em: 0.05,
    };
    for size in [10.0, 13.0] {
        let options = CellOptions {
            size,
            dpr: 1.0,
            line_height: 1.2,
            width: MetricAdjustment::Px(0.0),
            height: MetricAdjustment::Px(0.0),
            baseline: MetricAdjustment::Px(0.0),
            snap_cap_height: true,
            hinted: false,
        };
        let result = CellMetrics::compute(input, options).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(result.device_size, size);
        assert!(!result.cap_snapped);
    }
}

/// A hinted face keeps the requested size even when an unhinted one snaps.
#[test]
fn hinting_never_changes_cell_origin_formula() {
    let input = MetricInput {
        advance_em: 0.6,
        ascent_em: 0.8,
        descent_em: -0.2,
        gap_em: 0.0,
        cap_em: Some(0.75),
        underline_em: 0.05,
    };
    let options = CellOptions {
        size: 12.0,
        dpr: 1.0,
        line_height: 1.2,
        width: MetricAdjustment::Px(0.0),
        height: MetricAdjustment::Px(0.0),
        baseline: MetricAdjustment::Px(0.0),
        snap_cap_height: true,
        hinted: true,
    };
    let result = CellMetrics::compute(input, options).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(result.device_size, 12.0);
    assert_eq!(result.width_px, 7);
    assert_eq!(result.height_px, 15);
    assert_eq!(result.baseline_px, 11);
}

/// A preloaded pack and its later full face cannot move fixed cells.
#[test]
fn pack_and_full_font_share_cell_geometry() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    );
    let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let data = crate::FontData::new(bytes, 0);
    let face = crate::face::FontFace::new(data.clone(), 400)
        .unwrap_or_else(|error| panic!("font face: {error}"));
    let pack = crate::pack::FontPack::build(data, 400)
        .unwrap_or_else(|error| panic!("font pack: {error}"));
    for dpr in [1.0, 1.25, 1.5, 2.0] {
        // Hinted 1× text leaves cap size alone; high DPR uses the same
        // bounded cap snap from both pack and original face metrics.
        let options = CellOptions {
            size: 14.0,
            dpr,
            line_height: 1.0,
            width: MetricAdjustment::Px(0.0),
            height: MetricAdjustment::Px(0.0),
            baseline: MetricAdjustment::Px(0.0),
            snap_cap_height: true,
            hinted: dpr < 1.5,
        };
        let packed = CellMetrics::from_pack(pack.metrics, options)
            .unwrap_or_else(|error| panic!("pack metrics: {error}"));
        let full = CellMetrics::from_face(&face, options)
            .unwrap_or_else(|error| panic!("full metrics: {error}"));
        assert_eq!(packed.width_px, full.width_px, "DPR {dpr}");
        assert_eq!(packed.height_px, full.height_px, "DPR {dpr}");
        assert_eq!(packed.baseline_px, full.baseline_px, "DPR {dpr}");
        assert_eq!(packed.device_size, full.device_size, "DPR {dpr}");
    }
}
