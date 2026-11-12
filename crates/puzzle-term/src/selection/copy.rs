//! Clipboard representation is explicit and always crosses ClipboardWrite policy.
//!
//! Copy reads the already expanded tracked cells. Command-output copy reads
//! OSC 133 boundaries without replacing the user’s selection. No copy request
//! mutates terminal cells, the application cursor or viewport position.
//! Explicit command IDs are resolved at copy time, after normal command policy.
//! Both boundaries must still be retained and belong to that same command.
//! An empty completed interval is valid; an evicted interval is absent.
//! Half-open C/D endpoints retain the final newline without painting the prompt.
//! No block-copy request registers new selection owners or changes its highlight.
use super::SelectionState;
use crate::vt::{
    Terminal,
    selection::SelectPoint,
    view::{CellRange, Format},
    zones::ZoneKind,
};

/// Source and format requested by a user shortcut or API command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyTarget {
    /// Current tracked selection in the named serializer representation.
    Selection(Format),
    /// Most recent completed OSC 133 output as plain text.
    CommandOutput,
    /// Explicit retained OSC identity selected by a presented command-block button.
    ZoneOutput(u64),
}

/// Extracts only at explicit copy time; frame reads keep borrowing a small range.
impl SelectionState {
    /// Returns None when this source cannot act, preserving performable passthrough.
    pub(crate) fn copy(&self, term: &Terminal, target: CopyTarget) -> Option<String> {
        // Source availability is tested without extracting or changing any cells.
        // A missing tracked range makes a copy request inert.
        match target {
            CopyTarget::Selection(format) => {
                let range = self.range()?;
                // Normal ranges can have partial outer rows; blocks reuse min/max columns.
                let (first, end) = if range.block {
                    (
                        range.start.column.min(range.end.column),
                        range.start.column.max(range.end.column) + 1,
                    )
                } else {
                    (range.start.column, range.end.column + 1)
                };
                // Convert inclusive painted endpoints once to half-open copy bounds.
                // Wide spacers remain in the bounds but the serializer skips their text.
                // Each format reads the same cells and current trim preference.
                Some(term.buffer().serialize_cells(
                    CellRange {
                        start: (range.start.line, first),
                        end: (range.end.line, end),
                        block: range.block,
                        trim_trailing: self.trim_trailing,
                        // A hard-break gap or OSC D newline is outside the painted cells.
                        final_newline: self.final_newline,
                    },
                    format,
                ))
            }
            CopyTarget::ZoneOutput(id) => {
                // Identity lookup cannot redirect a stale click to the newest command.
                let view = term.buffer();
                let start = view
                    .zones()
                    .find(|mark| mark.id == id && mark.kind == ZoneKind::Output)?;
                let end = view
                    .zones()
                    .find(|mark| mark.id == id && mark.kind == ZoneKind::Finished)?;
                ((start.line, start.column) <= (end.line, end.column)).then(|| {
                    view.text_span_options(
                        (start.line, start.column),
                        (end.line, end.column),
                        self.trim_trailing,
                    )
                })
            }
            CopyTarget::CommandOutput => {
                // Command output is independent of the user’s current highlight.
                // Unfinished output cannot replace the most recent completed pair.
                let (start, end) = latest_output(term)?;
                // C/D are already half-open cursor boundaries, including a final hard break.
                Some(term.buffer().text_span_options(
                    (start.line, start.column),
                    (end.line, end.column),
                    self.trim_trailing,
                ))
            }
        }
    }
}

/// Finds the newest finished C/D pair without registering any new tracked owners.
pub(crate) fn latest_output(term: &Terminal) -> Option<(SelectPoint, SelectPoint)> {
    // One ordered pass pairs C/D; an intervening prompt or C invalidates an open C.
    let mut pending = None;
    let mut latest = None;
    // Zone iteration resolves tracked handles; evicted marks are already absent.
    // Ordered boundaries avoid a backwards scan for every completed command.
    for mark in term.buffer().zones() {
        let point = SelectPoint {
            line: mark.line,
            column: mark.column,
        };
        // A fresh C supersedes an incomplete output; A starts another prompt.
        // B only ends prompt text and does not itself complete command output.
        match mark.kind {
            ZoneKind::Output => pending = Some(point),
            ZoneKind::Prompt => pending = None,
            ZoneKind::Finished
                if pending.is_some_and(|start| {
                    (start.line, start.column) <= (point.line, point.column)
                }) =>
            {
                // Equal C/D positions represent a valid command with empty output.
                // Consume C so a second D cannot duplicate the same completion.
                latest = pending.take().map(|start| (start, point));
            }
            _ => {}
        }
    }
    latest
}
