//! Powerline polygons and a font-independent missing-codepoint indicator.
//!
//! Powerline tips meet the neighbouring cell's full fill at an exact
//! device boundary. The tofu outline uses a tiny built-in hexadecimal
//! alphabet, so a missing scalar remains diagnosable even if the fallback
//! font itself is corrupt or unavailable.
//!
//! Powerline separators are intentionally polygons rather than a font
//! lookup. A full-height edge aligns with the filled background block in
//! the preceding cell. The arrow tip then crosses the current cell while
//! keeping its source-grid advance exactly one column.
//!
//! The tofu codepoint is drawn into one cell as a border plus a miniature
//! hexadecimal raster. The digits explain which scalar lacked coverage.
//! This remains possible when no loaded face can draw even ASCII `?`.
use super::{BuiltinCell, PixelPen};

/// Produces common separator triangles and branch-marker outlines.
pub(super) fn powerline(pen: &mut PixelPen, cell: BuiltinCell, code: u32) -> u8 {
    let w = f64::from(cell.width);
    let h = f64::from(cell.height);
    let t = f64::from(cell.stroke.max(1));
    // The first group points right and presents a full left edge. The
    // inverse group points left and presents a full right edge.
    match code {
        // Solid left/right separators own the whole edge adjoining fill.
        0xe0b0 | 0xe0b4 | 0xe0b8 | 0xe0bc => pen.polygon(&[(0.0, 0.0), (w, h / 2.0), (0.0, h)]),
        0xe0b2 | 0xe0b6 | 0xe0ba | 0xe0be => pen.polygon(&[(w, 0.0), (0.0, h / 2.0), (w, h)]),
        // Thin separators retain a hollow centre and never advance cells.
        // Their outer and inner tips share the same y midpoint, preserving
        // apparent stroke weight when the row height is odd.
        0xe0b1 | 0xe0b5 | 0xe0b9 | 0xe0bd => pen.polygon(&[
            (0.0, 0.0),
            (t, 0.0),
            (w, h / 2.0),
            (t, h),
            (0.0, h),
            (w - t, h / 2.0),
        ]),
        0xe0b3 | 0xe0b7 | 0xe0bb | 0xe0bf => pen.polygon(&[
            (w, 0.0),
            (w - t, 0.0),
            (0.0, h / 2.0),
            (w - t, h),
            (w, h),
            (t, h / 2.0),
        ]),
        // The four U+E0A* symbols are represented by a branch node and
        // either a filled or open upper/lower connection.
        // Integer strokes keep this junction on the same cell grid as the
        // box-drawing family, although these are Private Use scalars.
        0xe0a0..=0xe0a3 => {
            let cx = i32::from(cell.width / 2);
            let cy = i32::from(cell.height / 2);
            pen.rect(
                cell,
                cx,
                0,
                cx + i32::from(cell.stroke),
                i32::from(cell.height),
            );
            pen.rect(
                cell,
                0,
                cy,
                i32::from(cell.width),
                cy + i32::from(cell.stroke),
            );
        }
        _ => {}
    }
    // These polygons are opaque coverage; the caller's text color is
    // applied after Slug rasterization like any ordinary glyph.
    255
}

/// Draws a one-pixel border and the scalar's hexadecimal digits in rows.
pub(super) fn tofu(pen: &mut PixelPen, cell: BuiltinCell, code: u32) {
    tofu_border(pen, cell);
    let w = i32::from(cell.width);
    let h = i32::from(cell.height);
    // Uppercase hexadecimal matches Unicode diagnostic notation, which
    // makes this fallback legible beside U+XXXX messages in logs.
    let hex = format!("{code:X}");
    if w < 9 || h < 8 {
        return;
    }
    let rows = hex.len().div_ceil(2);
    // Two 3-pixel digits plus one-pixel separation fit in a nine-pixel
    // cell; additional digits move downward in six-pixel row increments.
    for (index, digit) in hex.chars().enumerate() {
        let row = index / 2;
        let col = index % 2;
        // `index` is bounded by six Unicode hex digits. Conversion does
        // not truncate or wrap even if a malformed caller passes a u32.
        let y = 2 + i32::try_from(row).unwrap_or(0) * 6;
        // A too-short cell retains its border instead of drawing the
        // final digit across the bottom edge and into another row.
        if y + 5 >= h || row >= rows {
            continue;
        }
        draw_digit(pen, cell, digit, 1 + i32::try_from(col).unwrap_or(0) * 4, y);
    }
}

/// Keeps all four border contours together, preserving nonzero winding at corners.
pub(super) fn tofu_border(pen: &mut PixelPen, cell: BuiltinCell) {
    let w = i32::from(cell.width);
    let h = i32::from(cell.height);
    // The diagnostic outline reserves one device pixel around the mini alphabet.
    let t = i32::from(cell.stroke.min(1));
    pen.rect(cell, 0, 0, w, t);
    pen.rect(cell, 0, h - t, w, h);
    pen.rect(cell, 0, 0, t, h);
    pen.rect(cell, w - t, 0, w, h);
}

/// Paints a 3×5 hexadecimal digit from rows of three binary pixels.
pub(super) fn draw_digit(pen: &mut PixelPen, cell: BuiltinCell, digit: char, x: i32, y: i32) {
    // The mini alphabet is intentionally independent of every font table.
    // Bit 2 is the left pixel and bit 0 is the right pixel in each row.
    const GLYPHS: [[u8; 5]; 16] = [
        // 0–3 and 4–7 follow the familiar seven-segment silhouettes,
        // but each row is encoded as three filled square device pixels.
        [7, 5, 5, 5, 7],
        [2, 6, 2, 2, 7],
        [7, 1, 7, 4, 7],
        [7, 1, 7, 1, 7],
        [5, 5, 7, 1, 1],
        [7, 4, 7, 1, 7],
        [7, 4, 7, 5, 7],
        [7, 1, 1, 1, 1],
        // 8–B and C–F complete the hexadecimal diagnostic alphabet.
        [7, 5, 7, 5, 7],
        [7, 5, 7, 1, 7],
        [7, 5, 7, 5, 5],
        [6, 5, 6, 5, 6],
        [7, 4, 4, 4, 7],
        [6, 5, 5, 5, 6],
        [7, 4, 6, 4, 7],
        [7, 4, 6, 4, 4],
    ];
    let Some(value) = digit.to_digit(16) else {
        return;
    };
    // A set bit paints one exact device pixel. It is not antialiased
    // before Slug, which would soften the tiny letters twice.
    for (row, bits) in GLYPHS[value as usize].into_iter().enumerate() {
        for col in 0..3 {
            if bits & (1 << (2 - col)) != 0 {
                // PixelPen converts this square to em coordinates while
                // retaining integer edges for the output device size.
                pen.rect(
                    cell,
                    x + col,
                    y + row as i32,
                    x + col + 1,
                    y + row as i32 + 1,
                );
            }
        }
    }
}
