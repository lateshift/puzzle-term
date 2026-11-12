//! Bounded search slices with a short UTF-8 overlap across soft wraps (§6.10).
//!
//! Search each row immediately, retaining only needle-length overlap. A single
//! logical line spanning 100,000 rows therefore has bounded scratch memory and
//! never defers its entire matching cost to the last frame. Backward discovery
//! after a rewrite also consumes the physical-row allowance. Byte positions map
//! lowercased Unicode expansions back to whole terminal cells without strings
//! allocated per cell. Result registration happens after immutable row borrows.
//!
//! Scaled rectangles contribute their payload at the first retained fragment.
//! Reserved lower rows consume row credit but add no text. Matching byte spans
//! are expanded to physical block extents only when a hit is found, leaving
//! scalar-only rows on the same bounded scanning path as before OSC 66 support.
//! A match inside an explicit-width run selects its indivisible footprint;
//! it cannot invent a physical sub-cell position for one byte of that payload.
//! The source positions remain physical coordinates for tracked navigation.
use super::{FindMatch, FindPoint, Search, TrackedMatch};
use crate::{
    Terminal,
    view::{BufferView, CellText, LineView},
};

/// Shared per-frame physical-row ceiling, including backward source discovery.
pub(super) const SLICE_ROWS: usize = 2_000;
/// Hostile broad searches stop at the documented result cap (§16).
pub(super) const MAX_MATCHES: usize = 10_000;

impl Search {
    /// Scans one bounded slice; a host calls this at most once per refresh.
    pub fn advance(&mut self, term: &mut Terminal) {
        // Count this display turn, including any backward boundary discovery.
        self.visited = 0;
        // Completed and empty queries must leave the idle frame path cheap.
        if self.done || self.needle.is_empty() {
            // Reaching either the tail or the cap stops scheduling further slices.
            self.done = true;
            return;
        }
        // Walking to a logical start must not secretly scan outside the allowance.
        if !self.finish_rewind(term) {
            return;
        }
        // Keep temporary hits separate from mutable tracker registration.
        // Clearing preserves capacity for subsequent source-update slices.
        self.found.clear();
        // Ring dimensions are stored; counting its iterator would be O(history).
        let total = term.screen().ring.scrollback_len() + term.size().1;
        // Direct indexing makes later slices as cheap as the first slice.
        // Iterator skip would repeatedly traverse every preceding retained row.
        while self.next_row < total && self.visited < SLICE_ROWS {
            let Some(row) = term.screen().ring.retained(self.next_row) else {
                break;
            };
            // Borrow the row once, not once per cell through a history lookup.
            if let Some(line) = term.buffer().line(row.id) {
                append_row(self, line);
                collect_matches(self, term.buffer());
            }
            // A hard end forbids a query from joining unrelated physical lines.
            // A soft end retains only the possible incomplete matching suffix.
            if !term
                .buffer()
                .line(row.id)
                .is_some_and(|line| line.continues())
            {
                self.pending.clear();
                self.positions.clear();
            }
            // Identity rebases this frontier if output evicts earlier history.
            self.last_scanned = Some(row.id);
            // Both loop progress and row credit advance exactly once per row.
            self.next_row += 1;
            self.visited += 1;
            // Stop before registering more than the security result limit.
            if self.results.len() + self.found.len() == MAX_MATCHES {
                break;
            }
        }
        self.register_hits(term);
        // Reaching either the tail or the cap stops scheduling further slices.
        self.done = self.next_row >= total || self.results.len() == MAX_MATCHES;
        // Only the live grid may be edited before the next output transaction.
        self.capture_live(term);
    }
    /// Registers completed matches after the scan releases immutable row borrows.
    fn register_hits(&mut self, term: &mut Terminal) {
        // Register only after all read-only text views have left scope.
        for hit in self.found.drain(..) {
            self.results.push(TrackedMatch {
                start: term.tracked.track(hit.start.line, hit.start.column),
                end: term.tracked.track(hit.end.line, hit.end.column),
            });
        }
    }
}

