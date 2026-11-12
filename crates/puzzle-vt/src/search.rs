//! Incremental literal search over retained logical lines (§6.10).
//!
//! A physical row marked WRAPPED continues into the next row, so its
//! trailing text and the next row's prefix are searched as one string.
//! `advance` processes at most 2,000 rows per call and retains only the
//! incomplete needle-length suffix between calls. Results are registered with the terminal's
//! tracked-position table, allowing scrolling and reflow to move them.
//! Search is cold and opt-in; ordinary printing never visits these buffers.
//!
//! Smart case makes a lowercase query case-insensitive and an uppercase
//! query exact. Normalization is done per copied cell, with one source
//! position retained for each UTF-8 byte of the normalized text. This
//! makes `match_indices` byte offsets map back to terminal columns even
//! when a glyph is multibyte or a case fold expands a scalar.
//!
//! The result cap is 10,000 matches (§16), independent of scrollback
//! length. A query may be resumed next frame because at most 2,000
//! physical rows are visited per `advance`. A logical line that crosses
//! a slice boundary keeps only enough `pending` text to finish a future hit.
//! The host later decides how to scroll a selected match into view (M8).
/// Bounded physical-row scanning and read-only find.
mod scan;
/// Source revisions restart only the affected logical suffix.
mod source;

use crate::{
    terminal::Terminal,
    tracked::{LineId, PositionState, TrackedPos},
};

/// Literal-search settings; `None` selects uppercase-sensitive smart case.
///
/// Explicit `Some(true)` makes even lowercase input exact, while
/// `Some(false)` makes a mixed-case query insensitive.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FindOptions {
    /// Explicit case policy, or smart case when omitted.
    pub case_sensitive: Option<bool>,
}

/// One retained match endpoint, including a byte-independent cell column.
///
/// A result never exposes a UTF-8 byte offset to a grid consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FindPoint {
    /// Stable line identity.
    ///
    /// Its handle becomes Gone when that row leaves bounded scrollback.
    pub line: LineId,
    /// Zero-based cell column of the matching scalar.
    ///
    /// Several Unicode scalars in a cluster can share one column.
    pub column: usize,
}

/// One literal match over a possibly soft-wrapped logical line.
///
/// The two line identities may differ when a match crosses a soft wrap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FindMatch {
    /// First included cell.
    pub start: FindPoint,
    /// Last included cell.
    pub end: FindPoint,
}

/// Tracked match that survives scroll and reports Gone on eviction.
///
/// Both ends use the same registry as selection and overlay anchors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrackedMatch {
    /// Registry handle for the first matching cell.
    pub start: TrackedPos,
    /// Registry handle for the final matching cell.
    pub end: TrackedPos,
}

/// Incremental search state and bounded matching results.
///
/// A search owns its scratch buffers; a second query cannot interfere
/// with the first query's partial logical line.
#[derive(Clone, Debug)]
pub struct Search {
    /// Normalized literal needle, empty needles produce no results.
    ///
    /// This is allocated once per query, never in the parser print path.
    needle: String,
    /// Whether uppercase in the query requires exact comparison.
    sensitive: bool,
    /// Number of physical rows already examined.
    ///
    /// It advances by no more than two thousand in one call.
    next_row: usize,
    /// Normalized row plus a needle-length suffix from preceding soft wraps.
    ///
    /// Matching every physical row bounds memory even for giant logical lines.
    pending: String,
    /// One source cell for each normalized UTF-8 byte.
    ///
    /// Byte-indexed storage avoids rescanning a Unicode string for each hit.
    positions: Vec<FindPoint>,
    /// At most ten thousand tracked matches (§16).
    ///
    /// Once full, later lines are deliberately left unsearched.
    results: Vec<TrackedMatch>,
    /// The result selected by next/previous navigation.
    current: Option<usize>,
    /// True once the last retained row and pending line were searched.
    ///
    /// The caller can stop scheduling incremental work at this point.
    done: bool,
    /// Last scanned identity rebases the frontier when oldest rows are evicted.
    last_scanned: Option<LineId>,
    /// Backward logical-line discovery shares the same 2,000-row slice budget.
    rewind: Option<LineId>,
    /// Live rows can be rewritten; immutable history needs no version table.
    live: Vec<(LineId, u64)>,
    /// Screen incarnation and width distinguish reset, switch and reflow.
    context: Option<(u64, usize)>,
    /// Value endpoints await registration after immutable scanning ends.
    found: Vec<FindMatch>,
    /// Physical rows inspected in the last call, including backward discovery.
    visited: usize,
}

