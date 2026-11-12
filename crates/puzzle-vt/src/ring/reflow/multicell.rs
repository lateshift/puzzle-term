//! Atomic rectangular reflow, selected only for a screen with live OSC 66 text.
//!
//! Source rows remain immutable until their cells and anchors have been mapped.
//! Output reservations share the real destination cells, so placement tests the
//! whole rectangle and cannot overlap text already packed on a lower row.
//!
//! A scaled character has two different orders: its payload belongs to the
//! logical text stream, while its fragments reserve physical screen rows.
//! Flattening those rows would copy the payload several times and turn the
//! unused space beside a lower fragment into text. Instead, `placed` records
//! which owners already entered the destination and `fit` consults reservations.
//!
//! Packing can reach a future row before consuming the corresponding source
//! row. Source storage therefore remains separate until consume returns. Only
//! then may its allocation become a destination row. This is also why the
//! source coordinate map is keyed by old identity, not current ring index.
//!
//! Every old physical cell has a mapping entry for tracked anchors. A block
//! maps all fragments at once; later visits to its lower rows do not overwrite
//! those mappings. An oversized block leaves Gone entries, so an anchor cannot
//! silently jump to an unrelated cell after the rectangle is discarded.
//!
//! Fresh destination identities invalidate row caches without depending on a
//! particular snapshot slot. Content versions inherit the maximum source stamp:
//! reflow is a relocation, not an edit. Selection also verifies logical content.
//! History accounting is reconciled after packing.
//!
//! This builder is cold and exists only when live records are present. The
//! ordinary reflow implementation, its storage and its operation counts remain
//! independent of OSC 66. The real destination cells provide the temporary
//! occupancy map rather than allocating another screen-sized bitmap.
use super::super::RowRing;
use crate::multicell::{BlockRef, Fragment, MulticellStore};
use crate::tracked::PositionState;
use crate::{CellFlags, LineId, Row, RowFlags, TrackedPositions};
use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};

/// Reflow copies geometry, while the live store remains the payload's owner.
mod source;
use source::SourceBlock;

/// Temporary destination and sparse identity maps for an affected screen.
struct Builder {
    /// Destination rows include future lower reservations as soon as needed.
    rows: VecDeque<Box<Row>>,
    /// Source rows already consumed become reusable occupancy/destination storage.
    reusable: VecDeque<Box<Row>>,
    /// Physical output cursor; reservations may require it to skip cells.
    y: usize,
    /// Next candidate column on the current destination row.
    x: usize,
    /// New grid width, already clamped by RowRing.
    width: usize,
    /// New visible height bounds the maximum admitted rectangle.
    height: usize,
    /// Fresh monotonic row identities, never source identities.
    next_id: u64,
    /// Old physical coordinates map to new physical coordinates or Gone.
    positions: HashMap<LineId, Vec<PositionState>>,
    /// Each rectangular fragment inherits its own source row stamp.
    versions: HashMap<LineId, u64>,
    /// One source block can be encountered through several fragments.
    placed: HashSet<u32>,
    /// Mutable records are private until the rebuilt ring is published.
    store: Box<MulticellStore>,
}

