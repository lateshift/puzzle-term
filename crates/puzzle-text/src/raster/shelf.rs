//! Fixed-height native shelves keep short glyphs out of taller rows (D-47).
//!
//! Browser admission retains its existing monotonic shelf coordinates. Native
//! pages may append to any suitable shelf, choosing the smallest height that fits.
//! Every shelf has one fixed y/height and a monotonically growing x: no existing
//! address moves. Authored images retain transparent gutters for interpolation;
//! outline-derived grid masks are never transformed and need no empty border.
//! Shelf count is bounded by the page height and each row's positive height.
//! A warm glyph lookup does not traverse or allocate this cold insertion state.
//! A failed insertion changes no shelf, allowing another page to be tried safely.
use super::{AtlasRect, PAGE, Page};

/// One occupied row with immutable vertical bounds and a reusable horizontal tail.
pub(super) struct Shelf {
    /// Next unused column includes each previous crop's transparent border.
    x: u32,
    /// Immutable row top in device texels.
    y: u32,
    /// Fixed row height selected when its first glyph arrives.
    height: u32,
}

/// Advances one shelf only after a complete rectangle fits.
impl Page {
    /// Authored images retain one border texel; untransformed grid masks need none.
    pub(super) fn reserve(
        &mut self,
        width: u32,
        height: u32,
        native: bool,
        gutter: u32,
    ) -> Option<AtlasRect> {
        // Native complete builds use best-fit shelves; browser admission keeps its old addresses.
        if native {
            return self.native_reserve(width, height, gutter);
        }
        // Bilinear authored images need a border; nearest-only grid masks sample their crop exactly.
        // Padding consumes budget only when the draw may interpolate beyond its crop.
        let (w, h) = (width + 2 * gutter, height + 2 * gutter);
        let (mut x, mut y, mut shelf) = (self.x, self.y, self.height);
        // Starting a new shelf advances by the previous row's tallest crop plus its gutters.
        if x + w > PAGE {
            x = 0;
            y += shelf;
            shelf = 0;
        }
        // Refusal must leave the old cursor untouched so another page can accept this crop.
        if y + h > PAGE {
            return None;
        }
        let rect = AtlasRect {
            x: x + gutter,
            y: y + gutter,
            width,
            height,
        };
        // Commit coordinates only after both bounds passed; existing crops never move.
        self.x = x + w;
        self.y = y;
        self.height = shelf.max(h);
        Some(rect)
    }
}

impl Page {
    /// Uses the shortest fitting row, then appends one without repacking survivors.
    fn native_reserve(&mut self, width: u32, height: u32, gutter: u32) -> Option<AtlasRect> {
        let (w, h) = (width + 2 * gutter, height + 2 * gutter);
        // Best fit avoids reserving a CJK-height row for a punctuation-sized crop.
        let slot = self
            .shelves
            .iter()
            .enumerate()
            .filter(|(_, row)| row.height >= h && row.x + w <= PAGE)
            .min_by_key(|(_, row)| row.height)
            .map(|(i, _)| i);
        // An existing row contributes only its unused horizontal tail.
        let slot = if let Some(slot) = slot {
            slot
        } else {
            // New rows start below all fixed-height rows; variable glyph heights cannot overlap.
            let y = self.shelves.last().map_or(0, |row| row.y + row.height);
            if y + h > PAGE || w > PAGE {
                return None;
            }
            self.shelves.push(Shelf { x: 0, y, height: h });
            self.shelves.len() - 1
        };
        // Best-fit selects a row, not a compaction: prior handles remain valid at their UVs.
        let row = &mut self.shelves[slot];
        let rect = AtlasRect {
            x: row.x + gutter,
            y: row.y + gutter,
            width,
            height,
        };
        // Future insertions see the gutter as occupied, keeping neighboring ink independent.
        row.x += w;
        Some(rect)
    }
}
