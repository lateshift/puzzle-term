//! Damage-proportional copy of visible VT data for frame construction.
//! Selection is copied as a small expanded range alongside cursor and modes.
//! Its identities may lie outside the visible rows; assembly intersects them.
//! Capturing or clearing it never modifies copied cell versions.
//! Both desktop external frames and direct frames share this metadata.
//!
//! Each rotating slot owns its row contents even when that row is unchanged
//! since the slot's preceding capture. The renderer may skip a publication,
//! so its cache history differs from the slot's copy history (D-50). Copy
//! counters describe writes this turn; retained data is available for any
//! renderer cache miss without consulting the live terminal.
//!
//! Identity, version and cursor isolation determine which rows to copy.
//! Reused screen-position buffers retain both cell and cluster capacity;
//! three slots cost three bounded visible grids, never the scrollback.
//! Style, palette and screen changes force recapture. Metadata, cursor,
//! selection and search remain sealed with the rows as one owned value.
//!
//! There are two independent histories. A slot may last have carried frame
//! N, while the renderer last built frame N+1 from another slot. Copy flags
//! answer whether VT changed since N; they cannot answer whether prepared
//! geometry exists for the renderer's current identity and content version.
//! Dropping an intermediate publication must therefore leave the next slot
//! self-contained, including the rows its own capture did not have to write.
//!
//! Reusing only a compact changed-row prefix would also be incorrect: a
//! single edit at the bottom could overwrite the retained top row's storage.
//! Screen-position indexing preserves that top row until its metadata changes.
//! Warm recaptures update vectors with clone_from and explicitly reuse sparse
//! cluster strings rather than allocating a cluster copy for every redraw.
//!
//! A scrolled row can move without changing its content version. LineId and
//! cursor isolation accompany the version so every slot detects that move.
//! Palette overrides are copied separately from cells, preserving one color
//! state across the whole frame even while the Session parses later OSCs.
//! The style epoch likewise changes when fonts change without any VT edit.
//! No glyph handle or reference into the terminal crosses this boundary.
//! Sparse rectangle payloads likewise belong to a publication, not the live slot key.
use puzzle_vt::{
    cell::{Cell, CellFlags, PackedColor},
    row::Row,
    terminal::Terminal,
    tracked::LineId,
    view::{CellText, CursorView},
};

/// D-50's structural publication-slot count, shared with native hosts and
/// bounded immutable model pools. This is ownership, not a pacing parameter.
pub const PUBLICATION_SLOTS: usize = 3;

/// Allocation-preserving cell metadata copies and blink summaries.
mod copying;
/// Visible cell-anchored images sealed by the Session owner.
pub(crate) mod images;
/// Publication-local rectangular text and sparse visible ownership.
pub(crate) mod multicell;
/// Noncontiguous display windows retain the same copy loop and source identities.
mod projection;
use copying::{copy_extras, cursor_from_terminal, row_blinks};
/// Frozen source state travels with rows, independently of rendering resources.
pub(crate) mod publication;

/// Identity and location of one currently visible row.
#[derive(Clone, Copy, Default)]
pub struct RowMeta {
    /// Monotonic identity follows a logical row through scrolling.
    pub id: Option<LineId>,
    /// In-place content revision invalidates prepared glyph geometry.
    pub version: u64,
    /// Synthetic fold marker row; cells are blank while source remains intact.
    pub marker: bool,
    /// Sparse block revisions and first-visible ownership affect row ink.
    pub(crate) block_signature: u64,
    /// Cursor isolation affects ligatures without changing VT cells.
    pub cursor_column: Option<usize>,
    /// Slot in this snapshot’s retained pool, independent of current copy flags.
    slot: Option<usize>,
    /// Visible SGR-5 ink is computed only when this slot copies the row.
    blinking: bool,
    /// Whether this capture wrote cells; renderer cache selection ignores this flag.
    copied: bool,
}

/// Terminal-wide colors and modes needed to paint copied cells.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PaintState {
    /// Unified defaults and overrides sealed with the rows (D-50).
    pub colors: puzzle_vt::color_table::ColorTable,
    /// DEC reverse-video changes every cell's resolved color.
    pub reverse_video: bool,
    /// Mode 12 determines whether odd DECSCUSR styles blink.
    pub cursor_blink: bool,
}

