//! Opt-in bulk rectangular metadata never exposes a slot as a Unicode scalar.
//!
//! Bulk buffers retain their ordinary structure-of-arrays contract. A root uses
//! the existing CLUSTER sentinel and cluster side table; continuation cells use
//! zero width and zero code point. Thus a caller that did not request the optional
//! block table cannot accidentally render a private slot number as Unicode.
//!
//! The optional record table adds dimensions, scaling and original fragment
//! offsets for clients that understand rectangles. It is copied once per record
//! across all requested rows. Both tables are caller-owned cold query results;
//! neither is built during ordinary frame capture or terminal output.
//! Inline colours and attributes remain per physical cell, preserving selection
//! and style inspection even when the cell contributes no independent text.
use super::{CellBlock, CellFlags, CellText, LineView};

/// Only flagged rows take this decoder; ordinary primitive transfer is unchanged.
impl CellBlock {
    /// Appends semantic cells and copies each requested record at most once.
    pub(super) fn append_multicell_row(&mut self, line: LineView<'_>) {
        if let Some(records) = &mut self.multicells {
            for (key, record) in line.multicells() {
                if !records.iter().any(|(old, _)| *old == key) {
                    // Record copies are opt-in and proportional to sparse blocks.
                    records.push((key, record.clone()));
                }
            }
        }
        for column in 0..line.cells().len() {
            let Some(cell) = line.cell(column) else {
                continue;
            };
            let offset = self.code_points.len();
            // Keys are internal handles. Only actual semantic text enters the
            // public code-point/cluster arrays, even if no sparse table is requested.
            self.code_points
                .push(if cell.raw.flags.contains(CellFlags::MULTICELL) {
                    if cell.width == 0 {
                        0
                    } else {
                        crate::Cell::CLUSTER
                    }
                } else {
                    cell.raw.ch
                });
            self.widths.push(cell.width);
            // Paint tags stay unresolved so a consumer can apply the current theme.
            self.foregrounds.push(cell.raw.fg.0);
            self.backgrounds.push(cell.raw.bg.0);
            self.attributes.push(cell.raw.attrs.bits());
            self.link_ids.push(
                line.row
                    .extras
                    .iter()
                    .find(|extra| extra.column == column)
                    .and_then(|extra| extra.link_id),
            );
            if let CellText::Cluster(text) = cell.text {
                self.clusters.push((offset, text.to_owned()));
            }
        }
    }
}