/// Destination packing and physical anchor mapping happen together.
impl Builder {
    /// Initializes maps once; ordinary reflow never constructs this builder.
    fn new(ring: &mut RowRing, width: usize, height: usize, old: &VecDeque<Box<Row>>) -> Self {
        let positions = old
            .iter()
            .map(|row| (row.id, vec![PositionState::Gone; row.cells.len()]))
            .collect();
        // Each coordinate starts Gone; only an item actually retained can map it.
        Self {
            rows: VecDeque::new(),
            reusable: VecDeque::new(),
            y: 0,
            x: 0,
            width,
            height,
            next_id: ring.next_id,
            positions,
            versions: old.iter().map(|row| (row.id, row.version)).collect(),
            placed: HashSet::default(),
            store: ring.multicells.take().unwrap_or_default(),
        }
    }
    /// Allocates only rows actually reached by text or a rectangle reservation.
    fn ensure(&mut self, y: usize) {
        // Each push reduces the finite missing-row count y + 1 - rows.len().
        while self.rows.len() <= y {
            let id = LineId(self.next_id);
            let mut row = self
                .reusable
                .pop_back()
                .unwrap_or_else(|| Box::new(Row::new(id, self.width)));
            row.cells.resize(self.width, crate::Cell::default());
            // The real recycled destination cells are the occupancy scratch; no
            // second cell-sized bitmap is initialized for each affected band.
            row.clear_for_reflow(id, crate::PackedColor::DEFAULT);
            self.rows.push_back(row);
            // A recycled allocation still receives a new document identity.
            self.next_id = self.next_id.saturating_add(1);
        }
    }
    /// Converts the packing cursor to a stable destination position.
    fn position(&mut self) -> PositionState {
        // An empty hard line can still own the cursor or a tracked endpoint.
        self.ensure(self.y);
        PositionState::Present {
            line: self.rows[self.y].id,
            column: self.x.min(self.width - 1),
        }
    }
    /// Wraps only when text actually continues beyond the destination width.
    fn wrap(&mut self) {
        self.ensure(self.y);
        self.rows[self.y].flags.insert(RowFlags::WRAPPED);
        // The predecessor stores continuity; the next row starts at column zero.
        self.y += 1;
        self.x = 0;
    }
    /// Finds space for an indivisible item, skipping existing lower reservations.
    fn fit(&mut self, columns: usize, height: usize) -> bool {
        if columns == 0 || height == 0 || columns > self.width || height > self.height {
            // A fresh row cannot fit an oversized item either. Refuse instead
            // of allocating/wrapping forever; ordinary callers already admit it.
            return false;
        }
        // Only the finite existing row prefix contains reservations. New rows
        // are blank. Candidate rank y*(width+1)+x strictly increases at every
        // collision or wrap (including x==width), so remaining candidates before
        // the first fresh row strictly decrease. Width admission makes it fit.
        loop {
            if self.x + columns > self.width {
                self.wrap();
            }
            self.ensure(self.y + height - 1);
            // Testing all future fragments prevents a tall block from overlapping
            // an earlier root whose lower reservation lies on this candidate row.
            let collision = (self.y..self.y + height).find_map(|y| {
                (self.x..self.x + columns)
                    .find(|&x| self.rows[y].cells[x].flags.contains(CellFlags::MULTICELL))
            });
            let Some(x) = collision else {
                return true;
            };
            // Advancing past the colliding cell guarantees progress. A further
            // iteration checks the complete rectangle rather than just its root.
            self.x = x + 1;
        }
    }
    /// Copies an ordinary scalar/wide pair and its one sparse extra.
    fn scalar(&mut self, source: &Row, column: usize, span: usize) {
        if !self.fit(span, 1) {
            return;
        }
        for offset in 0..span {
            // Wide leads and spacers move together, preserving their two-cell span.
            let target = self.position();
            if let Some(map) = self.positions.get_mut(&source.id) {
                map[column + offset] = target;
            }
            self.rows[self.y].cells[self.x] = source.cells[column + offset];
            // A relocation inherits its source mutation, never a new write.
            self.rows[self.y].version = self.rows[self.y].version.max(source.version);
            self.x += 1;
        }
        if let Some(extra) = source.extras.iter().find(|extra| extra.column == column) {
            // Rare cluster/link data follows its lead, with a destination column.
            let mut extra = extra.clone();
            extra.column = self.x - span;
            self.rows[self.y].copy_extra(extra);
        }
        self.rows[self.y].flags |= source.flags & RowFlags::IMAGE_PLACEHOLDERS;
    }
    /// Places a record once, preserving original offsets of clipped roots.
    fn block(&mut self, source: &Row, reference: BlockRef) {
        if !self.placed.insert(reference.key) {
            // Lower fragments were mapped when the owner was first encountered.
            return;
        }
        let Some(old) = SourceBlock::capture(&self.store, reference.key) else {
            return;
        };
        if old.columns > self.width || usize::from(old.scale) > self.height {
            // No partial rectangle is published; old fragment anchors remain Gone.
            self.store.remove(reference.key);
            return;
        }
        if !self.fit(old.columns, old.fragments.len()) {
            self.store.remove(reference.key);
            return;
        }
        // A clipped root retains its original vertical offsets, but only surviving
        // fragments need destination rows. The payload still has one logical owner.
        // Only the inline coordinates need an owned copy. Cloning the payload
        // would discard spare String capacity and cannot describe live storage.
        let mut fragments = old.fragments.clone();
        for (dy, fragment) in old.fragments.iter().enumerate() {
            self.copy_fragment(&old, fragment, dy, reference, source);
            fragments[dy].line = self.rows[self.y + dy].id;
        }
        if let Some(record) = self.store.get_mut(reference.key) {
            // Text allocation is unchanged; only geometry and revision mutate.
            record.column = self.x;
            record.fragments = fragments;
            record.revision = record.revision.saturating_add(1);
        }
        if let Some(extra) = source
            .extras
            .iter()
            .find(|extra| extra.column == old.column)
        {
            // Only the logical owner's first cell carries link/underline metadata.
            let mut extra = extra.clone();
            extra.column = self.x;
            self.rows[self.y].copy_extra(extra);
        }
        self.x += old.columns;
    }
    /// Installs one fragment and maps every source cell covered by that fragment.
    fn copy_fragment(
        &mut self,
        old: &SourceBlock,
        fragment: &Fragment,
        dy: usize,
        reference: BlockRef,
        source: &Row,
    ) {
        let row = &mut self.rows[self.y + dy];
        row.version = row.version.max(
            self.versions
                .get(&fragment.line)
                .copied()
                .unwrap_or(source.version),
        );
        // Destination rows already have their final identities. Mapping now
        // lets selection and cursor reuse the same relocation as ordinary text.
        // Record capacity is charged on one fragment, not on every covered cell.
        if dy == 0 {
            // Captured from the actual retained record, never a payload clone.
            row.block_bytes += old.charge;
        }
        row.flags.insert(RowFlags::MULTICELLS);
        row.blocks.push(reference);
        // One witness covers the entire horizontal span; it is not duplicated
        // for each cell, and travels with this row during later region scrolls.
        // Continuations carry inline rendition but no duplicate sparse extras.
        let mut template = source.cells[old.column];
        template.flags.remove(CellFlags::HAS_EXTRAS);
        // Sparse style metadata is attached only to the new first cell after
        // fragment copying; leaving the bit on all cells would create false extras.
        for dx in 0..old.columns {
            row.cells[self.x + dx] = template;
            if let Some(map) = self.positions.get_mut(&fragment.line) {
                // Mapping physical offsets preserves a caret inside a lower row.
                map[old.column + dx] = PositionState::Present {
                    line: row.id,
                    column: self.x + dx,
                };
            }
        }
    }
    /// Streams source text in order; lower-only reservation rows add no newline.
    fn consume(&mut self, source: &Row) {
        // Decide before modifying records: their new fragments have new IDs.
        let lower_only = source.flags.contains(RowFlags::MULTICELLS)
            && source.cells.iter().all(|cell| {
                if cell.flags.contains(CellFlags::MULTICELL) {
                    self.store.get(cell.ch).is_some_and(|block| {
                        block.fragments.first().is_some_and(|f| f.line != source.id)
                    }) || self.placed.contains(&cell.ch)
                } else {
                    cell.ch == 0 && cell.bg == crate::PackedColor::DEFAULT
                }
            });
        let length = super::occupied_length(source);
        if length == 0 {
            self.ensure(self.y);
            self.rows[self.y].version = self.rows[self.y].version.max(source.version);
        }
        // A reservation-only row contains no independent text to pack. Visiting
        // its default gaps would manufacture spaces and sometimes a false wrap.
        let mut column = 0;
        // Every dispatch consumes a positive span or one already-mapped block
        // cell; the immutable source suffix shrinks even if a block is dropped.
        while !lower_only && column < length {
            let cell = source.cells[column];
            if cell.flags.contains(CellFlags::MULTICELL) {
                if let Some(reference) = source.blocks.iter().find(|r| r.key == cell.ch) {
                    self.block(source, *reference);
                }
                column += 1;
                // The sparse key set suppresses all other cells of this rectangle.
                continue;
            }
            // Only a real lead/spacer pair can bypass the next cell's dispatch.
            // A block witness beside an orphaned lead must still go through
            // block(), which relocates its record and cell flags together.
            let wide = cell.flags.contains(CellFlags::WIDE)
                && source
                    .cells
                    .get(column + 1)
                    .is_some_and(|next| next.flags.contains(CellFlags::WIDE_SPACER));
            let span = if wide { 2 } else { 1 };
            self.scalar(source, column, span.min(length - column));
            column += span;
        }
        // Map the unused suffix to the same logical endpoint as ordinary reflow.
        let target = self.position();
        if let Some(map) = self.positions.get_mut(&source.id) {
            for position in &mut map[length..] {
                *position = target;
            }
        }
        if !source.flags.contains(RowFlags::WRAPPED) && !lower_only {
            // Hard boundaries survive; lower reservations are geometry, not lines.
            self.y += 1;
            self.x = 0;
        }
    }
    /// Resolves a sparse anchor without searching destination cell contents.
    fn locate(&self, id: LineId, column: usize) -> PositionState {
        // Offscreen/removed coordinates fail closed rather than reusing an index.
        self.positions
            .get(&id)
            .and_then(|map| map.get(column.min(map.len().saturating_sub(1))))
            .copied()
            .unwrap_or(PositionState::Gone)
    }
}

