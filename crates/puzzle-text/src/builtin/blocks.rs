//! Block elements filled from exact cell fractions and quadrant masks.
//!
//! Half and eighth blocks use integer edge positions, so two adjacent
//! blocks cover their shared boundary without relying on a font bearing.
//! Shade glyphs use a full solid outline with reduced alpha: a patterned
//! dither would vary with the cell's screen position and break joins.
//!
//! Unicode's names are fractions of a *whole cell*, not fractions of a
//! font glyph's ink box. Each endpoint is therefore an integer position
//! within the caller's exact device-pixel width or height.
use super::{BuiltinCell, PixelPen};

/// Draws U+2580–U+259F and returns its alpha coverage multiplier.
pub(super) fn draw(pen: &mut PixelPen, cell: BuiltinCell, code: u32) -> u8 {
    let w = i32::from(cell.width);
    let h = i32::from(cell.height);
    // Half and eighth blocks extend exactly to the border of the cell.
    // The Slug edge falls on the same pixel boundary in adjacent cells.
    match code {
        0x2580 => pen.rect(cell, 0, 0, w, h / 2),
        0x2581..=0x2588 => {
            // U+2581 begins at one eighth from the bottom; U+2588 fills all.
            let eighths = i32::try_from(code - 0x2580).unwrap_or(0);
            pen.rect(cell, 0, h - h * eighths / 8, w, h);
        }
        0x2589..=0x258f => {
            // Unicode orders seven through one eighth from the left.
            let eighths = i32::try_from(0x2590 - code).unwrap_or(0);
            pen.rect(cell, 0, 0, w * eighths / 8, h);
        }
        0x2590 => pen.rect(cell, w / 2, 0, w, h),
        0x2591..=0x2593 => {
            // Alpha changes ink density without introducing a repeating
            // pattern that might misalign at a row or column boundary.
            pen.rect(cell, 0, 0, w, h);
            return match code {
                0x2591 => 64,
                0x2592 => 128,
                _ => 191,
            };
        }
        0x2594 => pen.rect(cell, 0, 0, w, (h / 8).max(1)),
        0x2595 => pen.rect(cell, w - (w / 8).max(1), 0, w, h),
        0x2596..=0x259f => quadrants(pen, cell, code),
        _ => return 0,
    }
    255
}

/// Maps Unicode's quadrant variants to four exact half-cell rectangles.
fn quadrants(pen: &mut PixelPen, cell: BuiltinCell, code: u32) {
    // Bits are upper-left, upper-right, lower-left and lower-right.
    // The table follows Unicode names, not numerical bit order.
    const MASKS: [u8; 10] = [4, 8, 1, 13, 9, 7, 11, 2, 6, 14];
    // A mask bit names a physical quadrant; the Unicode order itself is
    // irregular because it follows historic block-graphics assignments.
    let mask = MASKS[(code - 0x2596) as usize];
    let w = i32::from(cell.width);
    let h = i32::from(cell.height);
    let mid_x = w / 2;
    let mid_y = h / 2;
    // Both halves use the same integer divider, including odd cell sizes.
    // There is neither a gap nor a doubly owned device-pixel column.
    for (bit, rect) in [
        (1, [0, 0, mid_x, mid_y]),
        (2, [mid_x, 0, w, mid_y]),
        (4, [0, mid_y, mid_x, h]),
        (8, [mid_x, mid_y, w, h]),
    ] {
        if mask & bit != 0 {
            pen.rect(cell, rect[0], rect[1], rect[2], rect[3]);
        }
    }
}
