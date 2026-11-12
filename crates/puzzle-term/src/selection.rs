//! Session-owned selection state shared by API and pointer gestures (§6.9).
//!
//! Expanded endpoints are registered once in VT's existing selection engine.
//! Frame assembly reads a small cached range, never scans selected text. Output
//! and reflow reconcile row versions before publishing any changed range. Copy
//! alone allocates the requested string; highlighting never copies text (P1).
//!
//! There is one active range per terminal. SelectionCommand retains gesture
//! intent until policy-approved dispatch; SelectionRange contains the expanded
//! cells after that dispatch. Keeping these separate lets a word click and a
//! line click use the same frame and extraction machinery as a simple drag.
//!
//! Validation covers every selected physical row, including rows reached by
//! logical-line expansion. Tracking only the clicked row would leave a stale
//! highlight after an application rewrites another part of the wrapped line.
//! The VT engine also records the screen incarnation, so switching away and
//! back within one output chunk still invalidates the old document selection.
//!
//! Reconciliation is source-driven, after output, resize or a direct clear.
//! It does not run per display refresh. After a reflow the VT engine rebases
//! its version witnesses to the newly created physical rows. The cached range
//! then supplies rendering and scrollbar markers without inspecting content.
//! A gone endpoint clears the whole selection rather than clamping to new text.
//!
//! Rectangular extraction uses the same column interval on each physical row.
//! Linear extraction uses partial first/last rows and full intervening rows.
//! Soft-wrap continuity is delegated to BufferView so the external contents
//! API and selection copy do not develop different newline or Unicode rules.
//! A completed command can have a final hard newline beyond its last cell.
//! That copy fact is stored separately from the inclusive painted range.

/// Styled clipboard extraction reuses VT’s serializer helpers.
mod copy;
/// Half-open pointer gaps become the existing tracked inclusive copy range.
mod gaps;
use crate::vt::{
    Terminal,
    selection::{SelectPoint, Selection, SelectionKind, expanded, output_span},
    tracked::LineId,
    view::TextOptions,
};
pub use copy::CopyTarget;
pub(crate) use copy::latest_output;