/// Rebuilds rectangular bands and then applies the existing primary-height rule.
pub(super) fn resize(
    ring: &mut RowRing,
    columns: usize,
    height: usize,
    tracked: &mut TrackedPositions,
    cursor: Option<(LineId, usize)>,
) -> Option<(usize, usize)> {
    let origin = ring.visible(0).map(|row| row.id);
    // Taking ownership prevents source reads from aliasing destination recycling.
    let mut old = std::mem::take(&mut ring.rows);
    let mut builder = Builder::new(ring, columns, height, &old);
    // pop_front strictly reduces the immutable source-row count. Destination
    // growth cannot feed rows back into this queue.
    while let Some(source) = old.pop_front() {
        builder.consume(&source);
        // No later cell read borrows this row; reuse its cell allocation for
        // future reservations instead of allocating another width-sized array.
        builder.reusable.push_back(source);
    }
    let cursor = cursor.map(|(id, col)| builder.locate(id, col));
    // Visible origin and anchors use the same map as the physical cursor.
    let origin = origin.map(|id| builder.locate(id, 0));
    tracked.remap(|id, col| builder.locate(id, col));
    ring.rows = builder.rows;
    ring.next_id = builder.next_id;
    ring.multicells = Some(builder.store);
    // Height policy then removes unused bottom rows before moving text to history.
    ring.columns = columns;
    ring.apply_reflowed_height(height, tracked, cursor, origin)
}
