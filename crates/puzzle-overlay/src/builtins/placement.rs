//! Shared floating-panel bounds keep native exclusions separate from anchors.
//!
//! Both corner and caret-relative chrome use the public exclusion placer. Grid
//! annotations keep their source anchors and never call these helpers. Viewport
//! bounds include padding; caret origins come from the same sealed frame as ink.
//! Occlusions are logical points, so these helpers never apply backing scale.
//! A failed fit suppresses a whole panel, not merely its text or its controls.
//! This also keeps presented hit regions aligned with the retained draw list.
use crate::{geometry::Rect, view::ViewCtx};

/// Common model bounds are constrained to the actually visible surface.
pub(super) fn panel(ctx: &ViewCtx<'_>, width: f32, height: f32) -> Option<Rect> {
    // Clamp small windows so a fixed panel cannot extend beyond the surface.
    // The right/top inset leaves room around the native window edge.
    let surface = ctx.geometry.viewport.size;
    ctx.place(Rect {
        origin: [(surface[0] - width - 12.0).max(0.0), 12.0],
        size: [width.min(surface[0]), height.min(surface[1])],
    })
}

/// Move a cursor-relative hint without detaching its ink and clip from the caret.
pub(super) fn cursor(ctx: &ViewCtx<'_>, mut bounds: Rect) -> Option<Rect> {
    if ctx.models.occluded_rects.is_none() {
        return Some(bounds);
    }
    // The source and geometry belong to this same sealed frame, never live input.
    let cursor = ctx.buffer.cursor().filter(|cursor| cursor.visible)?;
    let origin = ctx
        .geometry
        .cell_rect(crate::geometry::Cell {
            line: cursor.line,
            column: cursor.column,
        })?
        .origin;
    let placed = ctx.place(Rect {
        origin: [origin[0] + bounds.origin[0], origin[1] + bounds.origin[1]],
        size: bounds.size,
    })?;
    // Keep Anchor::Cursor; only the relative panel/text offset changes.
    bounds.origin = [placed.origin[0] - origin[0], placed.origin[1] - origin[1]];
    Some(bounds)
}
