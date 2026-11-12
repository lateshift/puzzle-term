//! Grapheme joining under DEC private mode 2027 (design §6.7).
//!
//! A continued grapheme keeps the first scalars cell origin, then stores its
//! complete UTF-8 text in sparse row metadata. A later VS16 or regional
//! indicator may widen the cluster from one to two cells; the leading cell
//! and spacer are updated together. The 32-byte cap limits hostile combining
//! runs without letting them advance the cursor.
//!
//! D-31 enables this mode by default to match common VS16 placement and
//! mode 2027 behaviour. A legacy setting and DECRST still offer scalar
//! widths to applications that measure their cursor with `wcwidth()`.
//! UAX #29 boundaries decide whether the next scalar extends the cell;
//! `unicode-width` supplies most joined widths; keycaps use emoji width only
//! when VS16 explicitly requests emoji presentation.
//!
//! A prior cluster may be plain (`Cell.ch` contains its first scalar) or
//! already sparse (`Cell::CLUSTER` points into row extras). The latter is
//! authoritative because it includes earlier combining marks. A failed
//! grapheme-boundary test returns to ordinary printing so adjacent distinct
//! characters never merge merely because they are both Unicode.
//!
//! VS16 can change the rendered width of an earlier single-cell symbol.
//! The cursor has already moved past that original cell, so widening must
//! update both the spacer and cursor state. The glyph origin remains the
//! original column. This module modifies no font or shaping state; M4 text
//! layout interprets the resulting cluster text later.
//!
//! The default path for zero-width marks remains in `print.rs`. This path
//! is invoked only when mode 2027 is active, keeping ordinary ASCII output
//! free of per-character grapheme checks (P1 and §15.2 throughput).
//!
//! A terminal receives Unicode scalars one at a time. The width chosen for
//! the first scalar is provisional: a later variation selector can change
//! the same grapheme's presentation. The screen therefore has two related
//! representations: physical cells determine cursor movement, while sparse
//! cluster text gives the renderer all scalars belonging to one glyph.
//! Neither representation can be updated alone when width changes.
//!
//! The last column is a special case. There is no spacer cell to claim on
//! that row, so a late width increase follows DECAWM: the original cell is
//! removed and the whole grapheme begins on the next row. A disabled DECAWM
//! leaves the narrow cell intact. This matches the reference behaviour and
//! avoids moving an unrelated cell outside the right margin.
//!
//! The last selector applied to one pictograph controls presentation, even
//! when another selector precedes it. A non-pictographic base such as `x`
//! ignores both selectors. Filtering those invalid selectors before
//! consulting unicode-width avoids its broader emoji heuristics.

