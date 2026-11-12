//! Bulk cells and frame-damage change detection for external readers (§6.13).
//!
//! Structure-of-arrays storage makes the later JS typed-array bridge copy
//! one primitive slice at a time. Callers retain and clear one `CellBlock`
//! across reads, so capacity is reused. A bounded ring records one damage
//! entry when the host seals a frame; `changes_since` merges entries after
//! a tiny sequence token instead of copying the entire scrollback.
//!
//! Dirty bits and scroll hints are already maintained by the terminal.
//! A pending unsealed frame is included too, so API callers do not have to
//! wait for rendering before seeing current edits. When a token is older
//! than the 64-entry log, the observer gets a full retained-line refresh.
use super::*;
use std::collections::VecDeque;

/// Rare rectangular transfer is selected by the row summary.
mod multicell;

/// Existing §6.13 frame-journal bound; also defines complete warm-up for hosts.
/// Naming it does not change the historical 64-entry retention policy.
pub const CHANGE_LOG_FRAMES: usize = 64;

/// Caller-owned column data with rare clusters in a side table.
///
/// Primitive vectors preserve the cell order of retained lines so an
/// adapter can transfer them as typed arrays without per-cell objects.
#[derive(Clone, Debug, Default)]
pub struct CellBlock {
    /// Opt-in sparse OSC 66 records, keyed by slot and containing stable fragments.
    /// Leave None to copy primitive/cluster arrays without owning block metadata.
    pub multicells: Option<Vec<(u32, crate::multicell::Multicell)>>,
    /// Code point or `Cell::CLUSTER` sentinel for each copied column.
    pub code_points: Vec<u32>,
    /// Zero for a wide spacer, otherwise one or two columns.
    pub widths: Vec<u8>,
    /// Tagged foreground values, left unresolved for theme changes.
    pub foregrounds: Vec<u32>,
    /// Tagged background values.
    pub backgrounds: Vec<u32>,
    /// Inline SGR attribute bits, including underline style.
    pub attributes: Vec<u16>,
    /// OSC 8 handles are optional and generation checked on lookup.
    ///
    /// Consumers must resolve them through `LinkTable` before opening.
    pub link_ids: Vec<Option<u32>>,
    /// Full grapheme strings indexed by the flat column offset.
    pub clusters: Vec<(usize, String)>,
}

impl CellBlock {
    /// Reuses all allocated primitive buffers for the next caller read.
    ///
    /// Clearing lengths preserves capacities across repeated JS queries.
    pub fn clear(&mut self) {
        if let Some(records) = &mut self.multicells {
            records.clear();
        }
        self.code_points.clear();
        self.widths.clear();
        self.foregrounds.clear();
        self.backgrounds.clear();
        self.attributes.clear();
        self.link_ids.clear();
        self.clusters.clear();
    }
}

/// Small sequence token for one observer's last consumed frame.
///
/// The token belongs to one observed buffer; callers discard it after a
/// screen switch rather than comparing primary IDs to alternate IDs.
#[derive(Clone, Debug, Default)]
pub struct ChangeToken {
    /// Last sealed frame at this observation point.
    sequence: u64,
    /// Actual full-grid scroll count excludes region inserts and reflow.
    scroll_total: u64,
    /// Screen switches require a full refresh instead of comparing two grids.
    active: Option<ActiveScreen>,
}

/// One sealed frame's affected IDs and scroll movement.
#[derive(Clone, Debug, Default)]
struct FrameChange {
    /// Monotonic frame sequence, never reused within one terminal.
    sequence: u64,
    /// Active screen when this frame was sealed.
    active: Option<ActiveScreen>,
    /// Dirty IDs copied from visible rows, coalesced by numeric identity.
    lines: Vec<RangeInclusive<LineId>>,
    /// Full-grid upward movement in this frame.
    scroll_delta: i64,
}

/// Bounded journal of frame-level content changes.
#[derive(Clone, Debug, Default)]
pub(crate) struct ChangeLog {
    /// The oldest entry is recycled after 64 frames (§6.13).
    entries: VecDeque<FrameChange>,
    /// Sequence of the latest sealed frame.
    sequence: u64,
}

impl ChangeLog {
    /// Reports retained frame entries to the bounded-journal regression.
    #[cfg(test)]
    pub(crate) fn entries_len(&self) -> usize {
        self.entries.len()
    }

    /// Seals one frame using the grid's existing damage bits.
    ///
    /// Reusing the oldest entry retains its range-vector capacity after
    /// the log reaches 64 frames, so routine frames allocate nothing.
    pub(crate) fn record(&mut self, active: ActiveScreen, screen: &crate::Screen) {
        let mut entry = if self.entries.len() == CHANGE_LOG_FRAMES {
            self.entries.pop_front().unwrap_or_default()
        } else {
            FrameChange::default()
        };
        entry.lines.clear();
        self.sequence = self.sequence.saturating_add(1);
        entry.sequence = self.sequence;
        entry.active = Some(active);
        entry.scroll_delta = i64::from(screen.damage.scrolled);
        append_damage(screen, &mut entry.lines);
        self.entries.push_back(entry);
    }

