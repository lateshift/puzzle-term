//! Converts half-open text-view gestures without changing inclusive API selections.
//!
//! A boundary at column zero excludes that row; its predecessor owns the newline.
//! A width-column start paints no cells on its row but still copies the hard break.
//! Keeping the existing engine retains output/reflow validation and copy formats.
//!
//! Public Range requests still mean inclusive cells. Between requests preserve
//! the pointer's gap semantics in recordings and interceptors instead of making
//! policy depend on hidden gesture state. Only this conversion chooses an
//! inclusive end; the VT engine still owns tracking and rewrite invalidation.
//!
//! Column width is an empty, clipped row-end sentinel. It is necessary for a
//! newline-only selection: moving the start to the following row would lose
//! that newline, while moving it left would select the last printed character.
//! Rich copy uses the same final-newline flag as existing semantic selections.
use super::{SelectPoint, SelectionCommand, SelectionKind, SelectionState, Terminal};

/// Pointer boundaries are normalized once, before ordinary expansion and tracking.
impl SelectionState {
    /// Equal gaps clear; missing rows and oversized gaps preserve the old selection.
    pub(super) fn between(&mut self, term: &mut Terminal, a: SelectPoint, b: SelectPoint) -> bool {
        // Validate both untrusted gaps before replacing any registered owner.
        if a.column > term.size().0
            || b.column > term.size().0
            || term.buffer().line(a.line).is_none()
            || term.buffer().line(b.line).is_none()
        {
            return false;
        }
        // Motion within the same half cell is empty, rather than a one-cell copy.
        if a == b {
            return self.apply(term, SelectionCommand::Clear);
        }
        // Direction changes the moving edge, never the traversal order of copy.
        let (start, mut end) = if (a.line, a.column) < (b.line, b.column) {
            (a, b)
        } else {
            (b, a)
        };
        let mut newline = false;
        if end.column == 0 {
            // Retained order, not ID subtraction, follows partial-region rotations.
            let ring = &term.screen().ring;
            let Some(row) = ring
                .index_of(end.line)
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| ring.retained(i))
            else {
                return false;
            };
            end.line = row.id;
            end.column = term.size().0;
            newline = term
                .buffer()
                .line(end.line)
                .is_some_and(|line| !line.wrapped());
        } else {
            // Inclusive paint/copy ends immediately before the moving gap.
            end.column -= 1;
        }
        // The right edge of a soft wrap and the next row's left edge are the
        // same logical gap: no character or hard break was crossed.
        if start == end && end.column == term.size().0 && !newline {
            return self.apply(term, SelectionCommand::Clear);
        }
        // A copy boundary can change even when its clipped paint range does not.
        let previous_newline = self.final_newline;
        let changed = self.apply(
            term,
            SelectionCommand::Range {
                kind: SelectionKind::Simple,
                start,
                end,
            },
        );
        // A row-end sentinel is clipped by drawing/extraction; only a hard break
        // contributes the extra newline when the next row's gap is at zero.
        self.final_newline = newline;
        changed || previous_newline != newline
    }
}
