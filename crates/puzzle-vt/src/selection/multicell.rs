//! Selection validation hashes logical blocks, never mutable slot identities.
//! Physical lower reservations add neither text nor false hard boundaries.
//!
//! Selection stores tracked physical endpoints, whereas a rectangle is one
//! logical text item. A resize can move those endpoints to different rows and
//! can compact private slot numbers. Neither change means the selected text was
//! edited. The fingerprint therefore hashes payload, sizing and rendition, never
//! a slot number or fragment identity.
//!
//! The same payload may intersect several selected rows. `first_intersection`
//! chooses the earliest selected fragment without allocating a seen-key set on
//! every selection validation. A rectangular selection can start below the root;
//! its first selected fragment still contributes the complete payload once.
//!
//! Untouched default cells carry no semantic witness. Their quantity changes
//! when the grid width changes, so hashing them would invalidate an unchanged
//! selection during reflow. Printed spaces and styled erasures are different:
//! they are content and retain their style in the fingerprint.
//!
//! Normal selection expands over indivisible rectangles. Expanding one endpoint
//! can expose another block, so the bounds reach a fixed point. This is a cold
//! gesture operation over sparse row indexes; it never scans every cell during
//! ordinary output. Monotonic growth and finite retained rows guarantee completion.
use super::*;
use crate::{CellFlags, view::LineView};

/// Tests whether this row is the first selected intersection of a rectangle.
fn first_intersection(
    block: &crate::multicell::Multicell,
    line: LineView<'_>,
    first: SelectPoint,
    last: SelectPoint,
) -> bool {
    // Fragments are ordered independently of numeric IDs after region moves.
    for fragment in &block.fragments {
        if fragment.line == line.id() {
            // No earlier selected intersection was found before this fragment.
            return true;
        }
        if fragment.line < first.line || fragment.line > last.line {
            // Retained fragments outside the selection do not consume its payload.
            continue;
        }
        let begin = if fragment.line == first.line {
            first.column
        } else {
            0
        };
        let end = if fragment.line == last.line {
            // Public selection endpoints are inclusive; intersection uses an
            // exclusive horizontal edge to exclude a neighbour at the boundary.
            last.column + 1
        } else {
            usize::MAX
        };
        if begin < block.column + block.columns && end > block.column {
            // Half-open horizontal intersection excludes a merely adjacent cell.
            return false;
        }
    }
    false
}

/// Produces the same semantic/style witness before and after rectangular reflow.
pub(super) fn fingerprint(term: &Terminal, first: SelectPoint, last: SelectPoint) -> Option<u64> {
    // Hashing is a stability witness, not a serialized representation or identity.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for line in term.buffer().lines(first.line..=last.line) {
        let begin = if line.id() == first.line {
            first.column
        } else {
            0
        };
        let end = if line.id() == last.line {
            last.column + 1
        } else {
            line.cells().len()
        };
        let mut column = begin;
        // Advance by an entire footprint after a block; repeating its payload
        // hash for each physical column would make width changes invalidate copy.
        while column < end.min(line.cells().len()) {
            let cell = line.cell(column)?;
            if let Some(block) = line.multicell(column) {
                if first_intersection(block, line, first, last) {
                    block.text.hash(&mut hasher);
                    block.sizing.hash(&mut hasher);
                    block.columns.hash(&mut hasher);
                    // Coordinate/key changes are relocations, not text edits.
                    hash_style(cell, &mut hasher);
                }
                column = block.column + block.columns;
                // A continuation has no independent Unicode value to hash.
                continue;
            }
            column += 1;
            if cell.width == 0 || cell.raw == crate::Cell::default() {
                // Wide spacers and untouched padding add no logical text witness.
                continue;
            }
            cell.text.hash(&mut hasher);
            // Keep scalar versus printed-space representation distinguishable.
            cell.raw.ch.hash(&mut hasher);
            hash_style(cell, &mut hasher);
        }
        if !line.continues() && !line.reservation_only() && line.id() != last.line {
            // Geometry-only rows must not create a new logical line witness.
            '\n'.hash(&mut hasher);
        }
    }
    Some(hasher.finish())
}

/// Covers visible rendition while excluding a block's private numeric handle.
fn hash_style(cell: crate::view::CellView<'_>, hasher: &mut impl Hasher) {
    // Raw color tags preserve changes between theme defaults, palette and RGB.
    cell.raw.fg.0.hash(hasher);
    cell.raw.bg.0.hash(hasher);
    cell.raw.attrs.bits().hash(hasher);
    (cell.raw.flags & !CellFlags::HAS_EXTRAS)
        .bits()
        .hash(hasher);
    cell.underline_color.map(|color| color.0).hash(hasher);
    // A URL change is a semantic edit even when its visible text is identical.
    cell.link.hash(hasher);
}

/// Closes a normal interval over every intersected rectangle, including middle rows.
/// Each iteration can only grow the bounds, so a finite retained document terminates.
pub(super) fn expand(
    term: &Terminal,
    mut first: SelectPoint,
    mut last: SelectPoint,
) -> (SelectPoint, SelectPoint) {
    loop {
        let before = (first, last);
        // Compare complete bounds after the pass rather than stopping when one
        // block is unchanged: a later block can expose an earlier neighbour.
        // Each pass starts with the interval discovered by the preceding pass.
        for line in term.buffer().lines(first.line..=last.line) {
            let begin = if line.id() == first.line {
                first.column
            } else {
                0
            };
            let end = if line.id() == last.line {
                last.column + 1
            } else {
                term.size().0
            };
            for (_, block) in line.multicells() {
                if begin >= block.column + block.columns || end <= block.column {
                    continue;
                }
                let Some(root) = block.fragments.first() else {
                    // Empty records are invalid in debug, but a contents query
                    // still avoids panicking if it encounters one in production.
                    continue;
                };
                let Some(bottom) = block.fragments.last() else {
                    continue;
                };
                if (root.line, block.column) < (first.line, first.column) {
                    // Include upper fragments even when the gesture hit a lower one.
                    first = SelectPoint {
                        line: root.line,
                        column: block.column,
                    };
                }
                if (bottom.line, block.column + block.columns - 1) > (last.line, last.column) {
                    // The inclusive endpoint covers the rectangle's last column.
                    last = SelectPoint {
                        line: bottom.line,
                        column: block.column + block.columns - 1,
                    };
                }
            }
        }
        if before == (first, last) {
            // All intersected footprints now fit inside the normalized interval.
            return (first, last);
        }
    }
}
