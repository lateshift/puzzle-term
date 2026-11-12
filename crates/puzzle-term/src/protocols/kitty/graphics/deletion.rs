//! Kitty deletion reference: <https://sw.kovidgoyal.net/kitty/graphics-protocol/>.
//! Kitty deletion selectors retire placements first, optionally unreferenced data.
//! Uppercase selectors free pixel sources only after their last placement is gone.
use super::control::Control;
use crate::images::{ImageError, Placement, Store};
use crate::vt::{Terminal, tracked::PositionState};

/// All delete variants use the same screen-local rectangle intersection rules.
// Deletion never invokes codecs; it only retires Session-owned sources and anchors.
impl Store {
    /// Deletes matching placements, leaving stored bytes for lowercase selectors.
    pub(crate) fn delete(
        &mut self,
        control: &Control,
        terminal: &mut Terminal,
    ) -> Result<(), ImageError> {
        let (selector, selection) = Selection::parse(self, control)?;
        let generation = selection.generation;
        // Capture handle identities before mutating the table. Selecting source
        // generations alone would accidentally erase sibling placements of it.
        let selected: Vec<_> = self
            .placements
            .iter()
            .filter(|placement| selection.matches(placement, self, terminal))
            .map(|p| (p.anchor, p.image))
            .collect();
        // Retire precisely selected instances; sibling placements may share pixels.
        self.placements.retain(|p| {
            let keep = !selected.iter().any(|(anchor, _)| *anchor == p.anchor);
            if !keep {
                terminal.tracked.release(p.anchor);
            }
            keep
        });
        // Uppercase means free only sources with no remaining placement, including
        // placements retained in history or on the other screen. Lowercase keeps
        // pixels available for a later put without retransmission.
        if selector.is_ascii_uppercase() {
            // Descending indices remain valid as remove_image shifts entries.
            // An explicitly selected image can be freed even if never placed.
            for index in (0..self.images.len()).rev() {
                let image = &self.images[index];
                if (selected
                    .iter()
                    .any(|(_, generation)| *generation == image.generation)
                    || generation == Some(image.generation))
                    && !self.placements.iter().any(|p| p.image == image.generation)
                {
                    self.remove_image(index, terminal);
                }
            }
        }
        // Removed images may have covered cells anywhere in the grid. Repainting
        // the retained text reveals it without fabricating application output.
        terminal.screen_mut().damage.full = true;
        Ok(())
    }
}

/// A validated deletion query is separate from the mutation that releases handles.
struct Selection {
    /// Lowercase selector; original case is used by the caller for source retirement.
    kind: u8,
    /// Explicit image identity after resolving the newest image number.
    generation: Option<u64>,
    /// Optional placement ID restricts image-ID and image-number queries.
    id: u32,
    /// One-based column, or inclusive start of an image-ID range.
    x: u32,
    /// One-based row, or inclusive end of an image-ID range.
    y: u32,
    /// Signed layer value for z and point-plus-z selectors.
    z: i32,
}

/// Matching reads the old table completely before any tracked position is released.
impl Selection {
    /// Validates the selector and freezes its target before any table mutation.
    fn parse(store: &Store, control: &Control) -> Result<(u8, Self), ImageError> {
        // Case selects pixel retirement, while the lowercase form selects the
        // same geometric predicate. Unsupported animation deletion is not ignored.
        let selector = control.letter(b'd', b'a')?;
        let kind = selector.to_ascii_lowercase();
        if !b"aincpqrxyz".contains(&kind) {
            return Err(ImageError::Unsupported);
        }
        // Resolve image numbers before matching: several images may share a
        // number, but delete-by-number refers only to the most recently created.
        let target = if b"in".contains(&kind) {
            Some(store.find(control)?)
        } else {
            None
        };
        // Image identity is frozen before the table is edited, so removals
        // cannot change which numbered source this command meant to select.
        let generation = target.map(|i| store.images[i].generation);
        // Point selectors use one-based cells. Zero is malformed rather than a
        // coordinate just outside the grid; ID-range endpoints are inclusive.
        let id = control.number(b'p', 0)?;
        let x = control.number(b'x', 0)?;
        let y = control.number(b'y', 0)?;
        let z = control.z()?;
        if b"pqxy".contains(&kind)
            && (b"pqx".contains(&kind) && x == 0 || b"pqy".contains(&kind) && y == 0)
            || kind == b'r' && x > y
        {
            return Err(ImageError::Invalid);
        }
        // Validation is complete before allocating the temporary match list.
        // The list belongs only to this cold command, never a per-frame path.
        let selection = Self {
            kind,
            generation,
            id,
            x,
            y,
            z,
        };
        Ok((selector, selection))
    }

