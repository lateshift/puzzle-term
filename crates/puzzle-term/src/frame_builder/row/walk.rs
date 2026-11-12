//! Selects the row's image-placeholder path once, before visiting cells.
//! A const parameter removes placeholder inspection from ordinary text entirely.
//! VT sets the conservative row flag when storing U+10EEEE and preserves it
//! through snapshots/reflow. A false positive costs inspection for that row;
//! a row without the flag cannot contain an image placeholder.
//! Both paths still resolve paper, ordinary decorations and shaped text through
//! the same painter. The specialization changes work, never emitted geometry.
use super::{RowShape, cell};
use crate::{
    frame_builder::colors::ColorPolicy,
    frame_snapshot::{PaintState, cell_from_row},
};
use puzzle_text::metrics::CellMetrics;
use puzzle_vt::row::Row;

/// Walks physical cells without adding image work to placeholder-free rows.
pub(super) fn paint<const PLACEHOLDERS: bool>(
    shape: &mut RowShape<'_>,
    line: &Row,
    state: &PaintState,
    metrics: CellMetrics,
    policy: &ColorPolicy,
) -> Result<(), String> {
    for column in 0..line.cells.len() {
        // Sparse clusters remain borrowed; wide spacer ownership stays in VT.
        let visible = cell_from_row(line, column).ok_or("visible cell missing")?;
        cell::paint::<PLACEHOLDERS>(shape, state, column, visible, metrics, policy)?;
    }
    Ok(())
}
