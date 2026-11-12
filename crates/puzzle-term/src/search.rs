//! Session search model over the existing tracked VT engine (§6.10, D-34).
//!
//! Starting a query schedules work without scanning. Each refresh advances one
//! physical-row slice; parser and input drains merely mark the source dirty.
//! Resolved ranges are cached only after source/slice/navigation changes. Frame
//! assembly borrows them, and a desktop snapshot copies only visible hits.
//! Regex remains deferred. Cell fills and scrollbar markers share this model;
//! no search bar, marker overlay or per-frame query allocation lives here.
use crate::{
    selection::SelectionRange,
    vt::{
        Terminal,
        search::{FindOptions, Search},
        selection::SelectPoint,
        tracked::LineId,
    },
};

/// Per-surface choice of the query UI; both variants share the same search model.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SearchUi {
    /// Puzzle's lazily opened overlay handles query editing by default.
    #[default]
    BuiltIn,
    /// The host receives find intent and supplies its own query controls.
    Host,
}

/// Policy-approved literal search requests; navigation uses ScrollTo(Match).
#[derive(Clone, Debug)]
pub enum SearchCommand {
    /// Replace the previous query without doing synchronous history work.
    Find {
        /// Literal Unicode text; empty clears the model.
        query: String,
        /// Explicit case policy, otherwise smart case.
        options: FindOptions,
    },
    /// Release all hits and stop incremental work.
    Clear,
    /// Ask the embedding page to focus its own search UI.
    Request,
}

/// Plain current progress returned to hosts without exposing registry handles.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SearchState {
    /// Capped number of surviving matches found so far.
    pub count: usize,
    /// Zero-based current result; absent only while no matches are discovered.
    pub current: Option<usize>,
    /// True when the current source has been fully searched or capped.
    pub complete: bool,
    /// Physical rows inspected in the last refresh, including rewrite rewind.
    pub scanned: usize,
}

/// Resolved paper interval and whether it is the navigated match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchFill {
    /// Inclusive cells, widened to include a wide glyph's spacer.
    pub range: SelectionRange,
    /// Selects the stronger searchCurrent token.
    pub current: bool,
}

/// One document's lazy query, cached ranges and marker line identities.
#[derive(Default)]
pub(crate) struct SearchModel {
    /// Allocated only when a caller starts a nonempty literal query.
    engine: Option<Search>,
    /// Latest direction requested before this query has discovered any matches.
    pending_step: Option<i32>,
    /// Output/resize reconciliation runs once at the next frame boundary.
    pub(crate) dirty: bool,
    /// Resolved ranges reuse their allocation after incremental source changes.
    pub(crate) fills: Vec<SearchFill>,
    /// Scrollbar source borrows these lines without scanning text again.
    pub(crate) lines: Vec<(LineId, LineId)>,
}