impl PaintState {
    /// Copies small terminal-wide state without retaining a VT borrow.
    pub(crate) fn from_terminal(term: &Terminal) -> Self {
        Self {
            colors: term.colors,
            reverse_video: term.modes.get(true, 5).unwrap_or(false),
            cursor_blink: term.modes.get(true, 12).unwrap_or(false),
        }
    }
}

/// The visible cursor and its physical cell width.
#[derive(Clone, Copy)]
pub struct CursorSnapshot {
    /// Stable VT cursor facts, including DECSCUSR style and visibility.
    pub view: CursorView,
    /// Current grid row, independent of scrollback identity.
    pub row_index: usize,
    /// Wide leading cells occupy two physical columns.
    pub wide: bool,
    /// Cell colors are copied even when its row geometry stayed cached.
    pub cell: Cell,
}

/// One cell's borrowed text and sparse underline metadata.
pub struct SnapshotCell<'a> {
    /// Compact VT cell, with tagged colors and SGR attributes.
    pub raw: Cell,
    /// Inline scalar or borrowed cluster from the copied row.
    pub text: CellText<'a>,
    /// SGR 58 color, when present in sparse row metadata.
    pub underline_color: Option<PackedColor>,
}

/// Reused row-copy storage and terminal-wide paint facts for one frame.
#[derive(Default)]
pub struct FrameSnapshot {
    /// Sealed opt-in prevents unsynchronized observer queries from the render thread.
    pub(crate) observe_frame: bool,
    /// OSC title belongs to this source boundary, independently of later host events.
    title: String,
    /// Optional image metadata and shared pixels; absent before the first placement.
    pub(crate) images: Option<Box<images::Images>>,
    /// Lazy visible OSC 66 copies never resolve against the live Session.
    multicells: Option<Box<multicell::Blocks>>,
    /// Forward allowance even before the first placement; disabled images stay zero.
    pub(crate) image_budget: usize,
    /// None on direct VT-only captures; published frames seal host preferences.
    pub(crate) policy: Option<crate::frame_builder::policy::FramePolicy>,
    /// Immutable overlay inputs and one-time drawing registrations.
    pub(crate) overlays: publication::OverlayPayload,
    /// Visible width remains available when no row cells were copied.
    columns: usize,
    /// Fixed grid height excludes the extra clipped edge row.
    grid_rows: usize,
    /// Signed integral device pixels applied during assembly.
    row_shift: i32,
    /// Separate screen epochs prevent cache reuse across overlapping IDs.
    screen_epoch: u64,
    /// Metadata exists for every visible row, even when its cells are cached.
    metadata: Vec<RowMeta>,
    /// One retained row per visible position, reused when this slot circulates.
    copied_rows: Vec<Row>,
    /// Number of row writes in the latest capture (not retained capacity).
    active_rows: usize,
    /// Last style revision used to decide whether old geometry is reusable.
    style_epoch: Option<u64>,
    /// Color and mode facts copied with the rows as one consistent state.
    paint: Option<PaintState>,
    /// Cursor width is sampled before another parse slice can change it.
    cursor: Option<CursorSnapshot>,
    /// Expanded selection is sealed with rows by the Session owner.
    selection: Option<crate::selection::SelectionRange>,
    /// The Session owner publishes only query ranges within these visible rows.
    search: Vec<crate::search::SearchFill>,
}

impl FrameSnapshot {
    /// Copies changed visible rows on the Session owner before publishing an owned slot.
    pub fn capture(&mut self, term: &Terminal, style_epoch: u64) -> Result<(), String> {
        self.capture_viewport(term, style_epoch, term.screen().ring.scrollback_len())
    }

    /// Copies the visible document slice, which may begin in retained history.
    pub fn capture_viewport(
        &mut self,
        term: &Terminal,
        style_epoch: u64,
        top: usize,
    ) -> Result<(), String> {
        self.capture_scrolled(term, style_epoch, top, 0)
    }

    /// Includes both edge rows while retaining the fixed grid dimensions.
    pub fn capture_scrolled(
        &mut self,
        term: &Terminal,
        style_epoch: u64,
        top: usize,
        row_shift: i32,
    ) -> Result<(), String> {
        let grid_rows = term.size().1;
        // A nonzero shift needs one additional row at the bottom edge.
        let row_count = (grid_rows + usize::from(row_shift != 0))
            .min((term.screen().ring.scrollback_len() + grid_rows).saturating_sub(top));
        self.capture_selected(
            term,
            style_epoch,
            row_count,
            row_shift,
            &|index| top + index,
            &|_| false,
        )
    }

