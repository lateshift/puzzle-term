//! One traversal supplies identical logical units to count and copy passes.
//! Per-cell credit also bounds unusually wide rows; no hidden full-row scan
//! sits inside a physical-row visit. Sparse rectangles contribute text once,
//! with lower-only reservations omitted exactly as in VT contents extraction.
//!
//! Full-source attachment admission excludes rectangles cut by either boundary.
//! That invariant permits streaming without retaining a growing set of block
//! keys: every root precedes its included lower fragments in document order.
//! Ordinary rows need no preliminary classification; their selected columns
//! are visited once. Flagged rows require a bounded full-row classification
//! because blank padding and independently printed spaces differ logically.
//! The second pass repeats those same decisions rather than storing per-row
//! metadata proportional to the attachment's physical length.
use super::sink::Sink;
use crate::osc::SourceReadError;
use puzzle_vt::{
    BufferView, CellFlags, PackedColor, WatchState,
    view::{CellText, LineView},
};

/// Fixed per-pass source policy; no caller may widen it between turns.
#[derive(Clone, Copy)]
pub(super) struct ReadOptions {
    /// Half-open captured gaps, not producer-supplied coordinates.
    pub(super) state: WatchState,
    /// The final physical slot is maintained by the source tracker.
    pub(super) last: usize,
    /// User copy policy, identical during counting and extraction.
    pub(super) trim: bool,
    /// The remaining combined string/state allowance supplied by the consumer.
    pub(super) limit: usize,
}

/// No retained cell/row borrow crosses an owner turn.
pub(super) struct Walker {
    /// Retained order is maintained by the source watch, not numeric LineId order.
    row: usize,
    /// Next classification or extraction cell; zero-width continuations can skip.
    /// Resumption never restarts at column zero after spending a partial turn.
    column: usize,
    /// Work phases never revisit a completed cell in the same pass.
    /// A rectangle classifier must finish before any text boundary is emitted.
    stage: Stage,
    /// A lower-only physical reservation contributes neither spaces nor newline.
    /// This predicate applies to the full row, matching the existing copy API.
    reservation_only: bool,
    /// Rectangles connect rows even without a soft-wrap flag.
    /// Joining and trailing-space trimming use separate metadata deliberately.
    continues: bool,
    /// Newline insertion depends on the last emitted logical row.
    /// Skipped lower-only rows cannot overwrite this predecessor information.
    previous_continues: bool,
    /// Leading hard-row separators are suppressed exactly once per pass.
    /// Empty ordinary rows still count, while omitted reservations do not.
    emitted: bool,
}

/// A bounded classifier avoids calling a whole-row contents helper mid-turn.
enum Stage {
    /// One credit admits the next physical row.
    /// Even an empty interval segment must not bypass bounded traversal.
    Row,
    /// Rare rectangular rows classify lower reservations with cell credits.
    /// This avoids an unbounded reservation_only scan hidden in one row visit.
    Classify,
    /// Source cells contribute complete scalar/cluster/block text.
    /// No string allocation occurs here during the first counting pass.
    Text,
    /// A completed pass cannot replay sink effects.
    Done,
}

impl Walker {
    /// Both passes start with no text, newline or continuation state.
    /// Reconstructing this value between passes resets every logical decision;
    /// the second pass cannot inherit separators or trim state from counting.
    pub(super) fn new(row: usize) -> Self {
        Self {
            // The same maintained first slot is used by count and copy.
            row,
            // A later start_text applies the first row's actual gap.
            column: 0,
            // A physical row entry is charged before examining its cells.
            stage: Stage::Row,
            // Classification is opted into only by sparse rectangular metadata.
            reservation_only: false,
            // Continuity comes from each borrowed row, never a prior pass.
            continues: false,
            // There is no predecessor before the first logical row.
            previous_continues: false,
            // Suppress an initial separator, including for a partial first row.
            emitted: false,
        }
    }

    /// Visits only work the caller admits, including classification cells.
    pub(super) fn advance(
        &mut self,
        view: BufferView<'_>,
        options: ReadOptions,
        sink: &mut Sink,
        work: &mut usize,
    ) -> Result<bool, SourceReadError> {
        // These policy values come from the original owner capture, not from
        // a producer coordinate or a later pointer position. Copying them is
        // constant-size even for a source spanning the whole history.
        // A row-entry or cell credit decreases on every traversal iteration.
        // Empty hard lines still spend the row credit; a completed pass is sticky.
        while *work > 0 {
            if matches!(self.stage, Stage::Done) {
                return Ok(true);
            }
            // Numeric identities may be out of order after IL/DL. Direct slots
            // avoid both incorrect ID filtering and repeated history searches.
            let line = view.retained_line(self.row).ok_or(SourceReadError::Gone)?;
            self.step(line, options, sink, work)?;
        }
        Ok(matches!(self.stage, Stage::Done))
    }