use super::boundary::{Property, State};
use super::tail::Tail;
use super::{Terminal, clear_wide_pair, cluster_base};
use crate::{
    cell::{Cell, CellFlags},
    row::CellExtra,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The predecessor is either an ordinary grapheme or a rectangular text record.
/// This discriminator shares the existing predecessor lookup: no second feature
/// probe is added to ordinary Unicode printing.
enum Continuation {
    /// Bounded inline state for the existing scalar/cluster representation.
    Ordinary(usize, Cell, State),
    /// A slot key whose text and width are owned by the screen's sparse store.
    Multicell(u32),
}

impl Terminal {
    /// Extends one prior Unicode grapheme while retaining its cell origin.
    pub(super) fn extend_grapheme<const WATCH: bool>(&mut self, next: char) -> bool {
        // The differential reference is the pre-M16.5 string implementation,
        // selected only in tests. Production decides from state and properties.
        #[cfg(test)]
        if self.grapheme.reference {
            return self.extend_grapheme_reference::<WATCH>(next);
        }
        self.extend_grapheme_state::<WATCH>(next)
    }

    /// Performs the allocation-free boundary decision before storing an actual join.
    // Most Unicode scalars begin a new cluster. Keep this decision next to
    // printing; only an actual join enters the larger storage/width operation.
    #[inline]
    fn extend_grapheme_state<const WATCH: bool>(&mut self, next: char) -> bool {
        let property = Property::of(next);
        let previous_fact = std::mem::replace(&mut self.grapheme.property, (next as u32, property));
        let (column, cell, state) = match self.grapheme_continuation(property, previous_fact) {
            Some(Continuation::Ordinary(column, cell, state)) => (column, cell, state),
            Some(Continuation::Multicell(key)) => return self.join_multicell_at(key, next),
            None => return false,
        };
        self.finish_grapheme::<WATCH>(next, property, column, cell, state)
    }

    /// Refuses a new spacer at the final column when the client disabled DECAWM.
    fn late_widening_disabled(&self, cell: Cell, column: usize, width: usize) -> bool {
        // Width already belongs to the complete joined cluster; only a previously
        // narrow cell needs a new physical column. Existing wide text remains valid.
        width == 2
            && !cell.flags.contains(CellFlags::WIDE)
            && column + 1 == self.size().0
            && !self.autowrap
    }

    /// Decides continuity against the authoritative previous cell without owning text.
    // Both parser instantiations share this small rejection path. LLVM otherwise
    // outlines it and adds a call for every non-ASCII scalar, even with no watch.
    #[inline(always)]
    fn grapheme_continuation(
        &self,
        property: Property,
        fact: (u32, Property),
    ) -> Option<Continuation> {
        let row_index = self.cursor.row;
        // Pending wrap still points at the glyph just printed in the final
        // column. Otherwise the cursor points one column after that glyph.
        let column = if self.cursor.pending_wrap {
            self.cursor.column
        } else if self.cursor.column > 0 {
            self.cursor.column - 1
        } else {
            return None;
        };
        let row = self.screen().row(row_index)?;
        let (column, cell) = cluster_base(row, column)?;
        // A fact is reusable by scalar value, independent of grid location.
        // A control or edit changing the predecessor simply misses this key.
        let before = if fact.0 == cell.ch {
            fact.1
        } else {
            Property::of_codepoint(cell.ch)
        };
        // Classify ordinary versus sparse layout once, using the predecessor
        // test that already existed before OSC 66. Only that sparse branch
        // distinguishes a block key from row extras; scalar output pays no
        // independent MULTICELL predicate per Unicode character (M18.7/P1).
        #[cfg(test)]
        crate::scroll_operations::unicode_classification();
        if !cell
            .flags
            .intersects(CellFlags::HAS_EXTRAS | CellFlags::MULTICELL)
        {
            if !before.joins_single(property) {
                return None;
            }
        } else if cell.flags.contains(CellFlags::MULTICELL) {
            return Some(Continuation::Multicell(cell.ch));
        }
        self.sparse_continuation(row, column, cell, before, property)
    }

    /// Reconstructs suffix state only for a predecessor that may actually join.
    #[cold]
    #[inline(never)]
    fn sparse_continuation(
        &self,
        row: &crate::Row,
        column: usize,
        cell: Cell,
        before: Property,
        property: Property,
    ) -> Option<Continuation> {
        // Sparse text is authoritative after edits or legacy printing. The
        // witness reuses incremental state only for this exact row version,
        // column and screen epoch, so cursor controls cannot use stale context.
        let previous = row
            .extras
            .iter()
            .find(|extra| extra.column == column)
            .and_then(|extra| extra.cluster.as_deref());
        let state = if let Some(text) = previous {
            self.grapheme
                .tail
                .filter(|tail| tail.matches(self.screen_epoch, row, column))
                .map(|tail| tail.state)
                .or_else(|| State::from_text(text))
        } else {
            Some(State::start(before))
        };
        let state = state.filter(|state| state.joins(property))?;
        Some(Continuation::Ordinary(column, cell, state))
    }

    /// Saves suffix state only after a joined cluster has actually been stored.
    fn remember_grapheme(&mut self, state: State) {
        let column = if self.cursor.pending_wrap {
            self.cursor.column
        } else {
            self.cursor.column.saturating_sub(1)
        };
        self.grapheme.tail = self.screen().row(self.cursor.row).and_then(|row| {
            let (column, cell) = cluster_base(row, column)?;
            // A disabled autowrap can reject a late widening. In that case
            // the original scalar is still present and must not cache a join.
            (cell.ch == Cell::CLUSTER).then_some(Tail {
                epoch: self.screen_epoch,
                line: row.id,
                column,
                version: row.version,
                state,
            })
        });
    }

    /// Replaces one clustered cell and widens its occupied span if needed.
    fn store_grapheme<const WATCH: bool>(
        &mut self,
        row_index: usize,
        column: usize,
        cell: Cell,
        text: String,
        width: usize,
    ) {
        let old_width = if cell.flags.contains(CellFlags::WIDE) {
            2
        } else {
            1
        };
        if width > old_width && column + 1 == self.size().0 {
            // The reference terminal moves a late-wide base to the next line when DECAWM
            // permits it. With wrap disabled the selector cannot widen the
            // last cell, so the existing narrow base remains visible.
            if self.autowrap {
                self.move_widened_at_margin::<WATCH>(row_index, column, cell, text);
            }
            return;
        }
        let widen = width > old_width && column + 1 < self.size().0;
        let narrow = width < old_width;
        let extra = self.cluster_extra(row_index, column, column, text);
        if widen {
            // Late width changes overwrite their new spacer, even when that
            // spacer intersects a rectangular owner on another row.
            self.erase_multicell_span(row_index, column + 1, column + 2, false);
        }
        let (screen, clock) = self.screen_clock_mut();
        if let Some(row) = screen.row_mut(row_index) {
            let mut clustered = cell;
            // The sentinel points the renderer to the joined UTF-8 string;
            // retaining only the base scalar would draw emoji selectors as
            // separate tofu or lose a ZWJ ligature entirely.
            clustered.ch = Cell::CLUSTER;
            if widen {
                clustered.flags.insert(CellFlags::WIDE);
                // A VS16 or second regional indicator can turn a one-cell
                // grapheme into a two-cell emoji without moving its origin.
                clear_wide_pair::<WATCH>(row, column + 1, clock);
                // Overwriting either half of an old wide pair first removes
                // its partner, so widening cannot leave orphaned spacers.
                let mut spacer = Cell::erased(cell.bg);
                spacer.flags.insert(CellFlags::WIDE_SPACER);
                row.put_observed::<WATCH>(column + 1, spacer, clock);
            } else if narrow {
                // VS15 can turn an emoji-default wide cell into text. The
                // former spacer must not hide the next printed character.
                clustered.flags.remove(CellFlags::WIDE);
                row.put_observed::<WATCH>(column + 1, Cell::erased(cell.bg), clock);
            }
            row.put_observed::<WATCH>(column, clustered, clock);
            row.set_extra_observed::<WATCH>(extra, clock);
        }
        self.update_cursor_after_width(column, widen, narrow);
        self.screen_mut().damage.mark(row_index);
    }

    /// Tracks the number of physical columns occupied by the revised glyph.
    fn update_cursor_after_width(&mut self, column: usize, widen: bool, narrow: bool) {
        // The parser has already advanced for the base scalar. A new spacer
        // consumes one more column, but a removed spacer returns one column.
        if widen {
            let final_column = self.size().0 - 1;
            self.cursor.column = (self.cursor.column + 1).min(final_column);
            // Delayed wrap remains tied to the last occupied physical cell.
            self.cursor.pending_wrap = self.cursor.column == final_column && self.autowrap;
        } else if narrow {
            self.cursor.column = column + 1;
            self.cursor.pending_wrap = false;
        }
    }

    /// Carries a grapheme's sparse text and decorations to its new origin.
    fn cluster_extra(&self, row: usize, old: usize, new: usize, text: String) -> CellExtra {
        // OSC 8 links and underline colors belong to the glyph, not to its
        // physical column. A late width change must preserve both records.
        let old = self
            .screen()
            .row(row)
            .and_then(|row| row.extras.iter().find(|entry| entry.column == old));
        // Copy only decoration fields: cloning a cluster would allocate text
        // which is immediately replaced by the already-owned joined storage.
        CellExtra {
            column: new,
            cluster: Some(text),
            underline_color: old.and_then(|extra| extra.underline_color),
            link_id: old.and_then(|extra| extra.link_id),
        }
    }

    /// Relocates a completed wide grapheme when its base occupied column N.
    fn move_widened_at_margin<const WATCH: bool>(
        &mut self,
        row_index: usize,
        column: usize,
        cell: Cell,
        text: String,
    ) {
        let extra = self.cluster_extra(row_index, column, 0, text);
        // Removing the provisional base before wrapping prevents duplicate
        // ink when the application later repaints the old rightmost cell.
        let (screen, clock) = self.screen_clock_mut();
        if let Some(row) = screen.row_mut(row_index) {
            row.put_observed::<WATCH>(column, Cell::erased(cell.bg), clock);
        }
        self.screen_mut().damage.mark(row_index);
        self.cursor.column = column;
        self.cursor.pending_wrap = true;
        self.wrap_before_print();
        // The wrap operation may scroll the screen. Obtain the destination
        // row from the updated cursor rather than using the source index.
        let row_index = self.cursor.row;
        // The relocated pair can intersect a block already on the next row.
        // Retire its complete footprint before overwriting either witness.
        self.erase_multicell_span(row_index, 0, 2, false);
        let mut leading = cell;
        leading.ch = Cell::CLUSTER;
        leading.flags.insert(CellFlags::WIDE);
        let (screen, clock) = self.screen_clock_mut();
        if let Some(row) = screen.row_mut(row_index) {
            // A previous wide glyph can occupy either target column after
            // scrolling, so clear both old pairs before placing the new one.
            clear_wide_pair::<WATCH>(row, 0, clock);
            clear_wide_pair::<WATCH>(row, 1, clock);
            row.put_observed::<WATCH>(0, leading, clock);
            let mut spacer = Cell::erased(cell.bg);
            spacer.flags.insert(CellFlags::WIDE_SPACER);
            row.put_observed::<WATCH>(1, spacer, clock);
            row.set_extra_observed::<WATCH>(extra, clock);
        }
        self.screen_mut().damage.mark(row_index);
        // Advancing by two leaves pending wrap if the pair ends at the new
        // right margin, just like printing an initially wide scalar.
        self.advance_after_print(2, self.size().0);
    }
}

/// Resolves selectors and keycap presentation over the full cluster.
pub(crate) fn grapheme_width(text: &str) -> usize {
    // A keycap is wide only when its base is followed by VS16 before the
    // enclosing mark; a plain digit plus the mark remains one cell.
    let keycap = text.ends_with('\u{20e3}')
        && matches!(text.chars().next(), Some('#' | '*' | '0'..='9'))
        && text.chars().nth(1) == Some('\u{fe0f}');
    if keycap {
        2
    } else {
        // The most recent text/emoji selector controls presentation.
        // In particular VS15 narrows watch symbols that start wide.
        let mut base = None;
        let mut presentation = None;
        let mut filtered = [0_u8; 32];
        let mut used = 0;
        // Keep a separate filtered view for unicode-width. The authoritative
        // sparse cluster retains valid selectors for shaping and rendering.
        for scalar in text.chars() {
            let selector = matches!(scalar, '\u{fe0e}' | '\u{fe0f}');
            let valid = base.is_some_and(is_extended_pictographic);
            if selector && valid {
                // Both selectors refer to the most recent base; the final
                // one wins even if another selector sits between them.
                presentation = Some(scalar);
            }
            if !selector || valid {
                // unicode-width treats some invalid VS16 sequences as emoji.
                // The reference terminal discards those selectors before width calculation.
                // This function receives a capped stored cluster. Filtering
                // removes bytes; safe UTF-8 encoding cannot exceed that cap.
                used += scalar.encode_utf8(&mut filtered[used..]).len();
            }
            if !selector {
                base = Some(scalar);
            }
        }
        match presentation {
            Some('\u{fe0e}') => 1,
            Some('\u{fe0f}') => 2,
            _ => UnicodeWidthStr::width(std::str::from_utf8(&filtered[..used]).unwrap_or(""))
                .clamp(1, 2),
        }
    }
}

/// Queries the same pinned pictograph property as the incremental GB11 state.
fn is_extended_pictographic(ch: char) -> bool {
    // Retain the original zero-width exclusion used by the D-31 width oracle.
    Property::of(ch).presentation_candidate() && UnicodeWidthChar::width(ch) != Some(0)
}

#[cfg(test)]
mod reference;

/// Bounded text storage is entered only after a positive grapheme decision.
mod join;