    /// Both contiguous and projected captures share exactly one row-copy loop.
    fn capture_selected(
        &mut self,
        term: &Terminal,
        style_epoch: u64,
        row_count: usize,
        row_shift: i32,
        source_row: &impl Fn(usize) -> usize,
        marker: &impl Fn(usize) -> bool,
    ) -> Result<(), String> {
        let (columns, grid_rows) = term.size();
        self.grid_rows = grid_rows;
        self.row_shift = row_shift;
        // Host title notifications can overtake rendering of an older slot. Keep
        // this bounded observation with its cells; unchanged titles cost no copy.
        if self.title != term.title {
            self.title.clone_from(&term.title);
        }
        let paint = PaintState::from_terminal(term);
        let force = self.metadata.len() != row_count
            || self.style_epoch != Some(style_epoch)
            || self.paint != Some(paint)
            || self.screen_epoch != term.screen_epoch;
        // Resizing or recoloring invalidates every prepared row even if
        // its identity and content version happen to remain unchanged.
        self.metadata.resize(row_count, RowMeta::default());
        self.active_rows = 0;
        self.cursor = cursor_from_terminal(term);
        // Marker rows carry no source fragments; the ordinary closure erases this branch.
        self.capture_blocks(term, row_count, &|index| {
            if marker(index) {
                usize::MAX
            } else {
                source_row(index)
            }
        });
        for index in 0..row_count {
            let row = term
                .screen()
                .ring
                .retained(source_row(index))
                .ok_or("snapshot visible row missing")?;
            if marker(index) {
                self.metadata[index] = self.capture_marker_row(row, index, force);
                continue;
            }
            let cursor_column = self.cursor_column_at(index, row.id);
            self.metadata[index] = self.capture_row_meta(row, index, cursor_column, force);
        }
        self.paint = Some(paint);
        self.style_epoch = Some(style_epoch);
        self.columns = columns;
        self.screen_epoch = term.screen_epoch;
        self.place_cursor();
        Ok(())
    }

    /// Row versions bound blink detection to copied rows, never a warm full grid scan.
    fn capture_row_meta(
        &mut self,
        row: &Row,
        index: usize,
        cursor_column: Option<usize>,
        force: bool,
    ) -> RowMeta {
        let old = self.metadata[index];
        let block_signature = self.block_signature(index);
        // Cursor isolation changes both old and new row keys without a VT write.
        let changed = force
            || old.marker
            || old.id != Some(row.id)
            || old.version != row.version
            || old.block_signature != block_signature
            || old.cursor_column != cursor_column;
        // Retained cells are owned by this slot, so a renderer can skip other slots.
        let slot = if changed {
            Some(self.copy_row(row, index))
        } else {
            old.slot
        };
        RowMeta {
            id: Some(row.id),
            version: row.version,
            marker: false,
            block_signature,
            cursor_column,
            slot,
            copied: changed,
            // Warm captures reuse this summary alongside the identical row version.
            blinking: if changed {
                row_blinks(row)
            } else {
                old.blinking
            },
        }
    }
    /// Session timers read source metadata, independently of the renderer's caches.
    pub(crate) fn has_blinking_ink(&self) -> bool {
        self.metadata.iter().any(|row| row.blinking)
    }

    /// Screen transitions distinguish identical numeric IDs in separate grids.
    pub fn screen_epoch(&self) -> u64 {
        self.screen_epoch
    }

    /// Returns the title sampled with these rows, never a newer asynchronous event.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns grid width even on an unchanged frame.
    #[must_use]
    pub fn columns(&self) -> usize {
        self.columns
    }

