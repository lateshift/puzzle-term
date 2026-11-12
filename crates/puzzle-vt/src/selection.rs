//! Selection anchors and copy rules on stable terminal lines (§6.9).
//!
//! Endpoints are registered with `TrackedPositions`, so scrolling changes
//! their visible coordinates without changing what text they name. A row
//! rewrite changes its version and invalidates the selection: copied text
//! must never silently become different output under the same highlight.
//! Screen switches invalidate it as well because primary and alternate
//! line IDs belong to different visual documents.
//!
//! The selection object does not copy text at creation. Its two endpoints
//! and captured row versions describe the user's intent while output can
//! keep flowing. Extraction happens only when copy is requested. This
//! matters for a large scrollback: keeping selected text in a second
//! buffer would duplicate memory and could become stale after edits.
//!
//! A soft-wrapped row continues the same logical line, while a hard row
//! break contributes a newline. Wide spacers are storage placeholders,
//! not characters. The copy path uses `BufferView`'s cell text to keep
//! combining clusters intact and to skip those placeholders.
use crate::{
    terminal::{ActiveScreen, Terminal},
    tracked::{LineId, PositionState, TrackedPos},
    view::TextOptions,
};
use std::hash::{Hash, Hasher};
/// Gesture expansion is shared by rendering and extraction.
mod bounds;
/// Rectangular payload identity survives coordinate and slot remapping.
mod multicell;
pub use bounds::{expanded, output_span};

/// User gesture that decides how endpoints expand before copy.
///
/// Expansion is delayed until extraction so a word or shell zone can be
/// interpreted with the same row contents that passed version validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionKind {
    /// Exact drag endpoints, ordered in document order.
    ///
    /// Shift-click uses this same kind with one endpoint held fixed.
    Simple,
    /// Double-click expands across non-separator characters.
    ///
    /// The user-configured separator set is supplied at extraction time.
    Word,
    /// Triple-click selects full soft-wrapped logical lines.
    ///
    /// A wrapped command is one line even when it occupies several rows.
    Line,
    /// Alt-drag keeps a rectangular column interval on each row.
    ///
    /// Its output always has one newline per physical row.
    Block,
    /// OSC 133 output range associated with a shell command.
    ///
    /// The `C` mark starts output; the following `D` mark ends it.
    Semantic,
}

/// One endpoint in retained line identity and zero-based column.
///
/// Identity is independent of viewport scrolling and row number.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectPoint {
    /// Stable row identity, not a viewport index.
    ///
    /// Eviction makes a tracked handle Gone instead of reusing this ID.
    pub line: LineId,
    /// Cell column inside the selected row.
    ///
    /// A wide spacer is never copied; width denotes a simple line-end boundary.
    pub column: usize,
}

/// Selection whose registered endpoints survive scroll and reflow.
///
/// Covered row versions detect an in-place rewrite that leaves IDs intact.
#[derive(Clone, Debug)]
pub struct Selection {
    /// Mouse or semantic expansion policy.
    ///
    /// A drag can change its endpoint without changing the kind.
    kind: SelectionKind,
    /// First endpoint handle in the terminal's shared registry.
    ///
    /// The registry remaps this handle during primary-screen reflow.
    start: TrackedPos,
    /// Last endpoint handle in the same registry.
    ///
    /// No viewport coordinate is stored in the selection itself.
    end: TrackedPos,
    /// Active screen at creation; an alternate switch clears meaning.
    ///
    /// An editor's alternate grid is a separate text document.
    screen: ActiveScreen,
    /// An out-and-back screen switch still invalidates this selection.
    screen_epoch: u64,
    /// Rewrites of any covered row make the selection stale.
    ///
    /// Scroll alone preserves versions and therefore preserves selection.
    versions: Vec<(LineId, u64)>,
    /// Last possible source stamp at capture; reflow inherits stamps unchanged.
    write_clock: u64,
    /// Width reflow remaps old row IDs without changing selected content.
    reflow_generation: u64,
    /// Selected text and cell style identity across physical rewrapping.
    fingerprint: u64,
}

impl Selection {
    /// Releases both anchors when the UI replaces or clears a selection.
    ///
    /// The registry generation changes so a copied stale handle cannot
    /// accidentally resolve a later search result in the same slot.
    pub fn clear(self, term: &mut Terminal) {
        // Cloned/stale handles cannot retire another selection's observation.
        if term.tracked.registered(self.start) {
            term.unregister_source_observer();
        }
        term.tracked.release(self.start);
        term.tracked.release(self.end);
    }

