//! Host UI exclusion regions in logical surface coordinates (D-61).
//!
//! These constrain movable chrome, never terminal cells or text geometry. Empty
//! lists take the original placement path without allocating or moving anything.
//! Candidate origins come from obstacle edges, not a pixel search or a tuned gap.
//! Every accepted panel fits the surface and intersects no host rectangle; when
//! none fits, omitting it avoids stale ink and invisible interactive targets.
use crate::geometry::Rect;
/// Source-bounded placement for interactive controls, separate from floating panels.
mod controls;
/// Both built-in and optional source views share this policy through ViewCtx.
pub(crate) use controls::anchored;

/// Bound host-owned UI metadata independently of the terminal's text/history size.
/// Sixty-four rectangles cover complex host chrome without unbounded placement work.
pub const MAX_RECTS: usize = 64;

/// Reject a malformed list atomically before replacing the previous host geometry.
pub fn valid(rects: &[Rect]) -> bool {
    // Validate sums too: finite components can still overflow at the far edge.
    rects.len() <= MAX_RECTS
        && rects.iter().all(|rect| {
            (0..2).all(|axis| {
                rect.origin[axis].is_finite()
                    && rect.size[axis].is_finite()
                    && rect.size[axis] >= 0.0
                    && (rect.origin[axis] + rect.size[axis]).is_finite()
            })
        })
}

/// Keep a preferred panel if possible; otherwise try obstacle edges below/left/above.
/// No scratch storage or font work is needed; all candidates are borrowed coordinates.
pub fn place(preferred: Rect, surface: Rect, rects: &[Rect]) -> Option<Rect> {
    if rects.is_empty() {
        // Keep historical coordinates exactly; do not even clamp an ordinary view.
        return Some(preferred);
    }
    // The nested edge iterators also cover combinations of independent obstacles.
    // A finite host list and four surface edges bound work even if nothing fits.
    // Keep the preferred column first, allowing a downward move before moving left.
    for x in positions(0, preferred, surface, rects) {
        for y in positions(1, preferred, surface, rects) {
            let candidate = Rect {
                origin: [x, y],
                size: preferred.size,
            };
            // A panel must fit in full; clipping its controls is not avoidance.
            let inside = (0..2).all(|axis| {
                candidate.origin[axis] >= surface.origin[axis]
                    && candidate.origin[axis] + candidate.size[axis]
                        <= surface.origin[axis] + surface.size[axis]
            });
            // Exact shared edges are allowed; zero-area host rectangles cover nothing.
            if inside && rects.iter().all(|rect| !candidate.overlaps(*rect)) {
                return Some(candidate);
            }
        }
    }
    // Views omit both ink and regions, rather than retaining an obscured old list.
    None
}

/// Preferred coordinate, obstacle edges and surface edges require no allocations.
fn positions(
    axis: usize,
    panel: Rect,
    surface: Rect,
    rects: &[Rect],
) -> impl Iterator<Item = f32> + '_ {
    // Each obstacle contributes exactly two origins, so an impossible fit terminates.
    std::iter::once(panel.origin[axis])
        .chain(rects.iter().flat_map(move |rect| {
            [
                rect.origin[axis] + rect.size[axis],
                rect.origin[axis] - panel.size[axis],
            ]
        }))
        .chain([
            surface.origin[axis],
            surface.origin[axis] + surface.size[axis] - panel.size[axis],
        ])
}