    /// Returns the number of visible rows without borrowing VT again.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.metadata.len()
    }

    /// Fixed grid height remains unchanged during fractional movement.
    pub fn grid_rows(&self) -> usize {
        self.grid_rows
    }

    /// Offset is already rounded once to whole device pixels.
    pub fn row_shift(&self) -> i32 {
        self.row_shift
    }

    /// Returns the current metadata for one screen row.
    #[must_use]
    pub fn row_meta(&self, index: usize) -> Option<RowMeta> {
        self.metadata.get(index).copied()
    }

    /// Borrows cell data only when this capture copied the row.
    #[must_use]
    pub fn changed_row(&self, index: usize) -> Option<&Row> {
        self.metadata.get(index)?.copied.then_some(())?;
        self.retained_row(index)
    }

    /// Supplies sealed cells when the renderer's cache differs from this slot's history.
    #[must_use]
    pub fn retained_row(&self, index: usize) -> Option<&Row> {
        let slot = self.metadata.get(index)?.slot?;
        self.copied_rows.get(slot)
    }

    /// Returns the consistent paint state sampled with the rows.
    #[must_use]
    pub fn paint(&self) -> Option<&PaintState> {
        self.paint.as_ref()
    }

    /// Seals the current fade sample while VT queries retain destination defaults.
    pub(crate) fn color_defaults(&mut self, theme: &crate::theme::Theme) {
        if let Some(paint) = self.paint.as_mut() {
            theme.write_terminal_defaults(&mut paint.colors);
        }
    }

    /// Returns the visible cursor without requiring a terminal borrow.
    #[must_use]
    pub fn cursor(&self) -> Option<CursorSnapshot> {
        self.cursor
    }

    /// Reserves a later full redraw if a builder could not consume a copy.
    pub fn invalidate(&mut self) {
        self.style_epoch = None;
    }

    /// Counts copied cell arrays separately from lightweight row metadata.
    #[must_use]
    pub fn copied_count(&self) -> usize {
        self.active_rows
    }

    /// Names the current selection without exposing mutable VT handles.
    #[must_use]
    pub fn selection(&self) -> Option<crate::selection::SelectionRange> {
        self.selection
    }

    /// Copies a reconciled range without retaining terminal handles.
    pub fn set_selection(&mut self, range: Option<crate::selection::SelectionRange>) {
        self.selection = range;
    }

    /// Borrows match paper intervals sampled with this frame's row identities.
    pub fn search(&self) -> &[crate::search::SearchFill] {
        &self.search
    }

    /// Copies visible ranges into reused storage without exposing VT handles.
    pub fn set_search(&mut self, fills: &[crate::search::SearchFill]) {
        self.search.clear();
        let first = self.metadata.first().and_then(|m| m.id);
        let last = self.metadata.last().and_then(|m| m.id);
        if let (Some(first), Some(last)) = (first, last) {
            self.search.extend(
                fills
                    .iter()
                    .copied()
                    .filter(|hit| hit.range.start.line <= last && hit.range.end.line >= first),
            );
        }
    }

    /// Reuses an old row's cell and sparse-extra capacity for a new copy.
    fn copy_row(&mut self, source: &Row, slot: usize) -> usize {
        // Count at the copy itself, not in slot validation: unchanged rows
        // must contribute no copied cells to the ordinary-publication proof.
        #[cfg(test)]
        crate::frame_builder::testing_work::charge(0, source.cells.len());
        // A screen position keeps its own allocation across slot circulation.
        // Compacting only changed rows would overwrite retained unchanged cells.
        if slot == self.copied_rows.len() {
            self.copied_rows.push(source.clone());
        } else {
            let target = &mut self.copied_rows[slot];
            // `Row`'s derived clone replaces its Vec and allocates once per
            // changed line; copy fields explicitly so Vec keeps capacity.
            target.id = source.id;
            target.version = source.version;
            target.flags = source.flags;
            target.cells.clone_from(&source.cells);
            target.blocks.clone_from(&source.blocks);
            // Derived CellExtra::clone_from replaces each cluster String. Explicit
            // field copies let String::clone_from retain warmed grapheme capacity.
            copy_extras(target, source);
        }
        self.active_rows += 1;
        slot
    }
}

/// Decodes one copied cell without consulting the live terminal.
pub fn cell_from_row(row: &Row, column: usize) -> Option<SnapshotCell<'_>> {
    let raw = *row.cells.get(column)?;
    let extra = row.extras.iter().find(|item| item.column == column);
    let text = if raw
        .flags
        .intersects(CellFlags::WIDE_SPACER | CellFlags::MULTICELL)
        || raw.ch == 0
    {
        CellText::Empty
    } else if raw.ch == Cell::CLUSTER {
        extra
            .and_then(|item| item.cluster.as_deref())
            .map_or(CellText::Empty, CellText::Cluster)
    } else {
        char::from_u32(raw.ch).map_or(CellText::Empty, CellText::Scalar)
    };
    Some(SnapshotCell {
        raw,
        text,
        underline_color: extra.and_then(|item| item.underline_color),
    })
}

#[cfg(test)]
/// Warm-copy allocation and damage selection regressions.
mod tests;