    /// Registers both endpoints and captures versions of covered rows.
    ///
    /// A missing line is rejected before creating either registry handle,
    /// so malformed API positions cannot leave partial tracking state.
    pub fn new(
        term: &mut Terminal,
        kind: SelectionKind,
        start: SelectPoint,
        end: SelectPoint,
    ) -> Option<Self> {
        let view = term.buffer();
        // Simple text-view ranges may include an empty boundary at row width;
        // reject larger columns before registering either long-lived owner.
        if invalid_columns(kind, start, end, view.size().0) {
            return None;
        }
        view.line(start.line)?;
        view.line(end.line)?;
        // The user can drag backward; version capture follows document
        // order while the underlying handles retain the gesture endpoints.
        let (first, last) = order(start, end);
        let fingerprint = selection_fingerprint(term, first, last)?;
        let versions = view
            .lines(first.line..=last.line)
            .map(|line| (line.id(), line.version()))
            .collect();
        let screen = term.active;
        // Reflow validation compares global stamps while this selection exists.
        term.register_source_observer();
        let start = term.tracked.track(start.line, start.column);
        let end = term.tracked.track(end.line, end.column);
        Some(Self {
            kind,
            start,
            end,
            screen,
            screen_epoch: term.screen_epoch,
            versions,
            reflow_generation: term.reflow_generation,
            write_clock: term.write_clock(),
            fingerprint,
        })
    }

    /// Replaces a drag interval while reusing its warmed row-version allocation (P1).
    pub fn replace(
        &mut self,
        term: &mut Terminal,
        kind: SelectionKind,
        start: SelectPoint,
        end: SelectPoint,
    ) -> bool {
        if invalid_columns(kind, start, end, term.size().0) {
            return false;
        }
        if term.buffer().line(start.line).is_none() || term.buffer().line(end.line).is_none() {
            return false;
        }
        let (first, last) = order(start, end);
        let Some(fingerprint) = selection_fingerprint(term, first, last) else {
            return false;
        };
        // Registry slots and version storage survive successive drag samples.
        term.tracked.release(self.start);
        term.tracked.release(self.end);
        self.start = term.tracked.track(start.line, start.column);
        self.end = term.tracked.track(end.line, end.column);
        self.kind = kind;
        self.screen = term.active;
        self.screen_epoch = term.screen_epoch;
        self.reflow_generation = term.reflow_generation;
        self.write_clock = term.write_clock();
        self.fingerprint = fingerprint;
        self.versions.clear();
        self.versions.extend(
            term.buffer()
                .lines(first.line..=last.line)
                .map(|line| (line.id(), line.version())),
        );
        true
    }

    /// Returns live endpoints only while the same content is retained.
    ///
    /// A row edit invalidates even a partial-row selection because a glyph
    /// or style change inside it can alter the user's intended copy.
    #[must_use]
    pub fn endpoints(&self, term: &Terminal) -> Option<(SelectPoint, SelectPoint)> {
        if term.active != self.screen || term.screen_epoch != self.screen_epoch {
            return None;
        }
        // The full covered interval matters: unchanged endpoints are not
        // enough if output rewrote a line between them.
        let start = resolve(term, self.start)?;
        let end = resolve(term, self.end)?;
        let ordered = order(start, end);
        if term.reflow_generation == self.reflow_generation {
            for &(id, version) in &self.versions {
                if term.buffer().line(id)?.version() != version {
                    return None;
                }
            }
        } else {
            // Reflow creates new IDs but retains source write stamps. Even an
            // identical rewrite before or after reflow exceeds the capture clock;
            // the fingerprint additionally checks the remapped logical extent.
            if term
                .buffer()
                .lines(ordered.0.line..=ordered.1.line)
                .any(|line| line.version() > self.write_clock)
                || selection_fingerprint(term, ordered.0, ordered.1)? != self.fingerprint
            {
                return None;
            }
        }
        Some(ordered)
    }

    /// Rebases validation after a successful reflow without reallocating rows.
    /// Source-driven Session reconciliation calls this before caching the range.
    pub fn refresh(&mut self, term: &Terminal) -> Option<(SelectPoint, SelectPoint)> {
        let points = self.endpoints(term)?;
        if self.reflow_generation != term.reflow_generation {
            // New physical row IDs replace old version witnesses after reflow.
            self.versions.clear();
            self.versions.extend(
                term.buffer()
                    .lines(points.0.line..=points.1.line)
                    .map(|row| (row.id(), row.version())),
            );
            self.reflow_generation = term.reflow_generation;
        }
        Some(points)
    }