    /// One phase transition borrows a row only for this caller turn.
    fn step(
        &mut self,
        line: LineView<'_>,
        options: ReadOptions,
        sink: &mut Sink,
        work: &mut usize,
    ) -> Result<(), SourceReadError> {
        let ReadOptions {
            state,
            last,
            trim,
            limit,
        } = options;
        match self.stage {
            Stage::Row => self.start_row(line, state, sink, limit, work)?,
            Stage::Classify => {
                // Classification retains its column across turns and finishes
                // before deciding whether to contribute a logical separator.
                self.classify(line, work);
                if self.column == line.cells().len() {
                    self.start_text(line, state, sink, limit)?;
                }
            }
            Stage::Text => {
                // Both passes use the same sink. A zero-column end closes a
                // hard line without inspecting a nonexistent selected cell.
                self.text_cell(line, state, sink, limit, work)?;
                let end = if self.row == last {
                    state.end.1
                } else {
                    line.cells().len()
                };
                if self.column == end {
                    self.finish_row(line, last, trim, limit, sink)?;
                }
            }
            // Done is checked before entering this method; retain a total match.
            Stage::Done => {}
        }
        Ok(())
    }

    /// Ordinary rows bypass full-width classification; rare blocks opt into it.
    fn start_row(
        &mut self,
        line: LineView<'_>,
        state: WatchState,
        sink: &mut Sink,
        limit: usize,
        work: &mut usize,
    ) -> Result<(), SourceReadError> {
        // Entry itself costs a physical visit, independent of the selected cells.
        *work -= 1;
        // Tentatively omit a flagged row only until classification finds a root
        // or independently printed text. Soft wrapping is constant metadata.
        self.reservation_only = line.has_multicells();
        self.continues = line.wrapped();
        self.column = 0;
        if self.reservation_only {
            self.stage = Stage::Classify;
            Ok(())
        } else {
            self.start_text(line, state, sink, limit)
        }
    }

    /// Classification uses the complete physical row, matching reservation_only.
    fn classify(&mut self, line: LineView<'_>, work: &mut usize) {
        // Inspect one physical cell per credit, even for implicit lazy blanks.
        // The full-row predicate is required: ordinary text outside the chosen
        // columns can make a mixed row independent of its lower reservations.
        while self.column < line.cells().len() && *work > 0 {
            *work -= 1;
            let raw = line.cells()[self.column];
            // A block key is interpreted only after its explicit cell flag;
            // zero is a valid block key and is also an ordinary blank scalar.
            if raw.flags.contains(CellFlags::MULTICELL) {
                if let Some(block) = line.multicell(self.column) {
                    // The first retained fragment emits the block payload.
                    // Subsequent fragments are geometric space only.
                    self.reservation_only &= block
                        .fragments
                        .first()
                        .is_some_and(|first| first.line != line.id());
                    // A tall rectangle connects rows without requiring the
                    // application to print a soft-wrap control sequence.
                    self.continues |= block
                        .fragments
                        .last()
                        .is_some_and(|last| last.line != line.id());
                } else {
                    // Missing sparse data is not proof that a row is safely
                    // omittable; selected invalid data is refused below.
                    self.reservation_only = false;
                }
            // Printed spaces and coloured erased cells are ordinary source,
            // while untouched default blanks around a lower block are padding.
            } else if raw.ch != 0 || raw.bg != PackedColor::DEFAULT {
                self.reservation_only = false;
            }
            self.column += 1;
        }
    }

    /// Only independent logical rows contribute a boundary separator.
    /// A captured end at column zero is legal and can preserve a preceding LF.
    /// Invalid bounds are refused instead of clamped to unrelated source text.
    fn start_text(
        &mut self,
        line: LineView<'_>,
        state: WatchState,
        sink: &mut Sink,
        limit: usize,
    ) -> Result<(), SourceReadError> {
        // The interval is half-open. Neither leading program text nor the
        // following prompt can enter the exported payload.
        self.column = if line.id() == state.start.0 {
            state.start.1
        } else {
            0
        };
        let end = if line.id() == state.end.0 {
            state.end.1
        } else {
            line.cells().len()
        };
        // Do not clamp a malformed interval into a different meaning.
        // The caller receives refusal of the whole export instead.
        if self.column > end || end > line.cells().len() {
            return Err(SourceReadError::Invalid);
        }
        // A lower-only reservation contributes no line separator. The prior
        // emitted row controls joining, not the current row's wrap flag.
        if !self.reservation_only && self.emitted && !self.previous_continues {
            sink.newline(limit)?;
        }
        self.stage = Stage::Text;
        Ok(())
    }

