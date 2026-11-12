//! User row projections seal the same source data through a mapped visible window.
//! The source remains untouched; caches key rows by their real identity/version.
//! Maps are borrowed from the owner during capture, never cloned into a slot.
//! Ordinary captures keep the original contiguous row-copy loop.
//!
//! The closure is used synchronously while the Session still owns the source.
//! Only copied cells/row identities survive publication. Images and overlay
//! buffers borrow the same viewport mapping before that owner resumes parsing;
//! they cannot consult a different generation or retain the closure.
//! Bounding count before row copying prevents hostile map lengths from making
//! an unbounded snapshot. A bad retained index is reported by the shared row
//! copier, leaving the caller with an error rather than a partial presentation.
//!
//! A marked omission reserves one blank UI row beside its retained context.
//! Its identity lets overlays anchor a count without inventing a VT line. The
//! marker's cache key is separate from source ink, so opening it recaptures the
//! source rather than reusing blank geometry. Warm marker copies never visit
//! cluster strings or clone sparse block payloads. This matters for emoji and
//! combining text: copying then clearing extras would allocate every frame.
//! The generic capture loop specializes the no-marker closure away.
use super::FrameSnapshot;
use puzzle_vt::Terminal;

impl FrameSnapshot {
    /// Copies an already validated display-to-source window, without changing VT.
    /// Hidden source remains available to contents, selection and search queries.
    /// An out-of-range retained index is an error, never an incomplete publication.
    pub fn capture_rows(
        &mut self,
        term: &Terminal,
        style_epoch: u64,
        rows: &[usize],
        row_shift: i32,
    ) -> Result<(), String> {
        self.capture_mapped(term, style_epoch, rows.len(), row_shift, &|index| {
            rows[index]
        })
    }

    /// Seals a derived row map without an extra temporary source-index vector.
    /// Ordinary callers keep using capture_scrolled and do no index-copy work.
    pub fn capture_mapped(
        &mut self,
        term: &Terminal,
        style_epoch: u64,
        count: usize,
        row_shift: i32,
        source_row: &impl Fn(usize) -> usize,
    ) -> Result<(), String> {
        // A display window cannot exceed the grid plus its fractional edge row.
        // Refuse hostile dimensions before reserving any additional allocation.
        if count > term.size().1.saturating_add(usize::from(row_shift != 0)) {
            return Err("projected snapshot exceeds visible grid".into());
        }
        self.capture_selected(term, style_epoch, count, row_shift, source_row, &|_| false)
    }

    /// Use the same viewport mapping for every publication adapter.
    pub(crate) fn capture_session_view(
        &mut self,
        term: &Terminal,
        epoch: u64,
        viewport: &crate::viewport::Viewport,
    ) -> Result<(), String> {
        if viewport.projected() {
            self.capture_selected(
                term,
                epoch,
                viewport.visible_count(term.size().1),
                viewport.device_shift(),
                &|row| viewport.source_row(row).unwrap_or(usize::MAX),
                &|row| viewport.marker_row(row),
            )
        } else {
            self.capture_scrolled(term, epoch, viewport.top_index(), viewport.device_shift())
        }
    }

    /// Reuse a blank presentation row without ever copying hidden source clusters.
    /// The live row supplies identity/width only; copy/search still use all its text.
    pub(super) fn capture_marker_row(
        &mut self,
        source: &puzzle_vt::Row,
        index: usize,
        force: bool,
    ) -> super::RowMeta {
        let old = self.metadata[index];
        let changed = force || !old.marker || old.id != Some(source.id);
        if changed {
            // Each display position retains its own allocation, as ordinary capture
            // does. No source String or sparse rectangle is cloned then discarded.
            if index == self.copied_rows.len() {
                self.copied_rows
                    .push(puzzle_vt::Row::new(source.id, source.cells.len()));
            }
            let row = &mut self.copied_rows[index];
            row.id = source.id;
            row.version = 0;
            row.cells
                .resize(source.cells.len(), puzzle_vt::Cell::default());
            row.cells.fill(puzzle_vt::Cell::default());
            row.extras.clear();
            row.blocks.clear();
            row.flags = puzzle_vt::row::RowFlags::empty();
            self.active_rows += 1;
        }
        // Test identity rather than pre-projection cursor row numbers: folds and
        // scrollback have already changed which physical row is displayed here.
        if self
            .cursor
            .is_some_and(|cursor| cursor.view.line == source.id)
        {
            self.cursor = None;
        }
        // Zero is reserved for blank ink here. Unfolding sees marker=true and
        // recaptures the original source version before any cache can reuse it.
        super::RowMeta {
            id: Some(source.id),
            version: 0,
            marker: true,
            slot: Some(index),
            copied: changed,
            ..Default::default()
        }
    }
}
