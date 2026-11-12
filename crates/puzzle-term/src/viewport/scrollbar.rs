//! Scrollbar geometry and cached marker data; drawing belongs to M9 (D-34).
//!
//! Markers are refreshed on source changes, never by frame assembly. Their
//! normalized locations refer to document rows, not numeric line-ID gaps.
//!
//! The thumb uses the complete document length: visible rows divided by
//! visible rows plus history. Its origin is the top document row divided
//! by that same total, so a bottom-following thumb ends exactly at the track
//! bottom. Dragging uses available thumb travel, rather than dividing by
//! track height alone; otherwise the end could never reach the last row.
//!
//! Track coordinates remain device pixels. They are supplied by Session's
//! font/grid geometry so an M9 overlay can draw and hit-test the same model.
//! Fractions are converted only when queried, allowing a track resize to
//! move existing markers without scanning terminal cells or reshaping text.
//!
//! Shell integration is a stream of A/B/C/D boundaries. A prompt marker
//! takes the status of its following Finished mark, stopping at the next
//! Prompt even when the previous command omitted its end marker. This keeps
//! a later command's failure from recoloring an unrelated earlier prompt.
//! Search and selection ranges include their last document row; this gives
//! even a single-cell match a nonzero track interval. Bell markers are a
//! single row. If an endpoint was evicted, its marker is omitted entirely.
//!
//! Rebuilding preserves Vec capacity. Sorted document indices allow the
//! navigation code to use forward/reverse iteration for signed jumps, even
//! after region edits made numeric LineIds appear out of document order.
//! No timer or presentation callback rebuilds these source markers.
use puzzle_vt::{terminal::Terminal, tracked::LineId, zones::ZoneKind};
/// Which source supplies a scrollbar marker and its future overlay color.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkerKind {
    /// Shell prompt, with the following command's exit status when available.
    Prompt(Option<i32>),
    /// Literal-search match; search wiring supplies these in M8.6.
    Match,
    /// Selected text range; active selection supplies this in M8.4.
    Selection,
    /// A bell's retained cursor location.
    Bell,
}
/// One normalized interval on the scrollbar track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    /// Retained line used by absolute navigation.
    pub line: LineId,
    /// Document index for navigating through noncontiguous line identities.
    pub row: usize,
    /// Exclusive physical end survives reversible display projection exactly.
    pub end_row: usize,
    /// Fractional start in the total document.
    pub start: f64,
    /// Fractional end, including the selected/matched final row.
    pub end: f64,
    /// Source and semantic coloring information.
    pub kind: MarkerKind,
}
/// Reused data consumed by the future built-in overlay.
#[derive(Default)]
pub struct Scrollbar {
    /// Device rectangle [x, y, width, height], supplied by the grid geometry.
    track: [f64; 4],
    /// Normalized thumb start and size.
    thumb: [f64; 2],
    /// Source-driven normalized markers in document order.
    markers: Vec<Marker>,
    /// Counts recomputations so idle regressions can detect hidden scanning.
    revision: u64,
}
impl Scrollbar {
    /// Sets future overlay geometry without allocating a draw list.
    pub fn set_track(&mut self, rect: [f64; 4]) {
        self.track = rect;
    }
    /// Returns device-space track bounds.
    pub fn track(&self) -> [f64; 4] {
        self.track
    }
    /// Returns device-space thumb y origin and height.
    pub fn thumb(&self) -> [f64; 2] {
        [
            self.track[1] + self.thumb[0] * self.track[3],
            self.thumb[1] * self.track[3],
        ]
    }
    /// Borrows cached marker intervals without rescanning the terminal.
    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }
    /// Counts source-driven rebuilds, independent of presentation frequency.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Updates thumb geometry whenever viewport position or history changes.
    pub(super) fn set_position(&mut self, top: usize, rows: usize, history: usize) {
        let total = (rows + history).max(1) as f64;
        self.thumb = [top as f64 / total, rows as f64 / total];
    }
    /// Rebuilds markers after source changes, retaining the allocated vector.
    pub fn update(
        &mut self,
        term: &Terminal,
        matches: &[(LineId, LineId)],
        selection: Option<(LineId, LineId)>,
        bells: &[LineId],
    ) {
        self.markers.clear();
        // Exit belongs to the command after its prompt, stopping at the next prompt.
        let mut zones = term.buffer().zones().peekable();
        while let Some(mark) = zones.next() {
            if mark.kind != ZoneKind::Prompt {
                continue;
            }
            let mut exit = None;
            while zones
                .peek()
                .is_some_and(|next| next.kind != ZoneKind::Prompt)
            {
                if let Some(next) = zones.next()
                    && next.kind == ZoneKind::Finished
                {
                    exit = next.exit;
                }
            }
            self.push(term, mark.line, mark.line, MarkerKind::Prompt(exit));
        }
        for &(start, end) in matches {
            self.push(term, start, end, MarkerKind::Match);
        }
        if let Some((start, end)) = selection {
            self.push(term, start, end, MarkerKind::Selection);
        }
        for &line in bells {
            self.push(term, line, line, MarkerKind::Bell);
        }
        self.markers.sort_by_key(|marker| marker.row);
        self.revision = self.revision.wrapping_add(1);
    }
    /// Resolves a source range into document fractions, dropping evicted IDs.
    fn push(&mut self, term: &Terminal, start: LineId, end: LineId, kind: MarkerKind) {
        let ring = &term.screen().ring;
        let Some(first) = ring.index_of(start) else {
            return;
        };
        let Some(last) = ring.index_of(end) else {
            return;
        };
        let total = (ring.scrollback_len() + term.size().1) as f64;
        self.markers.push(Marker {
            line: start,
            row: first,
            end_row: last + 1,
            start: first as f64 / total,
            end: (last + 1) as f64 / total,
            kind,
        });
    }
    /// Recolouring/navigation retain source indices while track positions compress.
    pub(super) fn project_rows(
        &mut self,
        projection: &puzzle_overlay::folding::RowProjection,
        rows: usize,
    ) {
        // A short folded document still occupies one full thumb: fractions use
        // at least the visible grid height, never a zero-length denominator.
        let total = projection.len().max(rows).max(1) as f64;
        for marker in &mut self.markers {
            // A match inside hidden source points at the visible fold marker.
            let first = projection.display_anchor(marker.row).unwrap_or(0);
            let last = projection
                .display_anchor(marker.end_row.saturating_sub(1))
                .unwrap_or(first);
            // Physical row/end_row stay intact so navigation can reveal the match.
            marker.start = first as f64 / total;
            marker.end = (last + 1) as f64 / total;
        }
    }

    /// Finds a prompt or match strictly before/after the current top row.
    pub(super) fn navigate(&self, top: usize, delta: i32, prompts: bool) -> Option<LineId> {
        let qualifies = |marker: &&Marker| {
            if prompts {
                matches!(marker.kind, MarkerKind::Prompt(_))
            } else {
                marker.kind == MarkerKind::Match
            }
        };
        let steps = delta.unsigned_abs().saturating_sub(1) as usize;
        if delta < 0 {
            self.markers
                .iter()
                .rev()
                .filter(qualifies)
                .filter(|marker| marker.row < top)
                .nth(steps)
                .map(|marker| marker.line)
        } else if delta > 0 {
            self.markers
                .iter()
                .filter(qualifies)
                .filter(|marker| marker.row > top)
                .nth(steps)
                .map(|marker| marker.line)
        } else {
            None
        }
    }
}