impl Search {
    /// Starts a smart-case literal query without scanning immediately.
    ///
    /// Creating the handle is cheap relative to scanning an entire history.
    #[must_use]
    pub fn new(query: &str, options: FindOptions) -> Self {
        let sensitive = options
            .case_sensitive
            .unwrap_or_else(|| query.chars().any(char::is_uppercase));
        Self {
            needle: if sensitive {
                query.to_string()
            } else {
                query.to_lowercase()
            },
            sensitive,
            next_row: 0,
            pending: String::new(),
            positions: Vec::new(),
            results: Vec::new(),
            current: None,
            done: false,
            last_scanned: None,
            rewind: None,
            live: Vec::new(),
            context: None,
            found: Vec::new(),
            visited: 0,
        }
    }

    /// Reports whether all retained rows have been searched.
    ///
    /// This is distinct from having any matches.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Returns the capped number of matches found so far.
    ///
    /// The count can grow between incremental slices.
    #[must_use]
    pub fn count(&self) -> usize {
        self.results.len()
    }

    /// Moves to the next match, wrapping at the end of the result set.
    ///
    /// The host, not this pure crate, scrolls the viewport to the result.
    pub fn next(&mut self, term: &Terminal) -> Option<FindMatch> {
        if self.results.is_empty() {
            return None;
        }
        let index = self.current.map_or(0, |old| (old + 1) % self.results.len());
        self.current = Some(index);
        self.resolve(term, index)
    }

    /// Moves to the previous match, wrapping at the first result.
    ///
    /// A Gone match returns `None` after scrollback eviction.
    pub fn prev(&mut self, term: &Terminal) -> Option<FindMatch> {
        if self.results.is_empty() {
            return None;
        }
        let index = self.current.map_or(self.results.len() - 1, |old| {
            (old + self.results.len() - 1) % self.results.len()
        });
        self.current = Some(index);
        self.resolve(term, index)
    }

    /// Releases result storage and its terminal registry slots.
    ///
    /// Generation checks make copied handles inert when a later query
    /// reuses those slots; repeated searches have bounded storage.
    pub fn clear(&mut self, term: &mut Terminal) {
        for hit in &self.results {
            term.tracked.release(hit.start);
            term.tracked.release(hit.end);
        }
        self.results.clear();
        self.pending.clear();
        self.positions.clear();
        self.current = None;
        self.done = true;
        self.last_scanned = None;
        self.rewind = None;
    }

    /// Current match index is zero-based, or absent before selection.
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    /// Selects a discovered result without walking through intermediate matches.
    /// Session uses this once for its initial visible-match policy; VT has no viewport.
    pub fn select(&mut self, term: &Terminal, index: usize) -> Option<FindMatch> {
        let hit = self.resolve(term, index)?;
        self.current = Some(index);
        Some(hit)
    }

    /// Counts actual physical-row work, not logical lines or yielded frames.
    pub fn visited(&self) -> usize {
        self.visited
    }

    /// Resolves one match after possible scroll/reflow or eviction.
    ///
    /// Positions are asked from the registry at use time rather than
    /// copied at search time, preserving their remapped coordinates.
    #[must_use]
    pub fn resolve(&self, term: &Terminal, index: usize) -> Option<FindMatch> {
        let hit = self.results.get(index)?;
        Some(FindMatch {
            start: tracked_point(term, hit.start)?,
            end: tracked_point(term, hit.end)?,
        })
    }
}

/// Converts a registry handle into a retained line/column point.
///
/// Eviction is represented by absence, never by a recycled row index.
fn tracked_point(term: &Terminal, pos: TrackedPos) -> Option<FindPoint> {
    match term.tracked.get(pos) {
        PositionState::Present { line, column } => Some(FindPoint { line, column }),
        PositionState::Gone => None,
    }
}

#[cfg(test)]
#[path = "search/tests.rs"]
mod tests;
