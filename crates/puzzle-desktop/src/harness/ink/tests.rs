//! Cell assertions must be sensitive to genuinely blank terminal cells.

use super::{CellGeometry, check};
use puzzle_render::RgbaImage;

/// A blank cell cannot satisfy a positive ink lower bound.
#[test]
fn blank_terminal_cell_fails() {
    let image = RgbaImage {
        width: 48,
        height: 48,
        pixels: vec![0; 48 * 48 * 4],
    };
    let cells = CellGeometry {
        cols: 2,
        rows: 2,
        width: 8,
        height: 16,
    };
    let result = check(
        &image,
        &mut ["0:0-1:1", "1%", "80%"].into_iter(),
        Some(cells),
    );
    assert!(result.is_err_and(|error| error.contains("ink 0.00%")));
}

/// Cell coordinates use row first and reject a range outside the grid.
#[test]
fn row_column_bounds_are_checked() {
    let image = RgbaImage {
        width: 48,
        height: 48,
        pixels: vec![0; 48 * 48 * 4],
    };
    let cells = CellGeometry {
        cols: 2,
        rows: 2,
        width: 8,
        height: 16,
    };
    let result = check(
        &image,
        &mut ["0:0-1:3", "1%", "80%"].into_iter(),
        Some(cells),
    );
    assert!(result.is_err_and(|error| error.contains("outside terminal grid")));
}
