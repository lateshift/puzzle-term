//! Startup and resize grid tests for physical device pixels (§6.6).
//!
//! The live shell creates its PTY before the first prompt. These cases
//! derive the child size from the same bundled font metrics and physical
//! surface dimensions used by the frame builder at both common DPRs.
#![allow(
    clippy::expect_used,
    reason = "test fixtures report a missing bundled face as an assertion failure"
)]
use super::*;
use puzzle_term::frame_builder::FrameBuilder;
use puzzle_term::{slug_scene::FontData, text::font_set::FontSet};

/// Initial grid and later resize use the configured surface, not 80×24.
#[test]
fn startup_and_resize_use_physical_pixels_at_both_dprs() {
    // Cargo tests run with the crate as their working directory, while
    // the interactive harness resolves assets from the workspace root.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    );
    let bytes = std::fs::read(path).expect("bundled font");
    let fonts = FontSet::new(FontData::new(bytes, 0), None, None).expect("font set");
    for dpr in [1.0, 2.0] {
        // Each size here is a physical backing extent. Device-cell
        // metrics include DPR, so dividing logical dimensions again
        // would leave much of a Retina window unused.
        let metrics = FrameBuilder::metrics_for(&fonts, 14.0, dpr).expect("cell metrics");
        for (width, height) in [(800, 600), (1600, 1200), (2560, 1440)] {
            let size = PhysicalSize { width, height };
            let grid = grid_for_surface(size, metrics, 16);
            assert_eq!(u32::from(grid.columns), (width - 32) / metrics.width_px);
            assert_eq!(u32::from(grid.rows), (height - 32) / metrics.height_px);
            // This value goes directly into portable-pty at spawn and
            // into SessionConfig, so their first cell grid is identical.
            assert_eq!(u32::from(grid.pixel_width), width);
            assert_eq!(u32::from(grid.pixel_height), height);
            assert!(grid.columns > 80 || width == 800);
        }
    }
}

/// A tiny window still supplies the VT minimum instead of zero cells.
#[test]
fn tiny_surface_keeps_valid_pty_dimensions() {
    let metrics = CellMetrics {
        width_px: 16,
        height_px: 37,
        baseline_px: 28,
        device_size: 28.0,
        cap_snapped: false,
        underline_px: 1,
    };
    let grid = grid_for_surface(
        PhysicalSize {
            width: 1,
            height: 1,
        },
        metrics,
        16,
    );
    assert_eq!((grid.columns, grid.rows), (2, 1));
}

/// Producer UI consumes two columns, at every font/DPR/size, before PTY startup.
#[test]
fn reserved_gutter_reduces_only_the_pty_columns() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    ))
    .expect("font");
    let fonts = FontSet::new(FontData::new(bytes, 0), None, None).expect("font set");
    for dpr in [1.0, 2.0] {
        let metrics = FrameBuilder::metrics_for(&fonts, 14.0, dpr).expect("metrics");
        for size in [[800, 600], [1600, 1200], [400, 320]] {
            let size = PhysicalSize {
                width: size[0],
                height: size[1],
            };
            let ordinary = grid_for_surface(size, metrics, (8.0 * dpr) as u32);
            assert_eq!(
                ordinary,
                grid_with_gutter(size, metrics, (8.0 * dpr) as u32, 0)
            );
            let producer = grid_with_gutter(size, metrics, (8.0 * dpr) as u32, 2);
            assert_eq!(producer.columns + 2, ordinary.columns);
            assert_eq!(producer.rows, ordinary.rows);
            assert_eq!(
                [producer.pixel_width, producer.pixel_height],
                [ordinary.pixel_width, ordinary.pixel_height]
            );
        }
    }
}