/// Normalizes borrowed cells into one reused row-plus-overlap string.
fn append_row(search: &mut Search, line: LineView<'_>) {
    if line.reservation_only() {
        return;
    }
    // A reserved lower row has no independently searchable text or padding.
    // Borrow text and attributes without allocating a String for every cell.
    let end = if line.has_multicells() {
        // Reservation geometry can continue a logical band without filling the
        // row. Untouched padding is not searchable text between its owners.
        line.cells()
            .iter()
            .rposition(|cell| {
                cell.ch != 0
                    || cell.bg != crate::PackedColor::DEFAULT
                    || cell.flags.contains(crate::CellFlags::MULTICELL)
            })
            .map_or(0, |last| last + 1)
    } else {
        line.cells().len()
    };
    for column in 0..end {
        let Some(cell) = line.cell(column) else {
            continue;
        };
        // The second half of a wide character contributes no searchable text.
        if cell.width == 0 {
            continue;
        }
        // Every byte of a cluster maps back to its single owning terminal cell.
        let point = FindPoint {
            line: line.id(),
            column,
        };
        // Empty terminal cells represent spaces, not a NUL in the search input.
        match cell.text {
            CellText::Empty => append_char(search, ' ', point),
            CellText::Scalar(ch) => append_char(search, ch, point),
            CellText::Cluster(text) => {
                for ch in text.chars() {
                    append_char(search, ch, point);
                }
            }
        }
    }
}

/// Stack-encodes one scalar; case expansion keeps the original source cell.
fn append_char(search: &mut Search, ch: char, point: FindPoint) {
    // UTF-8 needs at most four stack bytes per scalar; no temporary heap string.
    let mut bytes = [0; 4];
    // Normalize source exactly as the query was normalized by Search::new.
    if search.sensitive {
        let text = ch.encode_utf8(&mut bytes);
        search.pending.push_str(text);
        // Byte offsets from match_indices must resolve inside multibyte text.
        search
            .positions
            .extend(std::iter::repeat_n(point, text.len()));
    } else {
        // Unicode lowercase may expand one scalar into several scalars.
        for lower in ch.to_lowercase() {
            let text = lower.encode_utf8(&mut bytes);
            search.pending.push_str(text);
            // An expanded lowercase scalar still names the original cell.
            search
                .positions
                .extend(std::iter::repeat_n(point, text.len()));
        }
    }
}

/// Emits nonoverlapping literal hits and preserves only an incomplete suffix.
fn collect_matches(search: &mut Search, view: BufferView<'_>) {
    // A completed nonoverlapping match consumes its whole prefix permanently.
    let mut consumed = 0;
    // Standard literal matching preserves Rust string UTF-8 boundaries.
    for (offset, _) in search.pending.match_indices(&search.needle) {
        if search.results.len() + search.found.len() == MAX_MATCHES {
            break;
        }
        // Endpoints are inclusive cells even though string ranges are exclusive.
        let end = offset + search.needle.len();
        if let (Some(&start), Some(&last)) =
            (search.positions.get(offset), search.positions.get(end - 1))
        {
            // Last-byte mapping retains the whole final Unicode cluster.
            let start = view
                .multicell_extent(start.line, start.column)
                .map_or(start, |(p, _)| FindPoint {
                    line: p.0,
                    column: p.1,
                });
            let end = view
                .multicell_extent(last.line, last.column)
                .map_or(last, |(_, p)| FindPoint {
                    line: p.0,
                    column: p.1,
                });
            search.found.push(FindMatch { start, end });
        }
        consumed = end;
    }
    // A future hit needs at most needle.len()-1 bytes from the preceding row.
    // Respect UTF-8 boundaries while keeping byte and cell maps in lockstep.
    let mut discard = consumed.max(search.pending.len().saturating_sub(search.needle.len() - 1));
    while !search.pending.is_char_boundary(discard) {
        discard += 1;
    }
    // Drop consumed bytes without relinquishing reusable row buffer capacity.
    search.pending.drain(..discard);
    search.positions.drain(..discard);
}

impl BufferView<'_> {
    /// Cold read-only contents search returns capped value ranges without trackers.
    pub fn find(self, query: &str, options: super::FindOptions) -> std::vec::IntoIter<FindMatch> {
        // This explicit cold contents query is synchronous, unlike frame search.
        let mut search = Search::new(query, options);
        if !search.needle.is_empty() {
            // Read-only results are values and do not consume tracked slots.
            for row in self.term.screen().ring.iter() {
                if let Some(line) = self.line(row.id) {
                    append_row(&mut search, line);
                    collect_matches(&mut search, self);
                }
                // A hard end forbids a query from joining unrelated physical lines.
                // A soft end retains only the possible incomplete matching suffix.
                if !self.line(row.id).is_some_and(|line| line.continues()) {
                    search.pending.clear();
                    search.positions.clear();
                }
                // Even bulk contents queries obey the shared bounded-hit policy.
                if search.found.len() == MAX_MATCHES {
                    break;
                }
            }
        }
        // Transfer the cold result allocation to the caller-owned iterator.
        search.found.into_iter()
    }
}
