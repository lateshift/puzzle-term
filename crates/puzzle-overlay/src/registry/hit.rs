//! Region picking follows the same painter order and nested scissors as rendering.
//!
//! Hit testing borrows retained lists without allocating a clip stack. Walking
//! preceding operations backwards pairs pops with pushes, leaving precisely
//! the active intersections at the region. Each slot has independent clips,
//! and each provider starts with the same grid/Top base scissor (§10.3).
//!
//! Logical pointer rectangles are resolved using the same attachment origin
//! as painter commands. Hidden and Gone anchors cannot produce a hit. A Top
//! region is clipped to the surface; other slots also inherit the grid clip.
//! An oversized region cannot bypass its enclosing provider scissor.
//!
//! Reverse clip traversal skips already-popped pairs. Any unmatched push
//! still encloses the region and must contain the point. Multiple unmatched
//! pushes are intersections, so no temporary rectangle stack is allocated.
//! A clip from another slot or another provider never leaks into this list.
//!
//! Painter order is the tuple (slot, registration identity, command index).
//! Monotonic registration IDs match surviving provider submission order.
//! Later commands in the same provider are visually above earlier ones.
use crate::draw::Draw;
use crate::draw::{DrawList, Primitive};
use crate::geometry::{Rect, TextGeometry};
use puzzle_frame::ZSlot;

/// Resolves an anchor-relative rectangle to logical surface coordinates.
/// Presented-region tables reuse this placement math with their captured geometry.
pub(crate) fn placed(rect: Rect, draw: &Draw, geometry: &TextGeometry) -> Option<Rect> {
    let origin = draw.resolved.origin(geometry)?;
    Some(Rect {
        origin: [rect.origin[0] + origin[0], rect.origin[1] + origin[1]],
        size: rect.size,
    })
}

/// Returns whether the point survives every active scissor preceding a region.
pub(crate) fn unclipped(
    list: &DrawList,
    at: usize,
    geometry: &TextGeometry,
    point: [f32; 2],
) -> bool {
    let slot = list.draws[at].slot;
    // Surface clipping applies even to Top; captured drags bypass picking instead.
    if !geometry.viewport.contains(point) {
        return false;
    }
    if slot != ZSlot::Top && !geometry.grid.contains(point) {
        return false;
    }
    let mut closed = 0usize;
    for draw in list.draws[..at].iter().rev().filter(|d| d.slot == slot) {
        match draw.primitive {
            Primitive::ClipPop => closed += 1,
            Primitive::ClipPush(_) if closed > 0 => closed -= 1,
            // An unresolved clip hides descendants just as an empty painter scissor does.
            Primitive::ClipPush(rect)
                if !placed(rect, draw, geometry).is_some_and(|r| r.contains(point)) =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}
