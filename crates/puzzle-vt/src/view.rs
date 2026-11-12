//! Borrowed terminal contents for selection, search and external APIs (§6.13).
//!
//! The grid and scrollback remain owned by `Terminal`. A `BufferView` is
//! only a borrow, so creating it to inspect a cell or its `LineId` does not
//! allocate. Explicit extraction, serialization and bulk-copy methods own
//! their output. This keeps per-cell rendering queries separate from the
//! message bus (§5.5) and makes the later JS API a thin wrapper.
//!
//! M5 has no animated viewport or frame builder. The viewport method
//! therefore reports the live visible tail with zero fractional offset;
//! M8 will supply the scroll position while keeping line identities.
//!
//! A line ID is monotonic and survives scrollback rotation. Grid row
//! numbers are transient: after one line feed, the same content appears
//! at a different viewport row. All long-lived references in this API
//! therefore use IDs, while `viewport()` is the adapter between IDs and
//! currently visible rows. A missing ID means the bounded ring evicted it.
//!
//! OSC shell zones and hyperlinks are borrowed only while their backing
//! line exists. A link's numeric handle is checked against the link table
//! generation before a URL reaches any caller. The VT crate does not open
//! URLs or decide clipboard policy; those are platform/session actions.
use crate::{
    cell::{Attrs, Cell, CellFlags, PackedColor},
    modes::Modes,
    row::{Row, RowFlags},
    terminal::{ActiveScreen, Terminal},
    tracked::LineId,
    zones::ZoneMark,
};
use std::ops::RangeInclusive;

/// Bulk structure-of-arrays transfer and change detection.
mod bulk;
/// Retention depth is shared with allocation probes instead of copied literals.
pub use bulk::CHANGE_LOG_FRAMES;
/// Borrowed scalar, cluster or unwritten cell text.
mod line;
/// OSC 8 and on-demand plain-text URL lookup.
mod links;
/// Borrowed rectangular text and logical continuation geometry.
mod multicell;
/// Plain, styled VT and HTML serialization.
mod serialize;
/// Text extraction and logical-line traversal.
mod text;

pub(crate) use bulk::ChangeLog;
pub use bulk::{CellBlock, ChangeSet, ChangeToken};
pub use line::{CellText, CellView, LineView};
/// Visible source cells of an explicit or detected hovered link.
pub use links::LinkTarget;
pub use serialize::{CellRange, Format};
pub use text::TextOptions;

/// Visible line range and device-pixel displacement of the live viewport.
///
/// M5 reports the bottom-following view. M8 owns animated offsets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Viewport {
    /// First currently visible line, independent of historical offset.
    pub first: LineId,
    /// Last currently visible line.
    pub last: LineId,
    /// Zero until the M8 smooth-scroll owner supplies its offset.
    pub fractional_px: i32,
}

/// Cursor state resolved to a stable line identity.
///
/// Delayed wrap is included because a cursor at the last column can be
/// either ready to overwrite or waiting to wrap on the next printable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CursorView {
    /// Line containing the active cursor.
    pub line: LineId,
    /// Zero-based cell column.
    pub column: usize,
    /// DECSET 25 controls visual visibility.
    pub visible: bool,
    /// DECSCUSR shape code 0–6.
    pub style: u8,
    /// Delayed autowrap after writing the final column.
    pub pending_wrap: bool,
}

/// A cheap read-only view of the active screen and retained history.
///
/// It contains one reference and may be created for individual cell
/// queries without copying rows, palettes or scrollback.
#[derive(Clone, Copy)]
pub struct BufferView<'a> {
    /// All policy state is borrowed from one consistent terminal instance.
    pub(crate) term: &'a Terminal,
    /// Session can attach a local viewport without mutating VT screen state.
    viewport_override: Option<Viewport>,
}

impl<'a> BufferView<'a> {
    /// Wraps a terminal borrow; callers normally use `Terminal::buffer()`.
    #[must_use]
    pub fn new(term: &'a Terminal) -> Self {
        Self {
            term,
            viewport_override: None,
        }
    }

    /// Attaches Session-owned presentation geometry to this cheap borrowed view.
    pub fn with_viewport(mut self, viewport: Viewport) -> Self {
        // Cursor and application screen queries still address the live VT.
        self.viewport_override = Some(viewport);
        self
    }

    /// Selects the main or alternate grid without changing it.
    #[must_use]
    pub fn active(self) -> ActiveScreen {
        self.term.active
    }

