//! Anchored controls may move within their source's visible rectangle, never its ink.
//!
//! Keep the original row first: each obstruction moves the control's right edge
//! to that obstruction's left edge. If the source has no horizontal room, visit
//! its remaining visible row origins in display order. A control must fit whole;
//! callers omit both its ink and its target when no row has unobscured room.
use crate::geometry::Rect;

/// Place inside source bounds, trying the original row before subsequent rows.
/// Empty coverage preserves the historical layout without a geometry traversal.
pub(crate) fn anchored(
    preferred: Rect,
    area: Rect,
    rows: impl Iterator<Item = f32>,
    covered: &[Rect],
) -> Option<Rect> {
    if covered.is_empty() || covered.iter().all(|rect| !preferred.overlaps(*rect)) {
        return Some(preferred);
    }
    // Visible row origins, not rounded pixel offsets, align fallback to text.
    // Geometry already carries fractional smooth-scroll and snapped DPR metrics.
    // Only a complete fit is useful: clipping half a label would leave a control
    // that the user cannot read even though its source still has visible rows.
    std::iter::once(preferred.origin[1])
        .chain(rows.filter(|y| *y > preferred.origin[1]))
        .find_map(|y| {
            on_row(
                Rect {
                    origin: [preferred.origin[0], y],
                    ..preferred
                },
                area,
                covered,
            )
        })
}

/// Slide only left. Every collision strictly lowers X past one obstacle's edge.
fn on_row(mut candidate: Rect, area: Rect, covered: &[Rect]) -> Option<Rect> {
    // Once the candidate passes an obstacle's left edge it cannot hit that
    // obstacle again. Thus at most covered.len() moves occur, without a retry cap.
    while fits(candidate, area) {
        let edge = covered
            .iter()
            .filter(|rect| candidate.overlaps(**rect))
            .map(|rect| rect.origin[0])
            .min_by(f32::total_cmp);
        let Some(edge) = edge else {
            return Some(candidate);
        };
        // The next rectangle abuts the blocker; touching edges do not overlap.
        candidate.origin[0] = edge - candidate.size[0];
    }
    // Never borrow space from another block, padding, or the terminal grid.
    None
}

/// Full containment keeps a relocated control and its hit rectangle identical.
fn fits(rect: Rect, area: Rect) -> bool {
    (0..2).all(|axis| {
        rect.origin[axis] >= area.origin[axis]
            && rect.origin[axis] + rect.size[axis] <= area.origin[axis] + area.size[axis]
    })
}
