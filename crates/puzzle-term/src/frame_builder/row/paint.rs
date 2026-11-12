//! Pixel-aligned background and decoration rectangles for one VT row (§10.5).
//!
//! The terminal stores colors with tags, but the parent row builder resolves
//! each tag before calling here. This module merges consecutive equal
//! backgrounds and keeps SGR lines attached to device-pixel cell geometry.
//! A wide placeholder has its own background, although it has no second
//! glyph; ignoring it would leave a one-cell hole after a colored wide
//! character. Coordinates remain relative to the row until assembly.
//! The GPU's rect pipeline consumes each merged run as one instanced quad.
use super::{CachedRow, colors};
use puzzle_frame::RectInstance;
use puzzle_text::metrics::CellMetrics;
use puzzle_vt::cell::Attrs;

/// Keeps a single rectangle for consecutive cells of one background.
pub(super) fn add_background(
    cache: &mut CachedRow,
    column: usize,
    bg: [u8; 3],
    metrics: CellMetrics,
    clear: [f32; 4],
) {
    // The clear value is converted back to byte space only for equality:
    // default-background cells need no rect because the surface clear
    // already filled their pixels. This saves most instances in a shell.
    let clear_rgb = [
        (clear[0] * 255.0).round() as u8,
        (clear[1] * 255.0).round() as u8,
        (clear[2] * 255.0).round() as u8,
    ];
    if bg == clear_rgb {
        return;
    }
    // Every column begins on an integer device-pixel boundary. Its width
    // comes from the primary font even when a glyph uses fallback ink.
    let x = column as f32 * metrics.width_px as f32;
    let color = colors::rgba(bg);
    // A contiguous equal-color row becomes one rectangle rather than one
    // per cell. This matters for full-screen status bars and BCE erases.
    // Comparing the exact preceding right edge also prevents a gap after
    // a wide cell's second placeholder.
    if let Some(last) = cache.backgrounds.last_mut()
        && last.color == color
        && last.rect[0] + last.rect[2] == x
    {
        last.rect[2] += metrics.width_px as f32;
        return;
    }
    // Nonmatching colors start a new run. The row-relative y stays zero;
    // `FrameBuilder::assemble` supplies this line's current viewport y.
    cache.backgrounds.push(rect(
        x,
        0.0,
        metrics.width_px as f32,
        metrics.height_px as f32,
        color,
    ));
}

/// Adds snapped SGR decoration bars without modifying glyph placement.
pub(super) fn add_decoration(
    cache: &mut CachedRow,
    column: usize,
    ink: [u8; 3],
    underline_ink: [u8; 3],
    attrs: Attrs,
    metrics: CellMetrics,
) {
    // SGR decorations attach to physical cell columns, not glyph bounds.
    // A space underlined by an application must still show its line.
    let x = column as f32 * metrics.width_px as f32;
    let w = metrics.width_px as f32;
    let h = metrics.height_px as f32;
    let thickness = metrics.underline_px as f32;
    let color = colors::rgba(ink);
    // SGR 58 can specify a color distinct from the text foreground; its
    // reset SGR 59 has already been represented as the absent extra.
    let underline_color = colors::rgba(underline_ink);
    let underline = attrs.underline_style();
    // SGR 58 overrides only underline ink; strike and overline retain the
    // resolved foreground. Each style uses integer device-pixel bars.
    match underline {
        1 => cache
            .decorations
            .push(rect(x, h - thickness - 1.0, w, thickness, underline_color)),
        2 => {
            // A one-pixel gap keeps the two parallel strokes distinct at 1×.
            // Their y coordinates are derived from the same lower baseline,
            // so changing font height moves both bars together.
            cache
                .decorations
                .push(rect(x, h - thickness - 1.0, w, thickness, underline_color));
            cache.decorations.push(rect(
                x,
                h - 2.0 * thickness - 2.0,
                w,
                thickness,
                underline_color,
            ));
        }
        // Dotted and dashed strokes are separate short rectangles; an
        // antialiased font outline would vary with the face and break the
        // terminal's device-pixel decoration grid.
        4 | 5 => patterned(
            cache,
            x,
            w,
            h - thickness - 1.0,
            thickness,
            underline_color,
            underline,
        ),
        // The curly shape is prepared through the Slug atlas by `wave`.
        _ => {}
    }
    // Strike and overline use the same snapped thickness so they join
    // when an application changes only foreground color mid-row.
    if attrs.contains(Attrs::STRIKE) {
        cache
            .decorations
            .push(rect(x, (h * 0.5).round(), w, thickness, color));
    }
    if attrs.contains(Attrs::OVERLINE) {
        cache.decorations.push(rect(x, 1.0, w, thickness, color));
    }
}

/// Tiles dotted or dashed underlines with integer device-pixel periods.
fn patterned(
    cache: &mut CachedRow,
    x: f32,
    width: f32,
    y: f32,
    thickness: f32,
    color: u32,
    style: u8,
) {
    let width = width as u32;
    // Dots use the shortest visible dash, while dashed strokes fill two
    // dots' worth. The same period starts at every physical cell edge.
    // Truncating at the right boundary keeps a malformed or narrow cell
    // from spilling ink into the next cell's different SGR decoration.
    let length = if style == 4 { 1 } else { 3 };
    let period = length * 2;
    for start in (0..width).step_by(period as usize) {
        // The final bar may be shorter, but never crosses a cell boundary
        // and therefore cannot color its neighbour's padding.
        let span = length.min(width - start);
        cache
            .decorations
            .push(rect(x + start as f32, y, span as f32, thickness, color));
    }
}

/// Constructs a filled rectangle for cell decoration layers.
fn rect(x: f32, y: f32, width: f32, height: f32, color: u32) -> RectInstance {
    // Zero radius and zero stroke select the filled pixel-aligned branch
    // of the rect shader; no per-decoration geometry upload is needed.
    RectInstance {
        rect: [x, y, width, height],
        color,
        corner_radius: 0.0,
        stroke_width: 0.0,
        flags: 0,
    }
}