    /// Returns retained entries after a token, or `None` when it expired.
    fn since(&self, token: &ChangeToken) -> Option<ChangeSet> {
        // A wrapped journal cannot answer which of the intervening
        // frames changed a row; the caller then requests a full refresh.
        let oldest = self
            .entries
            .front()
            .map_or(self.sequence.saturating_add(1), |entry| entry.sequence);
        if token.sequence.saturating_add(1) < oldest {
            return None;
        }
        let mut result = ChangeSet::default();
        for entry in self
            .entries
            .iter()
            .filter(|entry| entry.sequence > token.sequence)
        {
            if entry.active != token.active {
                return None;
            }
            result.lines.extend(entry.lines.iter().cloned());
            result.scroll_delta = result.scroll_delta.saturating_add(entry.scroll_delta);
        }
        Some(result)
    }
}

/// Adds dirty visible identities, including newly exposed scroll rows.
fn append_damage(screen: &crate::Screen, output: &mut Vec<RangeInclusive<LineId>>) {
    for row in 0..screen.ring.visible_rows() {
        let scrolled_in = row
            >= screen
                .ring
                .visible_rows()
                .saturating_sub(screen.damage.scrolled.max(0) as usize);
        if !(screen.damage.full || screen.damage.is_dirty(row) || scrolled_in) {
            continue;
        }
        if let Some(line) = screen.row(row) {
            output.push(line.id..=line.id);
        }
    }
    coalesce(output);
}

/// Sorts and combines adjacent numeric IDs after region edits or scrolls.
fn coalesce(ranges: &mut Vec<RangeInclusive<LineId>>) {
    ranges.sort_unstable_by_key(|range| *range.start());
    let mut write = 0;
    for read in 0..ranges.len() {
        let range = ranges[read].clone();
        if write > 0 && range.start().0 <= ranges[write - 1].end().0.saturating_add(1) {
            let end = *ranges[write - 1].end().max(range.end());
            ranges[write - 1] = *ranges[write - 1].start()..=end;
        } else {
            ranges[write] = range;
            write += 1;
        }
    }
    ranges.truncate(write);
}

/// Changed line spans and the amount of newly appended content.
///
/// A scroll delta can be nonzero even when retained rows kept their exact
/// cells and versions, because their viewport positions changed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChangeSet {
    /// Inclusive adjacent ID ranges with new or rewritten content.
    pub lines: Vec<RangeInclusive<LineId>>,
    /// Number of appended rows since the token, saturating at `i64::MAX`.
    pub scroll_delta: i64,
}

impl BufferView<'_> {
    /// Fills reused arrays in retained line order without per-cell objects.
    ///
    /// The cluster side table is populated only when a row extra exists;
    /// ordinary ASCII copies only fixed-size primitive values.
    pub fn cells_into(self, range: RangeInclusive<LineId>, block: &mut CellBlock) {
        block.clear();
        for line in self.lines(range) {
            if line.has_multicells() {
                // Dispatch once per flagged row; ordinary copies keep their loop.
                block.append_multicell_row(line);
                continue;
            }
            for (column, cell) in line.cells().iter().enumerate() {
                let offset = block.code_points.len();
                block.code_points.push(cell.ch);
                block
                    .widths
                    .push(if cell.flags.contains(CellFlags::WIDE_SPACER) {
                        0
                    } else if cell.flags.contains(CellFlags::WIDE) {
                        2
                    } else {
                        1
                    });
                block.foregrounds.push(cell.fg.0);
                block.backgrounds.push(cell.bg.0);
                block.attributes.push(cell.attrs.bits());
                let extra = line.row.extras.iter().find(|item| item.column == column);
                block.link_ids.push(extra.and_then(|item| item.link_id));
                if let Some(cluster) = extra.and_then(|item| item.cluster.as_ref()) {
                    block.clusters.push((offset, cluster.clone()));
                }
            }
        }
    }

    /// Captures the current frame sequence without copying scrollback.
    ///
    /// This token is cheap enough for every JS subscriber to retain.
    #[must_use]
    pub fn change_token(self) -> ChangeToken {
        ChangeToken {
            sequence: self.term.change_log.sequence,
            scroll_total: self.term.screen().ring.scroll_total(),
            active: Some(self.active()),
        }
    }

    /// Merges sealed frame entries and the current unsealed damage.
    ///
    /// An expired token or screen switch requests a full retained-line
    /// refresh, because intermediate damage cannot be reconstructed.
    #[must_use]
    pub fn changes_since(self, token: &ChangeToken) -> ChangeSet {
        let logged = self.term.change_log.since(token);
        let full = token.active != Some(self.active()) || logged.is_none();
        let mut result = logged.unwrap_or_default();
        if full {
            result.lines.clear();
            result
                .lines
                .extend(self.term.screen().ring.iter().map(|row| row.id..=row.id));
        } else {
            // The parser may have changed rows since the last frame seal;
            // merge those dirty IDs so an immediate API read is current.
            append_damage(self.term.screen(), &mut result.lines);
        }
        result.scroll_delta = self
            .term
            .screen()
            .ring
            .scroll_total()
            .saturating_sub(token.scroll_total)
            .min(i64::MAX as u64) as i64;
        coalesce(&mut result.lines);
        result
    }
}
