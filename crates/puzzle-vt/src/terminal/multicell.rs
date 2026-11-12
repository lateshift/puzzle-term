//! OSC 66 placement keeps rectangles atomic and cursor coordinates physical.
//! All mutable records belong to the active ring. Rendering never participates
//! in mutation or width calculation; D-31 and explicit protocol widths decide it.
//!
//! Integer scale multiplies both the physical height and the requested base width.
//! Fraction n/d and alignment are retained as ink metadata; they never reserve a
//! fractional cell or change CPR coordinates. Font metrics are therefore absent.
//! Explicit w>0 owns one text run. Implicit w=0 separates the same clusters as
//! normal mode-2027 output, while legacy mode separates nonzero-width scalars.
//! A mark alone extends the previous cell and does not create an empty rectangle.
//! Placement first fits width at the active right margin, then makes room for its
//! entire height with a single region scroll. Only then are fragments installed.
//! Reserving lower rows overwrites existing contents; it does not insert lines.
//! A failed fit leaves no partially installed record; the parser consumes the
//! entire operation regardless, so following terminal output remains intact.
//! Every fragment retains its original vertical offset and stable line identity,
//! so scrolling/eviction can clip the root without duplicating logical text.
//! Removal clears the full retained footprint with the current BCE background,
//! and damages all visible fragments rather than only the addressed physical row.
//! Fragment writes share the same Terminal clock as ordinary scalar stores.
//! A changed rectangle must invalidate source witnesses and row caches together,
//! including fragments already retained in scrollback. Relocation alone keeps
//! source versions, whereas an erased or overwritten fragment is a new write.
use super::Terminal;
// Keep the protocol's value types independent of parser and rendering ownership.
use crate::{
    Cell, CellFlags, RowFlags,
    multicell::{Fragment, Multicell, TextSizing},
};
use unicode_segmentation::UnicodeSegmentation;

/// Sparse editing intersects indexes, not every cell in the screen/history.
mod editing;
/// Late D-31 width changes preserve rectangular occupancy and style.
mod joining;
/// Joining and lower-fragment skipping share the regular print boundary.
mod printing;