    /// Extracts visible text by kind, excluding wide spacer columns.
    ///
    /// Returns `None` when a tracked line vanished or any selected row was
    /// rewritten, rather than returning text that no longer matches ink.
    #[must_use]
    pub fn text(&self, term: &Terminal, separators: &str) -> Option<String> {
        let (mut first, mut last) = self.endpoints(term)?;
        if self.kind == SelectionKind::Semantic {
            let (start, end) = output_span(term, first)?;
            return Some(
                term.buffer()
                    .text_span((start.line, start.column), (end.line, end.column)),
            );
        }
        let view = term.buffer();
        if self.kind == SelectionKind::Simple
            && let (Some(a), Some(b)) = (
                view.line(first.line)?.multicell(first.column),
                view.line(last.line)?.multicell(last.column),
            )
            && std::ptr::eq(a, b)
        {
            // A gesture entirely inside one indivisible item selects its payload,
            // not unrelated text to the right of its top physical fragment.
            return Some(a.text.clone());
        }
        (first, last) = expanded(term, self.kind, first, last, separators)?;
        if self.kind == SelectionKind::Block {
            // Rectangular selection keeps one physical row per output line;
            // its two columns form the same slice across every row.
            let options = TextOptions {
                block: true,
                first_column: first.column.min(last.column),
                end_column: Some(first.column.max(last.column).saturating_add(1)),
                ..TextOptions::default()
            };
            return Some(view.text(first.line..=last.line, options));
        }
        Some(view.text_span(
            (first.line, first.column),
            (last.line, last.column.saturating_add(1)),
        ))
    }
}

/// Only simple text-view intervals admit the empty boundary after a physical row.
fn invalid_columns(
    kind: SelectionKind,
    start: SelectPoint,
    end: SelectPoint,
    width: usize,
) -> bool {
    // Preserve the existing cell-addressed contract for word/line/rectangle APIs.
    let maximum = if kind == SelectionKind::Simple {
        width
    } else {
        width - 1
    };
    start.column > maximum || end.column > maximum
}

/// Hashes selected text and its retained style across a physical reflow.
///
/// Fresh destination rows have new IDs and inherited source stamps, so old
/// identity/version pairs cannot validate them. Logical text and cell records
/// remain in order; blank padding inserted before a wide glyph is ignored.
fn selection_fingerprint(term: &Terminal, first: SelectPoint, last: SelectPoint) -> Option<u64> {
    if term.screen().ring.multicell_storage_bytes() != 0 {
        return multicell::fingerprint(term, first, last);
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for line in term.buffer().lines(first.line..=last.line) {
        let begin = if line.id() == first.line {
            first.column
        } else {
            0
        };
        let end = if line.id() == last.line {
            last.column.saturating_add(1)
        } else {
            line.cells().len()
        };
        for column in begin..end.min(line.cells().len()) {
            let cell = line.cell(column)?;
            if cell.width == 0 || cell.raw == crate::Cell::default() {
                continue;
            }
            // Borrow the cluster instead of constructing a whole selected String (P1).
            cell.text.hash(&mut hasher);
            cell.raw.ch.hash(&mut hasher);
            cell.raw.fg.0.hash(&mut hasher);
            cell.raw.bg.0.hash(&mut hasher);
            cell.raw.attrs.bits().hash(&mut hasher);
            cell.raw.flags.bits().hash(&mut hasher);
            cell.underline_color.map(|color| color.0).hash(&mut hasher);
            cell.link.hash(&mut hasher);
        }
        // Hard logical boundaries remain meaningful when physical soft wraps change.
        // Hash a boundary only between selected rows, not beyond the final endpoint.
        if !line.wrapped() && line.id() != last.line {
            '\n'.hash(&mut hasher);
        }
    }
    Some(hasher.finish())
}

/// Resolves one marker after scrolling or reflow, rejecting eviction.
///
/// A Gone handle is terminal state, not a recoverable row index.
fn resolve(term: &Terminal, handle: TrackedPos) -> Option<SelectPoint> {
    match term.tracked.get(handle) {
        PositionState::Present { line, column } => Some(SelectPoint { line, column }),
        PositionState::Gone => None,
    }
}

/// Orders endpoints by stable retained document order.
///
/// Columns break ties when both gestures point into one physical row.
fn order(a: SelectPoint, b: SelectPoint) -> (SelectPoint, SelectPoint) {
    if (a.line, a.column) <= (b.line, b.column) {
        (a, b)
    } else {
        (b, a)
    }
}

#[cfg(test)]
#[path = "selection/tests.rs"]
mod tests;
