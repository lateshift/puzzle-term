//! Braille and sextant dots placed on a fixed device-pixel cell grid.
//!
//! Braille's Unicode bits are ordered by column (dots 1,2,3,7 on the
//! left; 4,5,6,8 on the right). Sextants number their two columns across
//! three rows. Keeping that naming apart from physical pixel positions
//! prevents a font-dependent offset from opening seams in block art.
//!
//! Dot positions are calculated after the cell has been rounded to
//! physical device pixels. That keeps all Braille dots at one apparent
//! weight on a 1× display, even when its cell width is odd. Sextants
//! differ: they are solid blocks whose subcells must tile completely.
use super::{BuiltinCell, PixelPen};

/// Draws the eight Braille dots encoded directly in U+2800's low byte.
pub(super) fn braille(pen: &mut PixelPen, cell: BuiltinCell, code: u32) -> u8 {
    let bits = (code - 0x2800) as u8;
    // The position table follows Braille's historical numbering, which
    // is not row-major: bits 7 and 8 are the last physical row.
    let positions = [
        (0, 0),
        (0, 1),
        (0, 2),
        (1, 0),
        (1, 1),
        (1, 2),
        (0, 3),
        (1, 3),
    ];
    let w = i32::from(cell.width);
    let h = i32::from(cell.height);
    let dot_w = (w / 4).max(1);
    let dot_h = (h / 8).max(1);
    // An empty Braille blank produces no contours, just as a space does.
    // The caller still keeps its fixed one-cell advance.
    for (bit, (col, row)) in positions.into_iter().enumerate() {
        if bits & (1 << bit) == 0 {
            continue;
        }
        // Dots are inset from each quadrant edge, so adjacent Braille
        // characters remain individually legible rather than merging.
        let x = ((2 * col + 1) * w / 4) - dot_w / 2;
        let y = ((2 * row + 1) * h / 8) - dot_h / 2;
        // Clipping in PixelPen handles the smallest valid cells without
        // letting an edge dot mark a neighbouring terminal column.
        pen.rect(cell, x, y, x + dot_w, y + dot_h);
    }
    255
}

/// Draws the legacy six-part block pattern assigned to U+1FB00–1FB3B.
pub(super) fn sextant(pen: &mut PixelPen, cell: BuiltinCell, code: u32) -> u8 {
    // Unicode omits the two six-part patterns already assigned in the
    // older block range. The lookup preserves the published code order.
    const MASKS: [u8; 60] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
        0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f,
        0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e,
    ];
    let mask = MASKS[(code - 0x1fb00) as usize];
    // Unlike Braille, every sextant owns a complete fraction of the cell.
    // Integer division assigns any odd remainder consistently to the
    // rightmost or bottommost subcell, leaving no uncovered strip.
    let w = i32::from(cell.width);
    let h = i32::from(cell.height);
    for bit in 0..6 {
        if mask & (1 << bit) == 0 {
            continue;
        }
        let col = bit % 2;
        let row = bit / 2;
        // Dividing shared edges with the same integer formula makes
        // opposite sextants tile a cell without empty device pixels.
        let x0 = col * w / 2;
        let x1 = (col + 1) * w / 2;
        let y0 = row * h / 3;
        let y1 = (row + 1) * h / 3;
        pen.rect(cell, x0, y0, x1, y1);
    }
    255
}