    /// Returns columns and rows of the active full-width grid.
    #[must_use]
    pub fn size(self) -> (usize, usize) {
        self.term.size()
    }

    /// Returns oldest and newest retained IDs, if the grid has rows.
    ///
    /// The inclusive range can contain gaps after region editing; `line`
    /// returns `None` for any missing identity inside it.
    #[must_use]
    pub fn line_ids(self) -> Option<RangeInclusive<LineId>> {
        let ring = &self.term.screen().ring;
        Some(ring.oldest_id()?..=ring.newest_id()?)
    }

    /// Reports the live visible tail until M8 owns the scroll offset.
    #[must_use]
    pub fn viewport(self) -> Option<Viewport> {
        if let Some(viewport) = self.viewport_override {
            return Some(viewport);
        }
        let screen = self.term.screen();
        Some(Viewport {
            first: screen.row(0)?.id,
            last: screen.row(self.term.size().1.saturating_sub(1))?.id,
            fractional_px: 0,
        })
    }

    /// Resolves the cursor to the identity of its current row.
    #[must_use]
    pub fn cursor(self) -> Option<CursorView> {
        Some(CursorView {
            line: self.term.cursor_line()?,
            column: self.term.cursor.column,
            visible: self.term.modes.get(true, 25) == Some(true),
            style: self.term.cursor_style,
            pending_wrap: self.term.cursor.pending_wrap,
        })
    }

    /// Looks up one retained row by identity, including scrollback.
    #[must_use]
    pub fn line(self, id: LineId) -> Option<LineView<'a>> {
        let row = self.term.screen().ring.line(id)?;
        Some(LineView::new(row, self.term))
    }

    /// Borrows one row in retained document order, without an identity search.
    /// Optional incremental contents consumers retain only the index and must
    /// revalidate their source ownership after a mutation or between turns.
    #[must_use]
    pub fn retained_line(self, index: usize) -> Option<LineView<'a>> {
        self.term
            .screen()
            .ring
            .retained(index)
            .map(|row| LineView::new(row, self.term))
    }

    /// Iterates existing rows in an inclusive ID range without copies.
    ///
    /// Filtering the borrowed ring handles noncontiguous identities.
    pub fn lines(self, range: RangeInclusive<LineId>) -> impl Iterator<Item = LineView<'a>> {
        self.term
            .screen()
            .ring
            .iter()
            .filter(move |row| range.contains(&row.id))
            .map(move |row| LineView::new(row, self.term))
    }

    /// Returns marks still referring to retained rows.
    ///
    /// Shell marks on evicted lines are filtered without mutating Zones.
    pub fn zones(self) -> impl Iterator<Item = &'a ZoneMark> {
        self.term
            .zones
            .iter()
            .filter(move |mark| self.line(mark.line).is_some())
    }

    /// Finds the most recent shell mark no later than this position.
    #[must_use]
    pub fn zone_at(self, line: LineId, column: usize) -> Option<&'a ZoneMark> {
        self.zones()
            .filter(|mark| (mark.line, mark.column) <= (line, column))
            .last()
    }

    /// Extracts the output following a command's OSC 133 C mark.
    #[must_use]
    pub fn command_output(self, mark: &ZoneMark) -> Option<String> {
        if mark.kind != crate::zones::ZoneKind::Output {
            return None;
        }
        let end = self.zones().find(|next| {
            next.id == mark.id
                && (next.line, next.column) >= (mark.line, mark.column)
                && next.kind == crate::zones::ZoneKind::Finished
        })?;
        // Shell boundaries are cursor positions, so the end cell itself
        // belongs to the next prompt rather than command output.
        Some(self.text_span((mark.line, mark.column), (end.line, end.column)))
    }

    /// Returns the supported-mode table for read-only queries.
    #[must_use]
    pub fn modes(self) -> &'a Modes {
        &self.term.modes
    }

    /// Borrows the active UAPI context path without copying metadata (OSC 3008).
    pub fn contexts(self) -> &'a [crate::context::Context] {
        self.term.contexts()
    }

    /// Borrows the bounded OSC title string.
    #[must_use]
    pub fn title(self) -> &'a str {
        &self.term.title
    }

    /// Borrows the optional OSC 7 current-directory URI.
    #[must_use]
    pub fn cwd(self) -> Option<&'a str> {
        self.term.cwd.as_deref()
    }
}

#[cfg(test)]
#[path = "view/tests.rs"]
mod tests;
