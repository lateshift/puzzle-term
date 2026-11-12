//! Kitty placeholders reference: <https://sw.kovidgoyal.net/kitty/graphics-protocol/>.
//! Kitty Unicode placeholders are ordinary VT text with raw colour identifiers.
//! Only rows marked by VT printing are visited; image-free grids have no cell scan.
//!
//! A virtual placement is a reusable image prototype. Its visible instances are
//! ordinary text cells, so VT editing moves them without graphics commands.
//! Colours carry wire identifiers rather than resolved ink: palette changes,
//! bold-bright policy and contrast correction must not select another image.
//! Combining marks carry table indices, not their Unicode numeric values.
//! Row/column/high-byte omission is decoded in display order before the owner
//! seals image sources alongside the snapshot's text rows (D-50).
//! No source lookup, font fallback or glyph preparation happens in this decoder.
//! The same walk serves source-admission visibility and snapshot publication.
//! Reference: https://sw.kovidgoyal.net/kitty/graphics-protocol/#unicode-placeholders.
use crate::vt::{
    cell::PackedColor,
    row::{Row, RowFlags},
    view::CellText,
};
/// Exact protocol order, independent of the current Unicode version.
mod diacritics;
/// A cell's source identity and zero-based position in a virtual placement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Tile {
    /// Full 32-bit image ID, before palette or theme colour resolution.
    pub image: u32,
    /// Underline colour selects a placement; zero selects any prototype.
    pub placement: u32,
    /// Row and column in the prototype rectangle.
    pub position: [u32; 2],
    /// Raw tags matter for the protocol's left-cell inheritance equality.
    colours: [PackedColor; 2],
}
/// Tests the base scalar only; malformed or unknown placeholders still have no glyph.
pub(crate) fn is_placeholder(text: CellText<'_>) -> bool {
    // Scalar cells need no cluster storage; marks turn the same base into a cluster.
    match text {
        CellText::Scalar(ch) => ch as u32 == crate::vt::Cell::IMAGE_PLACEHOLDER,
        CellText::Cluster(text) => text.starts_with('\u{10eeee}'),
        CellText::Empty => false,
    }
}
/// Tagged RGB encodes 24 ID bits; indexed colour encodes the index, never palette RGB.
fn identifier(colour: PackedColor) -> u32 {
    // The tag distinguishes indexed identifiers from 24-bit RGB identifiers.
    // Default colour has no identifier even when the theme resolves it to RGB.
    match colour.0 >> 24 {
        1 => colour.0 & 255,
        2 => colour.0 & 0xffffff,
        _ => 0,
    }
}
/// Decodes one cell, applying exactly the three left-to-right inheritance rules.
fn decode(cell: crate::frame_snapshot::SnapshotCell<'_>, left: Option<Tile>) -> Option<Tile> {
    if !is_placeholder(cell.text) {
        return None;
    }
    // SGR 59 removes the underline extra, which means unspecified placement zero.
    // Keep both packed tags as well as payloads for exact inheritance equality.
    let colours = [cell.raw.fg, cell.underline_color.unwrap_or_default()];
    // Option preserves omission: an explicit zero row is different from no row.
    let mut marks = [None; 3];
    if let CellText::Cluster(text) = cell.text {
        // Invalid marks cannot shift later diacritics into another wire field.
        for (slot, ch) in marks.iter_mut().zip(text.chars().skip(1)) {
            // A non-table mark invalidates this cell rather than borrowing a field.
            *slot = Some(diacritics::TABLE.binary_search(&(ch as u32)).ok()? as u32);
        }
    }
    // A colour change terminates inheritance, even if both resolve to equal ink.
    let previous = left.filter(|p| p.colours == colours);
    // Uninherited missing fields default to zero under the protocol.
    let mut row = marks[0].unwrap_or(0);
    let mut column = marks[1].unwrap_or(0);
    let mut high = marks[2].unwrap_or(0);
    if let Some(previous) = previous {
        // Checked advancement also keeps malicious text from wrapping an index.
        let adjacent = previous.position[1].checked_add(1)?;
        // No marks inherit all fields; one row mark inherits the remainder only
        // for the same row; two marks inherit the high byte only if adjacent.
        // No row mark means the entire positional prefix is inherited.
        if marks[0].is_none() {
            row = previous.position[0];
        }
        // A changed explicit row starts at column zero rather than extending left.
        if marks[1].is_none() && row == previous.position[0] {
            column = adjacent;
        }
        // The high byte follows only a consecutive tile in the same source row.
        if marks[2].is_none() && row == previous.position[0] && column == adjacent {
            high = previous.image >> 24;
        }
    }
    // The table has more than 256 entries, but an image ID has only eight high bits.
    if high > 255 {
        return None;
    }
    Some(Tile {
        // Indexed/RGB provides the low bits; the third mark supplies bits 24..31.
        image: identifier(colours[0]) | (high << 24),
        placement: identifier(colours[1]),
        position: [row, column],
        colours,
    })
}
/// Walks only placeholder-bearing rows, resetting inheritance at every non-placeholder.
pub(crate) fn visit(row: &Row, mut tile: impl FnMut(usize, Tile)) {
    // The sticky VT summary can be a false positive after partial erase.
    // It can never hide a live placeholder, including after reflow or cluster join.
    if !row.flags.contains(RowFlags::IMAGE_PLACEHOLDERS) {
        return;
    }
    // Row boundaries are inheritance boundaries even across a soft wrap.
    let mut left = None;
    for column in 0..row.cells.len() {
        // A blank, ordinary scalar or malformed cluster resets the left state.
        // cell_from_row respects lazy cleared tails and sparse cluster metadata.
        left =
            crate::frame_snapshot::cell_from_row(row, column).and_then(|cell| decode(cell, left));
        if let Some(value) = left {
            tile(column, value);
        }
    }
}
#[cfg(test)]
mod tests;

/// Prototype lookup is lazy: ordinary image placements never scan text cells.
impl crate::images::Store {
    /// Zero placement ID lets the terminal choose any matching prototype.
    pub(crate) fn virtual_placement(
        &self,
        tile: Tile,
    ) -> Option<(&crate::images::Placement, &crate::images::Image)> {
        // A retransmitted wire ID selects the new immutable source generation.
        let image = self
            .images
            .iter()
            .find(|image| image.id == tile.image && tile.image != 0)?;
        // Only virtual prototypes participate; physical placements cannot supply tiles.
        // Placement zero is intentionally terminal-selected, not an exact-ID query.
        let placement = self.placements.iter().find(|p| {
            p.virtual_
                && p.image == image.generation
                && (tile.placement == 0 || p.id == tile.placement)
        })?;
        Some((placement, image))
    }
}
/// Admission must pin a virtual source only while its actual text tiles are visible.
pub(crate) fn visible(
    store: &crate::images::Store,
    placement: &crate::images::Placement,
    term: &crate::vt::Terminal,
) -> bool {
    // Only the currently displayed rows pin storage; retained history is evictable.
    let (top, end) = store.visible_rows(term);
    for row in top..end {
        let Some(row) = term.screen().ring.retained(row) else {
            continue;
        };
        // Different prototypes may share an image. Compare the selected prototype
        // identity so an unspecified placement cannot pin every alternative.
        let mut found = false;
        visit(row, |_, tile| {
            if let Some((chosen, _)) = store.virtual_placement(tile) {
                // Out-of-range tiles render blank and must not pin image memory.
                found |= chosen.anchor == placement.anchor
                    && tile.position[0] < placement.cells[1]
                    && tile.position[1] < placement.cells[0];
            }
        });
        if found {
            return true;
        }
    }
    false
}
