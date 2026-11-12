//! Unicode box strokes whose arms reach neighbouring cell boundaries.
//!
//! The compact table follows Unicode's U+2500–U+257F names. Each two-bit
//! arm encodes absent, light, heavy or double in up/down/left/right order.
//! It was transcribed from those directional names, allowing mixed-weight
//! junctions to keep the arm weight of each neighbouring character.
//!
//! For example `─` has left and right light arms, while `┼` has all
//! four. A left arm fills from x=0 to the cell centre; the next cell's
//! right arm fills through x=width. Their filled regions touch at the
//! shared integer boundary without a font bearing decision.
//!
//! Curved corners and dashed forms occupy this same range. They use their
//! own contours: a dash's gaps are intentional, while a rounded corner's
//! curve preserves the same edge endpoints as a square corner.
//!
//! The Unicode range contains several visually related but geometrically
//! distinct families. Ordinary arms are rectangles because their edge
//! coverage must be exact at both terminal-cell boundaries. Dashed forms
//! centre ink inside each integer slot; the two end margins add to a gap
//! across cells rather than merging the last and first dashes.
//!
//! The four arc corners use the same integer stroke bands as straight light
//! arms. A quarter-circle ribbon joins their centre lines at a radius near
//! half the shorter cell dimension. Only the arm directions change among
//! corners; reflecting the cell itself would shift a band in an odd-sized
//! cell. The ordinary kurbo-to-Slug path still prepares these contours.
use super::{BuiltinCell, PixelPen};

/// Cubic control distance for one quarter of a circle, relative to radius.
const QUARTER_CIRCLE_KAPPA: f64 = 0.552_284_749_830_793_6;

/// Four two-bit arm styles in Unicode codepoint order.
const ARMS: [u8; 128] = [
    // The early codes are horizontal, vertical, dashed and simple corners.
    0x50, 0xa0, 0x05, 0x0a, 0x50, 0xa0, 0x05, 0x0a, 0x50, 0xa0, 0x05, 0x0a, 0x44, 0x84, 0x48, 0x88,
    0x14, 0x24, 0x18, 0x28, 0x41, 0x81, 0x42, 0x82, 0x11, 0x21, 0x12, 0x22, 0x45, 0x85, 0x46, 0x49,
    0x4a, 0x86, 0x89, 0x8a, 0x15, 0x25, 0x16, 0x19, 0x1a, 0x26, 0x29, 0x2a, 0x54, 0x64, 0x94, 0xa4,
    // Mixed junctions preserve each half-arm's light or heavy weight.
    0x58, 0x68, 0x98, 0xa8, 0x51, 0x61, 0x91, 0xa1, 0x52, 0x62, 0x92, 0xa2, 0x55, 0x65, 0x95, 0xa5,
    0x56, 0x59, 0x5a, 0x66, 0x96, 0x69, 0x99, 0xa6, 0xa9, 0x6a, 0x9a, 0xaa, 0x50, 0xa0, 0x05, 0x0a,
    0xf0, 0x0f, 0xc4, 0x4c, 0xcc, 0x34, 0x1c, 0x3c, 0xc1, 0x43, 0xc3, 0x31, 0x13, 0x33, 0xc5, 0x4f,
    // Double-line junctions and the final half-line codes finish the table.
    0xcf, 0x35, 0x1f, 0x3f, 0xf4, 0x5c, 0xfc, 0xf1, 0x53, 0xf3, 0xf5, 0x5f, 0xff, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x10, 0x01, 0x40, 0x04, 0x20, 0x02, 0x80, 0x08, 0x90, 0x09, 0x60, 0x06,
];