    /// Tests source identity or screen-local geometry without changing recency.
    fn matches(&self, placement: &Placement, store: &Store, terminal: &Terminal) -> bool {
        // Identity selectors can find offscreen placements; point selectors
        // always use the active screen's current cell coordinate space.
        // Prototypes have no physical screen rectangle; only identity/range
        // selectors may delete them (kitty Unicode-placeholders section).
        if placement.virtual_ && !b"inr".contains(&self.kind) {
            return false;
        }
        match self.kind {
            b'i' | b'n' => {
                Some(placement.image) == self.generation
                    && (self.id == 0 || placement.id == self.id)
            }
            // Range deletion compares wire IDs rather than private generations.
            // A successful replacement therefore remains addressable by its ID.
            b'r' => store.images.iter().any(|image| {
                image.generation == placement.image && image.id >= self.x && image.id <= self.y
            }),
            // Layer deletion compares signed z values without narrowing them.
            // It does not infer a layer from the sign or collapse distinct z's.
            b'z' => placement.z == self.z,
            // A retained history anchor may have a negative screen-relative row.
            // Only a rectangle crossing the screen can satisfy visible deletion.
            _ => {
                placement.screen == terminal.active
                    && bounds(placement, terminal).is_some_and(|rect| match self.kind {
                        b'a' => rect[1] < terminal.size().1 as i64 && rect[3] > 0,
                        b'c' => contains(
                            rect,
                            terminal.cursor.column as i64,
                            terminal.cursor.row as i64,
                        ),
                        b'p' | b'q' => {
                            contains(rect, i64::from(self.x) - 1, i64::from(self.y) - 1)
                                && (self.kind != b'q' || placement.z == self.z)
                        }
                        b'x' => rect[0] < i64::from(self.x) && i64::from(self.x) - 1 < rect[2],
                        b'y' => rect[1] < i64::from(self.y) && i64::from(self.y) - 1 < rect[3],
                        _ => false,
                    })
            }
        }
    }
}

/// Screen-relative cell rectangle permits images anchored in retained history.
fn bounds(placement: &Placement, terminal: &Terminal) -> Option<[i64; 4]> {
    let PositionState::Present { line, column } = terminal.tracked.get(placement.anchor) else {
        return None;
    };
    // Resolve the retained row through its ring index. Region scrolling can
    // reorder line IDs, so subtraction of raw IDs would give incorrect cells.
    let ring = &terminal.screen().ring;
    let row = i64::try_from(ring.index_of(line)?).ok()? - ring.scrollback_len() as i64;
    // Subtracting the live scrollback length makes history rows negative.
    // Half-open ends let an image ending at the top edge count as offscreen.
    Some([
        column as i64,
        row,
        column as i64 + i64::from(placement.cells[0]),
        row + i64::from(placement.cells[1]),
    ])
}

/// Half-open cell rectangles match kitty's one-based point selectors after conversion.
fn contains(rect: [i64; 4], x: i64, y: i64) -> bool {
    // Coordinates exactly on the bottom/right boundary belong to the next
    // cell, matching how the placement rectangle is clipped during drawing.
    rect[0] <= x && x < rect[2] && rect[1] <= y && y < rect[3]
}