/// Inclusive cell range after gesture expansion, ready for copy and rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionRange {
    /// First cell in document order; width denotes an empty row-end boundary.
    pub start: SelectPoint,
    /// Last selected cell; width is a clipped row-end sentinel for a hard-break gap.
    pub end: SelectPoint,
    /// Rectangular columns are reused on every physical row.
    pub block: bool,
}
/// Maps the same expanded range to every visible physical row.
impl SelectionRange {
    /// Resolves one row to a half-open selected column interval.
    pub fn columns(self, line: LineId, width: usize) -> Option<(usize, usize)> {
        // The retained document interval excludes padding and unrelated rows.
        if line < self.start.line || line > self.end.line {
            return None;
        }
        // Rectangle endpoints may have reversed columns even after row ordering.
        let (first, end) = if self.block {
            (
                self.start.column.min(self.end.column),
                self.start.column.max(self.end.column) + 1,
            )
        } else {
            (
                if line == self.start.line {
                    self.start.column
                } else {
                    0
                },
                if line == self.end.line {
                    self.end.column + 1
                } else {
                    width
                },
            )
        };
        // Clamp against the current grid width after a source-driven resize.
        (first < end.min(width)).then_some((first, end.min(width)))
    }
}
/// Public requests preserve typed endpoints instead of copying buffer content.
#[derive(Clone, Copy, Debug)]
pub enum SelectionCommand {
    /// Simple text-view drag between half-open cell edges (columns may equal width).
    Between {
        /// Fixed text boundary, before or after an indivisible glyph.
        start: SelectPoint,
        /// Moving text boundary; equal endpoints clear the selection.
        end: SelectPoint,
    },
    /// Replace with an expanded range; invalid positions leave the old one intact.
    Range {
        /// Simple, word, logical line, block or OSC 133 semantic expansion.
        kind: SelectionKind,
        /// Gesture's fixed cell.
        start: SelectPoint,
        /// Gesture's moving cell, which may precede start.
        end: SelectPoint,
    },
    /// Select all retained rows of the active screen.
    All,
    /// Select the command under this cell, or the latest finished output.
    CommandOutput(Option<SelectPoint>),
    /// Release tracked handles and remove the highlight.
    Clear,
}
/// Mutable model belongs to exactly one Session, without shared ownership.
pub(crate) struct SelectionState {
    /// VT owns stability checks; the Session owns this engine's lifetime.
    engine: Option<Selection>,
    /// Frame and marker consumers read only this small reconciled value.
    range: Option<SelectionRange>,
    /// A semantic D or pointer gap at column zero includes the preceding hard break.
    final_newline: bool,
    /// Word boundaries are a user preference, supplied before expansion.
    separators: String,
    /// Explicit copy chooses whether padding spaces are retained.
    pub trim_trailing: bool,
    /// A release gesture copies only when the user enabled this option.
    pub copy_on_select: bool,
    /// Source-updated availability avoids scanning shell marks in idle pumps.
    pub output_available: bool,
}
/// Starts with no registered endpoints and the schema’s extraction defaults.
impl Default for SelectionState {
    /// Mirrors the schema defaults without starting any timers or work.
    fn default() -> Self {
        Self {
            engine: None,
            range: None,
            final_newline: false,
            separators: " `│|:\"'()[]{}<>,;".into(),
            trim_trailing: true,
            copy_on_select: false,
            output_available: false,
        }
    }
}
/// Owns replacement, source validation and explicit copy without frame-time scans.
impl SelectionState {
    /// Borrows the already validated range without touching VT rows.
    pub(crate) fn range(&self) -> Option<SelectionRange> {
        self.range
    }
    /// Applies live extraction/gesture preferences without mutating selected cells.
    pub(crate) fn configure(&mut self, separators: &str, trim: bool, copy: bool) {
        // Retain the preference allocation when settings are applied again.
        self.separators.clear();
        self.separators.push_str(separators);
        // Changing copy preferences does not invalidate the selected document cells.
        self.trim_trailing = trim;
        self.copy_on_select = copy;
    }
    /// Executes one accepted selection request; returns whether its range changed.
    pub(crate) fn apply(&mut self, term: &mut Terminal, command: SelectionCommand) -> bool {
        if let SelectionCommand::Between { start, end } = command {
            return self.between(term, start, end);
        }
        // Compare small range values so unchanged requests produce no fact.
        let before = self.range;
        if matches!(command, SelectionCommand::Clear) {
            self.clear(term);
        } else if let Some((kind, a, b)) = self.request_points(term, command) {
            // Gesture direction must not reverse the document traversal.
            let (a, b) = if (a.line, a.column) <= (b.line, b.column) {
                (a, b)
            } else {
                (b, a)
            };
            if let Some((start, end)) = expanded(term, kind, a, b, &self.separators) {
                // Capture versions over the expanded interval, not just the clicked row.
                let engine_kind = if kind == SelectionKind::Block {
                    kind
                } else {
                    SelectionKind::Simple
                };
                let accepted = if let Some(engine) = &mut self.engine {
                    engine.replace(term, engine_kind, start, end)
                } else {
                    self.engine = Selection::new(term, engine_kind, start, end);
                    self.engine.is_some()
                };
                // Invalid input preserves the prior interval and registered owners.
                if accepted {
                    // Paint excludes the next prompt, while copy retains its preceding hard break.
                    self.final_newline = kind == SelectionKind::Semantic
                        && output_span(term, a).is_some_and(|(_, end)| end.column == 0);
                    // Cache expanded endpoints once for all frame and marker consumers.
                    self.range = Some(SelectionRange {
                        start,
                        end,
                        block: kind == SelectionKind::Block,
                    });
                }
            }
        }
        before != self.range
    }
    /// Invalidates rewritten/evicted/screen-local text and rebases successful reflow.
    pub(crate) fn reconcile(&mut self, term: &mut Terminal) -> bool {
        // Compare small range values so unchanged requests produce no fact.
        let before = self.range;
        self.output_available = latest_output(term).is_some();
        if let Some(engine) = &mut self.engine {
            // VT remaps tracked cells and rebases row witnesses after valid reflow.
            if let Some((start, end)) = engine.refresh(term) {
                // Reflow changes coordinates while preserving the same selected characters.
                if let Some(range) = &mut self.range {
                    range.start = start;
                    range.end = end;
                }
            } else {
                // A rewritten or evicted row cannot remain highlighted as old content.
                self.clear(term);
            }
        }
        before != self.range
    }
    /// Copies an explicit selection request using the configured trim rule.
    pub(crate) fn text(&self, term: &Terminal) -> String {
        let Some(range) = self.range else {
            return String::new();
        };
        // Rectangular copy deliberately emits one newline per physical row.
        if range.block {
            return term.buffer().text(
                range.start.line..=range.end.line,
                TextOptions {
                    block: true,
                    trim_trailing: self.trim_trailing,
                    first_column: range.start.column.min(range.end.column),
                    end_column: Some(range.start.column.max(range.end.column) + 1),
                    ..TextOptions::default()
                },
            );
        }
        // Linear copy joins soft wraps and trims only actual hard line ends.
        let mut text = term.buffer().text_span_options(
            (range.start.line, range.start.column),
            (range.end.line, range.end.column + 1),
            self.trim_trailing,
        );
        // A gap or OSC D at the next row’s start includes the unpainted hard break.
        if self.final_newline {
            text.push('\n');
        }
        text
    }
    /// Releases registry handles before replacing the owned engine.
    fn clear(&mut self, term: &mut Terminal) {
        // Handles belong to VT’s registry and must be released before dropping the engine.
        if let Some(engine) = self.engine.take() {
            engine.clear(term);
        }
        // Remove the cached paint range together with its stability witnesses.
        self.range = None;
        self.final_newline = false;
    }
    /// Resolves whole-buffer and shell requests before registering any handles.
    fn request_points(
        &self,
        term: &Terminal,
        command: SelectionCommand,
    ) -> Option<(SelectionKind, SelectPoint, SelectPoint)> {
        match command {
            // Direct callers already supply typed cells; VT validates retention below.
            SelectionCommand::Range { kind, start, end } => Some((kind, start, end)),
            SelectionCommand::All => {
                // All includes retained history, not merely the current viewport.
                let ids = term.buffer().line_ids()?;
                Some((
                    SelectionKind::Simple,
                    SelectPoint {
                        line: *ids.start(),
                        column: 0,
                    },
                    SelectPoint {
                        line: *ids.end(),
                        column: term.size().0 - 1,
                    },
                ))
            }
            SelectionCommand::CommandOutput(point) => {
                // The latest C mark is sufficient; expansion requires its following D.
                let point = point.or_else(|| {
                    term.buffer()
                        .zones()
                        .filter(|mark| mark.kind == crate::vt::zones::ZoneKind::Output)
                        .last()
                        .map(|mark| SelectPoint {
                            line: mark.line,
                            column: mark.column,
                        })
                })?;
                Some((SelectionKind::Semantic, point, point))
            }
            SelectionCommand::Clear | SelectionCommand::Between { .. } => None,
        }
    }
}