/// Draws a named box character with integer cell edges and weights.
pub(super) fn draw(pen: &mut PixelPen, cell: BuiltinCell, code: u32) -> u8 {
    // The public dispatcher has already checked the range, so subtraction
    // yields a valid table index for every assigned box-drawing scalar.
    let index = (code - 0x2500) as usize;
    let arms = ARMS[index];
    // Diagonals occupy the four reserved zero entries in the arm table;
    // their crossing must stay inside one cell rather than join sideways.
    if (0x2571..=0x2573).contains(&code) {
        diagonal(pen, cell, code);
        return 255;
    }
    if matches!(code, 0x2504..=0x250b | 0x254c..=0x254f) {
        dashed(pen, cell, code);
        return 255;
    }
    if (0x256d..=0x2570).contains(&code) {
        rounded_corner(pen, cell, code);
        return 255;
    }
    let cx = i32::from(cell.width / 2);
    let cy = i32::from(cell.height / 2);
    // Draw arms independently to represent mixed-weight tees and crosses.
    // Their small centre overlap prevents a one-pixel junction pinhole.
    for direction in 0..4 {
        let style = (arms >> (direction * 2)) & 3;
        if style != 0 {
            arm(pen, cell, cx, cy, direction, style);
        }
    }
    255
}

/// Centres Unicode's two-, three- and four-dash strokes in pixel slots.
fn dashed(pen: &mut PixelPen, cell: BuiltinCell, code: u32) {
    // Unicode distinguishes two, three and four pieces; the glyph's
    // visual count should not collapse to an ordinary solid arm.
    let requested = match code {
        0x2504..=0x2507 => 3,
        0x2508..=0x250b => 4,
        _ => 2,
    };
    let vertical = matches!(code, 0x2506..=0x2507 | 0x250a..=0x250b | 0x254e..=0x254f);
    // Heavy dashed strokes have twice the light cell stroke while keeping
    // the same longitudinal gaps and terminal-cell advance.
    let heavy = matches!(code, 0x2505 | 0x2507 | 0x2509 | 0x250b | 0x254d | 0x254f);
    let length = i32::from(if vertical { cell.height } else { cell.width });
    // The perpendicular centre matches the full solid-arm table, so a
    // dashed segment can meet a tee without a sideward offset.
    let centre = i32::from(if vertical { cell.width } else { cell.height }) / 2;
    let thick = i32::from(cell.stroke) * if heavy { 2 } else { 1 };
    let (band_start, band_end) = stroke_band(centre, thick);
    // A cell narrower than its named dash count cannot fit both ink and
    // gaps. Preserve as many pieces as integer pixels permit.
    let count = requested.min(((length + 1) / 2).max(1));
    let dash_length = (length / count / 2).max(1);
    for part in 0..count {
        // Equal slots differ by at most one pixel. Centring each fixed-size
        // dash leaves a half-gap at each cell edge after quantisation.
        let slot_start = part * length / count;
        let slot_end = (part + 1) * length / count;
        let start = slot_start + (slot_end - slot_start - dash_length + 1) / 2;
        let end = start + dash_length;
        if vertical {
            pen.rect(cell, band_start, start, band_end, end);
        } else {
            pen.rect(cell, start, band_start, end, band_end);
        }
    }
}