/// Owns query lifetime and caches the two render/marker views of its results.
impl SearchModel {
    /// Installs a cold query, releasing the old owner's registry slots first.
    pub(crate) fn find(&mut self, term: &mut Terminal, query: &str, options: FindOptions) {
        // Replacing a query releases both tracked endpoints of every old match.
        self.clear(term);
        // Empty input is an explicit clear, with no latent work or timer.
        if !query.is_empty() {
            self.engine = Some(Search::new(query, options));
            // First reconciliation establishes screen identity before any scan.
            self.dirty = true;
        }
    }
    /// Stops work and frees endpoint slots, preserving vector capacity.
    pub(crate) fn clear(&mut self, term: &mut Terminal) {
        // The VT registry owns slots, so dropping a handle Vec alone is insufficient.
        if let Some(mut engine) = self.engine.take() {
            engine.clear(term);
        }
        // Keep buffer capacity across navigation and replacement queries.
        self.fills.clear();
        self.lines.clear();
        // Intent belongs to this query, never a replacement or a reopened field.
        self.pending_step = None;
        // Source reconciliation has consumed this transaction's dirty edge.
        self.dirty = false;
    }
    /// Returns cheap progress after the latest actual frame slice.
    pub(crate) fn state(&self) -> SearchState {
        // No active query is complete immediately and has no current match.
        self.engine.as_ref().map_or(
            SearchState {
                complete: true,
                ..SearchState::default()
            },
            |s| SearchState {
                count: s.count(),
                current: s.current(),
                // Previously finished results become partial as soon as output arrives.
                complete: s.is_done() && !self.dirty,
                scanned: s.visited(),
            },
        )
    }
    /// Work requests refreshes only while a source or unfinished query exists.
    pub(crate) fn working(&self) -> bool {
        self.engine
            .as_ref()
            .is_some_and(|s| self.dirty || !s.is_done())
    }
    /// Performs one slice and resolves changed ranges before a snapshot copies them.
    pub(crate) fn advance(&mut self, term: &mut Terminal) -> bool {
        // Ordinary frames never allocate or scan until search is explicitly started.
        let Some(engine) = &mut self.engine else {
            return false;
        };
        // Reconcile once per display turn, never once per parsed cell or byte.
        if self.dirty {
            engine.reconcile(term);
        }
        // Partial work remains dirty for presentation even without parser output.
        let changed = self.dirty || !engine.is_done();
        // Source reconciliation has consumed this transaction's dirty edge.
        self.dirty = false;
        if changed {
            // VT enforces the shared forward/backward physical-row allowance.
            engine.advance(term);
            self.refresh(term);
        }
        changed
    }
    /// Steps current result modulo the bounded count and returns its centre line.
    pub(crate) fn navigate(&mut self, term: &Terminal, delta: i32) -> Option<LineId> {
        let engine = self.engine.as_mut()?;
        // A zero step must not overwrite an earlier real navigation intent.
        if delta == 0 {
            return None;
        }
        let count = engine.count();
        if count == 0 {
            // Search runs at display boundaries, after commands have drained.
            // Remember only the latest direction; do not queue repeated clicks.
            self.pending_step = Some(delta.signum());
            return None;
        }
        // Modulo avoids a hostile i32 count creating billions of navigation calls.
        let steps = ((delta.unsigned_abs() as usize - 1) % count) + 1;
        // Preserve the final resolved range across repeated directional steps.
        let mut hit = None;
        for _ in 0..steps {
            hit = if delta > 0 {
                engine.next(term)
            } else {
                engine.prev(term)
            };
        }
        // Same-row navigation still changes which interval gets the strong paper.
        self.refresh(term);
        hit.map(|hit| hit.start.line)
    }
    /// Consumes one retained intent as soon as an incremental slice finds a match.
    pub(crate) fn pending_navigation(&mut self, term: &Terminal) -> Option<LineId> {
        // Empty slices leave the intent intact, including across later live output.
        if self.engine.as_ref()?.count() == 0 {
            return None;
        }
        // Take before navigating so an idle frame cannot repeat the same step.
        let delta = self.pending_step.take()?;
        self.navigate(term, delta)
    }
    /// Resolves tracked endpoints only when the model changes, never in idle frames.
    fn refresh(&mut self, term: &Terminal) {
        // Keep buffer capacity across navigation and replacement queries.
        self.fills.clear();
        self.lines.clear();
        let Some(engine) = &self.engine else { return };
        // Trackers may become Gone; render data never retains their stale coordinates.
        for index in 0..engine.count() {
            let Some(hit) = engine.resolve(term, index) else {
                continue;
            };
            // A query matching a wide cluster paints both of its physical cells.
            let width = term
                .buffer()
                .line(hit.end.line)
                .and_then(|line| line.cell(hit.end.column))
                .map_or(1, |c| c.width.max(1));
            self.fills.push(SearchFill {
                range: SelectionRange {
                    start: SelectPoint {
                        line: hit.start.line,
                        column: hit.start.column,
                    },
                    end: SelectPoint {
                        line: hit.end.line,
                        column: hit.end.column + usize::from(width) - 1,
                    },
                    block: false,
                },
                // The public index and strong fill describe the same occurrence.
                current: engine.current() == Some(index),
            });
            // Marker geometry needs only line identities, not the copied cell text.
            self.lines.push((hit.start.line, hit.end.line));
        }
    }
}

/// Initial visible-result policy stays outside the engine's viewport-free matching.
mod initial;