    /// Each scalar or atomic payload is inspected once; spacers carry no text.
    fn text_cell(
        &mut self,
        line: LineView<'_>,
        state: WatchState,
        sink: &mut Sink,
        limit: usize,
        work: &mut usize,
    ) -> Result<(), SourceReadError> {
        let end = if line.id() == state.end.0 {
            state.end.1
        } else {
            line.cells().len()
        };
        // Whole atomic-unit admission ensures these lower payloads already
        // occurred at their roots earlier in this same selected source.
        if self.reservation_only {
            self.column = end;
            return Ok(());
        }
        // Empty partial rows are legal cursor-gap boundaries. They have no
        // cell to inspect, but finish_row still accounts their hard boundary.
        if self.column == end {
            return Ok(());
        }
        *work -= 1;
        let cell = line.cell(self.column).ok_or(SourceReadError::Invalid)?;
        // Hidden text was rejected at attachment admission. Refuse again
        // rather than silently omitting source if a trusted caller bypassed
        // eligibility checks when it installed its generic watch.
        if cell.raw.attrs.contains(puzzle_vt::Attrs::HIDDEN) {
            return Err(SourceReadError::Rejected);
        }
        if cell.raw.flags.contains(CellFlags::MULTICELL) {
            self.block_cell(line, end, sink, limit)?;
        } else {
            self.ordinary_cell(
                cell,
                line.id() == state.start.0,
                state.start.1,
                end,
                sink,
                limit,
            )?;
        }
        Ok(())
    }

    /// Full rectangles have one text owner; lower fragments never emit twice.
    fn block_cell(
        &mut self,
        line: LineView<'_>,
        end: usize,
        sink: &mut Sink,
        limit: usize,
    ) -> Result<(), SourceReadError> {
        let block = line
            .multicell(self.column)
            .ok_or(SourceReadError::Invalid)?;
        if self.column < block.column || block.column + block.columns > end {
            return Err(SourceReadError::Invalid);
        }
        if block
            .fragments
            .first()
            .is_some_and(|first| first.line == line.id())
        {
            if self.column != block.column {
                return Err(SourceReadError::Invalid);
            }
            sink.emit(&block.text, limit)?;
        }
        // The block's horizontal continuation cells carry no extra text.
        // Advancing by its real usize extent avoids the CellView u8 width
        // representation for a potentially wider scaled rectangle.
        self.column = block.column + block.columns;
        Ok(())
    }

    /// Whole-unit admission excludes a first spacer or a final wide lead.
    fn ordinary_cell(
        &mut self,
        cell: puzzle_vt::view::CellView<'_>,
        first_row: bool,
        start: usize,
        end: usize,
        sink: &mut Sink,
        limit: usize,
    ) -> Result<(), SourceReadError> {
        if (cell.width == 0 && self.column == start && first_row)
            || self.column + usize::from(cell.width) > end
        {
            return Err(SourceReadError::Invalid);
        }
        // Wide spacers expose width zero; joined graphemes are one borrowed
        // UTF-8 string at the lead, never separately emitted combining cells.
        if cell.width != 0 {
            // Stack UTF-8 encoding avoids allocating one String per scalar.
            let mut scalar = [0; 4];
            sink.emit(
                match cell.text {
                    CellText::Empty => " ",
                    CellText::Scalar(ch) => ch.encode_utf8(&mut scalar),
                    CellText::Cluster(text) => text,
                },
                limit,
            )?;
        }
        self.column += 1;
        Ok(())
    }

    /// Strictly advances retained order or finishes; no text-dependent retry loop.
    /// Row advancement never depends on whether a character had visible ink.
    /// Thus all-space spans and lower reservations terminate under the same cap.
    fn finish_row(
        &mut self,
        line: LineView<'_>,
        last: usize,
        trim: bool,
        limit: usize,
        sink: &mut Sink,
    ) -> Result<(), SourceReadError> {
        // Trim after all selected units, including spaces in a block payload.
        // A soft-wrapped row retains its trailing spaces as logical content.
        if !self.reservation_only {
            sink.end_row(trim && !line.wrapped(), limit)?;
            self.emitted = true;
            self.previous_continues = self.continues;
        }
        // A traversal cannot skip backwards or retry the same source row.
        // Finishing at the maintained end slot also tolerates reordered IDs.
        if self.row == last {
            self.stage = Stage::Done;
        } else {
            self.row += 1;
            self.stage = Stage::Row;
        }
        Ok(())
    }
}