/// Placement and removal own every fragment mutation before exposing a block.
impl Terminal {
    /// Splits implicit-width text using the existing width policy, or keeps an explicit run.
    pub(crate) fn print_sized(&mut self, text: &str, sizing: TextSizing) {
        // Explicit width describes one indivisible run, even with many graphemes.
        if sizing.width != 0 {
            self.place_multicell(text, sizing, usize::from(sizing.width));
        } else if self.modes.get(true, 2027) == Some(true) {
            // Each iteration consumes one immutable input grapheme, including
            // discarded placements. No cursor retry can rewind that iterator.
            for cluster in text.graphemes(true) {
                // D-31's selector policy is authoritative, including invalid selectors.
                // A stored grapheme is bounded independently of an explicit text run.
                let mut end = cluster.len().min(32);
                while !cluster.is_char_boundary(end) {
                    // A byte cap must never split a UTF-8 scalar at its last byte.
                    // The remaining prefix length strictly decreases to a boundary.
                    end -= 1;
                }
                let cluster = &cluster[..end];
                // Match normal scalar printing before a cluster forms: regional
                // indicators and the user's ambiguous-width policy apply here too.
                let Some(first) = cluster.chars().next() else {
                    continue;
                };
                if unicode_width::UnicodeWidthChar::width(first) == Some(0) {
                    // A leading mark is ordinary continuation, not a new sized item.
                    for mark in cluster.chars() {
                        self.print_char(mark);
                    }
                    continue;
                }
                let width = if cluster.chars().count() == 1 {
                    // The first scalar uses the same provisional width as print_char.
                    self.cell_width(first)
                } else {
                    // A completed cluster uses D-31's selector-aware width oracle.
                    super::print::grapheme::grapheme_width(cluster)
                };
                self.place_multicell(cluster, sizing, width);
            }
        } else {
            // Legacy mode retains per-scalar occupancy; zero-width marks still
            // extend the previous block without creating a reserved rectangle.
            for scalar in text.chars() {
                if unicode_width::UnicodeWidthChar::width(scalar) == Some(0) {
                    // Scaling is per operation, so a continuation inherits its owner.
                    self.join_multicell(scalar);
                } else {
                    let width = self.cell_width(scalar);
                    self.place_multicell(scalar.encode_utf8(&mut [0; 4]), sizing, width);
                }
            }
        }
    }
    /// Reserves every fragment before advancing along the first row only.
    fn place_multicell(&mut self, text: &str, sizing: TextSizing, base_width: usize) {
        let (screen_width, screen_height) = self.size();
        let height = usize::from(sizing.scale);
        // Check the complete footprint before touching cells or cursor state.
        // The protocol cap is small, but checked multiplication is the admission rule.
        let Some(width) = base_width.checked_mul(height) else {
            return;
        };
        if width > screen_width
            || height > screen_height
            || height > self.margins.bottom - self.margins.top + 1
        {
            // Kitty screen.c handle_fixed_width_multicell_command bounds height
            // by the scrolling band as well as the screen. See
            // https://github.com/kovidgoyal/kitty/blob/master/kitty/screen.c#L1493-L1497
            // Reject before any
            // wrap/scroll: repetition cannot make an over-tall rectangle fit.
            return;
        }
        // Width and height fitting finish before any fragment is published.
        if !self.fit_multicell_width(width, screen_width)
            || !self.fit_multicell_height(height, screen_height)
        {
            return;
        }
        self.reserve_multicell(text, sizing, width, height);
        self.finish_sized_storage();
    }
    /// Reconciles rare text growth with tracked history eviction.
    fn finish_sized_storage(&mut self) {
        let (screen, tracked) = match self.active {
            super::ActiveScreen::Primary => (&mut self.primary, &mut self.tracked),
            super::ActiveScreen::Alternate => (&mut self.alternate, &mut self.tracked),
        };
        screen.ring.finish_multicell_mutation(tracked);
    }
    /// Fits one whole horizontal span, using active margins and delayed wrap.
    fn fit_multicell_width(&mut self, width: usize, screen_width: usize) -> bool {
        let left = if self.lrmm_enabled {
            // DECSLRM applies only while its enabling mode is active.
            self.margins.left
        } else {
            0
        };
        let right = if self.lrmm_enabled {
            self.margins.right + 1
        } else {
            screen_width
        };
        if width > right - left {
            // Wrapping cannot make an item wider than the region fit.
            return false;
        }
        // A delayed wrap and an overflowing rectangle use the same physical line transition.
        if self.autowrap && (self.cursor.pending_wrap || self.cursor.column + width > right) {
            return self.wrap_sized(left);
        } else if self.cursor.column + width > right {
            // Kitty specifies backing up to fit when automatic wrapping is disabled.
            self.cursor.column = right - width;
        }
        true
    }
    /// Consumes the exact vertical overflow in one scroll, or rejects an immovable fit.
    fn fit_multicell_height(&mut self, height: usize, screen_height: usize) -> bool {
        let in_band = (self.margins.top..=self.margins.bottom).contains(&self.cursor.row);
        let bottom = if in_band {
            self.margins.bottom + 1
        } else {
            screen_height
        };
        let overflow = (self.cursor.row + height).saturating_sub(bottom);
        if overflow > 0 {
            // Outside the scroll region there is no legal scroll to create room.
            // Discard rather than moving unrelated rows or retrying a no-op LF.
            if !in_band {
                return false;
            }
            // Height was admitted against the band's complete height before
            // wrapping. Hence overflow <= cursor.row - top: subtraction stays
            // inside the band. This single scroll consumes all overflow; no
            // loop depends on LF making room one row at a time.
            self.scroll_region_up(overflow);
            self.cursor.row -= overflow;
        }
        true
    }
    /// Wraps only if LF can advance a row or replace the bottom row by scrolling.
    fn wrap_sized(&mut self, left: usize) -> bool {
        let (_, rows) = self.size();
        // LF is a no-op at the physical bottom outside DECSTBM. Decide before
        // WRAPPED or x changes: resetting x there would revisit the same lower
        // reservations forever. At the region bottom, scrolling is progress
        // even though the numeric cursor row itself remains unchanged.
        if self.cursor.row + 1 >= rows && self.cursor.row != self.margins.bottom.min(rows - 1) {
            return false;
        }
        let y = self.cursor.row;
        let (screen, clock) = self.screen_clock_mut();
        if let Some(row) = screen.row_mut(y) {
            // WRAPPED describes this outgoing row, not the row reached by LF.
            row.flags.insert(RowFlags::WRAPPED);
            row.version = clock.written::<true>(row, row.cells.len()..row.cells.len() + 1);
        }
        self.line_feed();
        self.cursor.column = left;
        true
    }
    /// Erases overlapping blocks and ordinary wide partners before installing all fragments.
    fn reserve_multicell(&mut self, text: &str, sizing: TextSizing, width: usize, height: usize) {
        let (top, column) = (self.cursor.row, self.cursor.column);
        for y in top..top + height {
            // Repair any old wide partners and retire intersected rectangles first.
            self.erase_span(y, column, column + width, false);
        }
        // Stable row identities let region scrolls carry fragments without
        // rewriting every block's absolute vertical coordinate on each LF.
        let fragments = (0..height)
            .filter_map(|offset| {
                self.screen().row(top + offset).map(|row| Fragment {
                    line: row.id,
                    offset: offset as u8,
                })
            })
            .collect();
        let block = Multicell {
            // Copy text exactly once; cells and row indexes contain only slot keys.
            text: text.into(),
            sizing,
            column,
            columns: width,
            fragments,
            revision: 0,
        };
        // The row cells retain the pen's inline colors/protection. The owned
        // payload is stored once regardless of the rectangle's area.
        let pen = self.pen;
        let (screen, clock) = self.screen_clock_mut();
        if let Some(key) = screen.ring.insert_multicell(block, pen, clock) {
            // Extras stay on the first occupied cell, exactly as with ordinary text.
            if self.current_link.is_some() || self.underline_color.is_some() {
                let extra = crate::CellExtra {
                    column,
                    cluster: None,
                    link_id: self.current_link,
                    underline_color: self.underline_color,
                };
                let (screen, clock) = self.screen_clock_mut();
                if let Some(row) = screen.row_mut(top) {
                    row.set_extra(extra, clock);
                }
            }
            debug_assert!(self.screen().ring.multicell(key).is_some());
            // Rendering later uses row versions and damage to rebuild every fragment.
            for y in top..top + height {
                self.screen_mut().damage.mark(y);
            }
            self.advance_after_print(width, self.size().0);
        }
    }
    /// Erases a record and damages all its visible fragments, including unrelated rows.
    pub(crate) fn remove_multicell(&mut self, key: u32) {
        let Some(block) = self.screen().ring.multicell(key) else {
            return;
        };
        let fragments = block.fragments.clone();
        // The inline coordinate copy ends the store borrow before row mutation.
        let history = self.screen().ring.scrollback_len();
        for fragment in &fragments {
            if let Some(index) = self.screen().ring.index_of(fragment.line)
                && index >= history
            {
                // Retained history has no active damage bit; visible fragments do.
                self.screen_mut().damage.mark(index - history);
            }
        }
        // BCE applies to the complete erased footprint, including lower rows.
        let blank = Cell::erased(self.pen.bg);
        let (screen, clock) = self.screen_clock_mut();
        screen.ring.erase_multicell(key, blank, clock);
    }
}

#[cfg(test)]
/// Deterministic operation guards and rectangle-placement termination cases.
mod tests;
