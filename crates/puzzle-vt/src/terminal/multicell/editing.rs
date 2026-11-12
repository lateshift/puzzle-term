//! Rectangular editing is resolved through each affected row's sparse index.
//! Intact single-row blocks shift with ICH/DCH; multi-row suffixes are removed.
//! Region operations remove only blocks cut by a moving boundary, then row
//! handles carry every intact fragment without rewriting historical anchors.
//!
//! ECH/EL/ED use half-open column intersections and erase a touched block wholly.
//! Selective erase takes protection from any fragment, because every cell carries
//! the owner's DECSCA state. Lower-row reservations are not independently editable.
//! ICH/DCH can translate a one-row block only if both splice boundaries preserve it.
//! IL/DL/SU/SD first remove split records, then reuse ordinary handle rotation.
//! The primary top-origin/full-width case retains outgoing rows in history, so
//! its survival rule differs from a region whose outgoing rows are discarded.
//! No block-store iteration occurs in production scrolling; only row indexes
//! at the affected boundaries/band are consulted before the ordinary operation.
use super::*;
/// Indexed intersection rules run before ordinary row splices or handle moves.
impl Terminal {
    /// Erases every intersecting record as a unit, honoring DECSCA protection.
    pub(crate) fn erase_multicell_span(
        &mut self,
        y: usize,
        start: usize,
        end: usize,
        selective: bool,
    ) {
        let Some(row) = self.screen().row(y) else {
            return;
        };
        // A row without blocks stays on the existing bulk edit path.
        if !row.flags.contains(RowFlags::MULTICELLS) {
            return;
        }
        // Collect keys before mutation: clearing one rectangle changes indexes
        // on other affected rows, so borrowing an index across removal is invalid.
        let keys: smallvec::SmallVec<[u32; 4]> = row
            .blocks
            .iter()
            .filter_map(|reference| {
                let block = self.screen().ring.multicell(reference.key)?;
                // Protection was copied to all covered cells at admission; any
                // retained fragment can answer selective erase as a single unit.
                let protected = row
                    .cells
                    .get(block.column)
                    .is_some_and(|cell| cell.flags.contains(CellFlags::PROTECTED));
                // Half-open intervals include a block touched by even one column.
                // Selective erase leaves protected blocks and all their cells intact.
                (block.column < end
                    && block.column + block.columns > start
                    && !(selective && protected))
                    .then_some(reference.key)
            })
            .collect();
        // Removal damages every fragment, including rows outside this erase span.
        for key in keys {
            self.remove_multicell(key);
        }
    }
    /// Removes cut blocks and updates intact one-row coordinates before cell splicing.
    pub(crate) fn shift_multicells(
        &mut self,
        start: usize,
        right: usize,
        count: usize,
        insert: bool,
    ) {
        let Some(row) = self.screen().row(self.cursor.row) else {
            return;
        };
        if !row.flags.contains(RowFlags::MULTICELLS) {
            return;
        }
        // Only references are copied. Payload strings remain in the store until
        // an actual removal, and intact one-row records merely change column.
        let references = row.blocks.clone();
        // Progress is the finite snapshot iterator, not mutable cursor motion.
        // Each old key is consumed once; erasure/translation cannot enqueue it.
        for reference in references {
            let Some(block) = self.screen().ring.multicell(reference.key) else {
                continue;
            };
            // Blocks wholly before/after the horizontal edit retain their coordinates.
            if block.column > right || block.column + block.columns <= start {
                continue;
            }
            let end = block.column + block.columns;
            // A splice cannot leave half a rectangle. Multi-row suffixes are
            // removed because this edit moves only the addressed physical row.
            let cut = block.fragments.len() > 1
                || block.column < start
                || end > right + 1
                || if insert {
                    end + count > right + 1
                } else {
                    block.column < start + count
                };
            if cut {
                // Erasure precedes the ordinary splice, so no dangling key can move.
                self.remove_multicell(reference.key);
            } else if let Some(store) = &mut self.screen_mut().ring.multicells
                && let Some(block) = store.get_mut(reference.key)
            {
                // Update the sparse coordinate once; the following cell splice
                // moves its witnesses and preserves the row-local key index.
                block.column = if insert {
                    block.column + count
                } else {
                    block.column - count
                };
                // A cached block transform must observe its new horizontal origin.
                block.revision = block.revision.saturating_add(1);
            }
        }
    }
    /// Eliminates rectangles split by a moving region or its discarded edge.
    pub(crate) fn prepare_multicell_scroll(
        &mut self,
        top: usize,
        bottom: usize,
        count: usize,
        upward: bool,
    ) {
        // No feature allocation or historical scan exists before first use.
        if self.screen().ring.multicells.is_none() {
            return;
        }
        let (width, height) = self.size();
        // Horizontal margins copy a rectangle of cells rather than whole rows;
        // any intersecting multi-cell text must be removed before that copy.
        let whole_width = self.margins.left == 0 && self.margins.right + 1 == width;
        // Only the primary screen with top=0 and full width retains outgoing rows.
        let history =
            upward && top == 0 && whole_width && self.active == super::super::ActiveScreen::Primary;
        // Full-screen upward scrolling moves row handles into history intact.
        // Retirement already detaches only the evicted row's sparse witnesses;
        // visiting every surviving block here would make each LF a screen scan.
        if history && bottom + 1 == height {
            return;
        }
        let mut remove = smallvec::SmallVec::<[u32; 8]>::new();
        // Partial regions can split rectangles at either moving/discarded edge.
        // Deduplicate keys because up to seven rows can name the same record.
        for y in top..=bottom {
            let Some(row) = self.screen().row(y) else {
                continue;
            };
            for reference in &row.blocks {
                let Some(block) = self.screen().ring.multicell(reference.key) else {
                    continue;
                };
                // Every fragment must undergo the same whole-row movement.
                // Horizontal margins cannot move a rectangular record atomically.
                let survives = whole_width
                    && self.multicell_survives_scroll(block, top, bottom, count, upward, history);
                // A key occurs in several row indexes, but is erased exactly once.
                if !survives && !remove.contains(&reference.key) {
                    remove.push(reference.key);
                }
            }
        }
        // All indexes have been borrowed and released before mutating any row.
        for key in remove {
            self.remove_multicell(key);
        }
    }
    /// Tests whether every retained fragment belongs to the same moving row band.
    fn multicell_survives_scroll(
        &self,
        block: &Multicell,
        top: usize,
        bottom: usize,
        count: usize,
        upward: bool,
        history: bool,
    ) -> bool {
        // Stable IDs are not physical y coordinates after region rotations. Resolve
        // only this block's at-most-seven rows, never the entire block table.
        block.fragments.iter().all(|fragment| {
            self.screen()
                .ring
                .index_of(fragment.line)
                .is_some_and(|index| {
                    // Negative y is retained history, not a wrapping unsigned row index.
                    let y = index as isize - self.screen().ring.scrollback_len() as isize;
                    if history {
                        // Top-origin scrolling preserves outgoing fragments in history;
                        // only a fragment below the band's bottom would split the block.
                        y <= bottom as isize
                    } else if upward {
                        // The discarded leading edge consumes `count` rows. A record
                        // touching it is removed as a unit before row handles rotate.
                        y >= (top + count) as isize && y <= bottom as isize
                    } else {
                        // Downward movement discards the corresponding trailing edge.
                        // Records above the region must not acquire a moved lower half.
                        y >= top as isize && y + count as isize <= bottom as isize
                    }
                })
        })
    }
}
