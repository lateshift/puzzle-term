//! Fixed scrollbar targets survive partial coverage by native UI (D-61).
//!
//! Track paging excludes whole vertical bands, preserving the document-to-track
//! mapping. Thumb dragging instead keeps its original grab bounds whenever any
//! part remains exposed: native controls above the surface own covered clicks.
//! All sweeps borrow the bounded host list; retained DrawList capacity is reused.
use crate::geometry::Rect;

/// Emit full-width vertical gaps outside the union of overlapping host bands.
/// A narrow horizontal shadow removes only its Y interval, rather than making
/// every part of the fixed scrollbar disappear. Drawing still uses the full
/// track; these rectangles describe paging and hover input only.
pub(super) fn track_bands(rect: Rect, covered: &[Rect], mut emit: impl FnMut(Rect)) {
    let end = rect.origin[1] + rect.size[1];
    let mut y = rect.origin[1];
    while y < end {
        // Either a band covers y, or the next band's top ends a clear interval.
        let mut clear_end = end;
        let mut covered_end = y;
        // Touching X edges and zero-area host metadata must not remove targets.
        for obstacle in covered.iter().filter(|other| rect.overlaps(**other)) {
            let top = obstacle.origin[1];
            let bottom = top + obstacle.size[1];
            // Unsorted overlapping bands merge by advancing to a covering end.
            if top <= y && bottom > y {
                covered_end = covered_end.max(bottom.min(end));
            } else if top > y {
                clear_end = clear_end.min(top);
            }
        }
        if covered_end > y {
            y = covered_end;
        } else {
            // Preserve X and width so paging uses the original scrollbar column.
            emit(Rect {
                origin: [rect.origin[0], y],
                size: [rect.size[0], clear_end - y],
            });
            y = clear_end;
        }
        // Each iteration crosses an obstacle edge or the track end, so even
        // nested/disjoint rectangles terminate without a pixel-sized step.
    }
}

/// Keep the entire thumb target unless the union of host rectangles covers it.
/// Checking only vertical bands would incorrectly hide a thumb whose left edge
/// touches a find-box shadow. Horizontal slabs distinguish that partial cover
/// from several host rectangles jointly covering every point of the thumb.
pub(super) fn thumb_visible(rect: Rect, covered: &[Rect]) -> bool {
    let end = rect.origin[0] + rect.size[0];
    let mut x = rect.origin[0];
    while x < end {
        // Split only at actual host edges. Within a slab, horizontal coverage
        // is constant, so a vertical gap proves that some thumb area is visible.
        let next = covered
            .iter()
            .filter(|other| rect.overlaps(**other))
            .flat_map(|other| [other.origin[0], other.origin[0] + other.size[0]])
            .filter(|edge| *edge > x)
            .fold(end, f32::min);
        // Start at the thumb's own left edge and stop at its right edge; host
        // rectangles can extend beyond either without enlarging the tested area.
        let slab = Rect {
            origin: [x, rect.origin[1]],
            size: [next - x, rect.size[1]],
        };
        let mut visible = false;
        track_bands(slab, covered, |_| visible = true);
        if visible {
            return true;
        }
        // Strictly increasing obstacle edges bound this sweep by the host list.
        x = next;
    }
    false
}
