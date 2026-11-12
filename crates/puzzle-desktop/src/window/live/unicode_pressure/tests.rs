//! GPU evidence compares text with independent paper, including uniform builtin cells.
use super::ink;
use puzzle_render::RgbaImage;

/// The reviewer exempts only invisible RGB contrast, never missing prepared coverage.
#[test]
fn zero_contrast_requires_prepared_coverage_and_other_cells_require_pixels() {
    assert!(super::valid_ink(true, true, 0));
    assert!(!super::valid_ink(false, true, 0));
    assert!(!super::valid_ink(true, false, 0));
    assert!(super::valid_ink(true, false, 1));
}

/// Full blocks and every uniform shade have ink despite having no unfilled corner.
#[test]
fn uniform_coverage_is_ink_and_colored_paper_is_not() {
    let paper = RgbaImage {
        width: 4,
        height: 4,
        pixels: [10, 20, 30, 255].repeat(16),
    };
    assert_eq!(ink(&paper, &paper, 0, 0, 4, 4), 0);
    for value in [64, 128, 192, 255] {
        let block = RgbaImage {
            width: 4,
            height: 4,
            pixels: [value, value, value, 255].repeat(16),
        };
        // A corner-based estimator would incorrectly report all these cells as blank.
        assert_eq!(ink(&block, &paper, 0, 0, 4, 4), 16);
    }
}

/// A narrow rule's first-pixel stroke belongs to its cell, while the next cell does not.
#[test]
fn full_cell_ink_includes_left_border_and_excludes_next_cell() {
    let paper = RgbaImage {
        width: 4,
        height: 4,
        pixels: [10, 20, 30, 255].repeat(16),
    };
    let mut image = RgbaImage {
        width: 4,
        height: 4,
        pixels: paper.pixels.clone(),
    };
    for y in 0..4 {
        image.pixels[y * 16..y * 16 + 4].copy_from_slice(&[255, 255, 255, 255]);
    }
    assert_eq!(ink(&image, &paper, 0, 0, 1, 4), 4);
    assert_eq!(ink(&image, &paper, 1, 0, 1, 4), 0);
}