/// Fills a circular quarter-turn between the exact straight-arm bands.
fn rounded_corner(pen: &mut PixelPen, cell: BuiltinCell, code: u32) {
    // The signs choose the open horizontal and vertical arms. The local
    // curve stays identical, while odd cell widths keep their snapped band.
    let (sx, sy) = match code {
        0x256e => (-1.0, 1.0),
        0x256f => (-1.0, -1.0),
        0x2570 => (1.0, -1.0),
        _ => (1.0, 1.0),
    };
    let w = f64::from(cell.width);
    let h = f64::from(cell.height);
    let t = f64::from(cell.stroke.max(1)).min(w.min(h));
    let (vx0, vx1) = stroke_band(i32::from(cell.width / 2), t as i32);
    let (hy0, hy1) = stroke_band(i32::from(cell.height / 2), t as i32);
    let (cx, cy) = (f64::from(vx0 + vx1) / 2.0, f64::from(hy0 + hy1) / 2.0);
    let ex = if sx > 0.0 { w - cx } else { cx };
    let ey = if sy > 0.0 { h - cy } else { cy };
    // Bound all four corners by the shortest available half-cell. This
    // keeps one common radius when an even width puts the stroke band
    // half a pixel to one side of the geometric cell centre.
    let radius = (w.min(h) / 2.0 - t / 2.0)
        .min(cx.min(w - cx) - 0.5)
        .min(cy.min(h - cy) - 0.5)
        .max(t / 2.0);
    let (outer, inner) = (radius + t / 2.0, radius - t / 2.0);
    // Map the local y-down quarter turn into the same em-space transform
    // as PixelPen::point, without borrowing its mutable path during writes.
    let map = |u: f64, v: f64| kurbo::Point::new((cx + sx * u) / h, 1.0 - (cy + sy * v) / h);
    pen.path.move_to(map(-t / 2.0, ey));
    pen.path.line_to(map(-t / 2.0, radius));
    // Circle tangents meet both rectangles without a kink. Slug's cubic
    // converter then gives the same quadratic input as any font outline.
    pen.path.curve_to(
        map(-t / 2.0, radius - QUARTER_CIRCLE_KAPPA * outer),
        map(radius - QUARTER_CIRCLE_KAPPA * outer, -t / 2.0),
        map(radius, -t / 2.0),
    );
    pen.path.line_to(map(ex, -t / 2.0));
    pen.path.line_to(map(ex, t / 2.0));
    pen.path.line_to(map(radius, t / 2.0));
    pen.path.curve_to(
        map(radius - QUARTER_CIRCLE_KAPPA * inner, t / 2.0),
        map(t / 2.0, radius - QUARTER_CIRCLE_KAPPA * inner),
        map(t / 2.0, radius),
    );
    pen.path.line_to(map(t / 2.0, ey));
    pen.path.close_path();
}

/// Returns the half-open integer band used by every centred box stroke.
fn stroke_band(centre: i32, thick: i32) -> (i32, i32) {
    (centre - thick / 2, centre + (thick + 1) / 2)
}

/// Extends one vertical or horizontal half-stroke to its cell boundary.
fn arm(pen: &mut PixelPen, cell: BuiltinCell, cx: i32, cy: i32, direction: u8, style: u8) {
    let stroke = i32::from(cell.stroke);
    // Heavy lines double the already snapped light stroke in device pixels.
    let thick = if style == 2 { stroke * 2 } else { stroke };
    // A double stroke uses parallel pixel rows or columns. The gap stays
    // centred so a neighbouring double arm meets it at the shared edge.
    let offsets: &[i32] = if style == 3 { &[-stroke, stroke] } else { &[0] };
    for &offset in offsets {
        // Directions 0/1 fill vertical halves; 2/3 fill horizontal halves.
        // Both halves reach the cell boundary, never a fractional bearing.
        let (lo, hi) = stroke_band(if direction < 2 { cx } else { cy } + offset, thick);
        let (x0, y0, x1, y1) = match direction {
            0 => (lo, 0, hi, cy + 1),
            1 => (lo, cy, hi, i32::from(cell.height)),
            2 => (0, lo, cx + 1, hi),
            _ => (cx, lo, i32::from(cell.width), hi),
        };
        pen.rect(cell, x0, y0, x1, y1);
    }
}

/// Fills a diagonal as a thin quadrilateral through the cell corners.
fn diagonal(pen: &mut PixelPen, cell: BuiltinCell, code: u32) {
    // Diagonal tips stop at this cell's corners rather than extending as
    // arms into adjacent cells with unrelated content.
    let w = f64::from(cell.width);
    let h = f64::from(cell.height);
    let t = f64::from(cell.stroke.max(1));
    if code == 0x2571 || code == 0x2573 {
        pen.polygon(&[(w - t, 0.0), (w, 0.0), (t, h), (0.0, h)]);
    }
    if code == 0x2572 || code == 0x2573 {
        pen.polygon(&[(0.0, 0.0), (t, 0.0), (w, h), (w - t, h)]);
    }
}
