//! Row-major CPU texel plane shared by curve and band packers (§9.8).
//!
//! The GPU will receive dirty row spans in M3. Keeping mirrors here makes
//! device recovery a re-upload rather than a shape regeneration. Capacity
//! doubles by rows, starting at 64, while the atlas enforces its byte budget.
//!
//! The shader treats x as a coordinate within one 4096-texel row. Some reads
//! can wrap through `CalcBandLoc`, but curve endpoints, headers and a single
//! band list cannot. The packers call `align_row` only at those boundaries;
//! this store itself retains a simple linear cursor.
//!
//! Dirty spans are row-local because a future `queue.write_texture` needs a
//! valid row origin and byte count. Adjacent writes merge, keeping uploads
//! proportional to changed regions rather than to individual texels.
use super::{AtlasError, TEXTURE_WIDTH};

/// A contiguous modified range on one texture row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtySpan {
    /// Zero-based texture row.
    pub row: u16,
    /// Inclusive first modified column.
    pub start: u16,
    /// Exclusive last modified column.
    pub end: u16,
}

/// Owned mirror and next free linear texel position.
pub(super) struct TexelStore<T: Copy + Default> {
    /// Allocated rows, including unused capacity for later glyphs.
    pub(super) texels: Vec<T>,
    /// Next free linear texel; packers may skip to a new row.
    pub(super) cursor: usize,
    /// Exclusive end of the active page; writes may never cross that range.
    pub(super) limit: usize,
    /// Highest occupied texel across all pages, independent of the active cursor.
    pub(super) high_water: usize,
    /// Changes since the last upload request.
    pub(super) dirty: Vec<DirtySpan>,
}

/// Allocation and upload bookkeeping independent of pixel format.
impl<T: Copy + Default> TexelStore<T> {
    /// Starts both mirrors at the reference's 64-row minimum.
    pub(super) fn new() -> Self {
        Self {
            texels: vec![T::default(); TEXTURE_WIDTH * 64],
            cursor: 0,
            limit: TEXTURE_WIDTH * 64,
            high_water: 0,
            dirty: Vec::new(),
        }
    }

    /// Reports actual allocated bytes for the shared memory budget.
    pub(super) fn bytes(&self) -> usize {
        self.texels.len() * std::mem::size_of::<T>()
    }

    /// Grows in whole rows, doubling height without invalidating addresses.
    pub(super) fn ensure(
        &mut self,
        needed: usize,
        other_bytes: usize,
        budget: usize,
    ) -> Result<(), AtlasError> {
        // Page selection, not an individual curve or band, owns texture growth.
        // Crossing this limit rolls back the whole shape and tries another page.
        if needed > self.limit {
            return Err(AtlasError::Capacity);
        }
        self.grow(needed, other_bytes, budget)
    }

    /// Reserves texture rows at an address-safe allocation boundary.
    pub(super) fn grow(
        &mut self,
        needed: usize,
        other_bytes: usize,
        budget: usize,
    ) -> Result<(), AtlasError> {
        // Existing capacity is already charged to the atlas budget, so an
        // ordinary glyph insertion needs neither allocation nor resizing.
        if needed <= self.texels.len() {
            return Ok(());
        }
        let mut rows = self.texels.len() / TEXTURE_WIDTH;
        while rows * TEXTURE_WIDTH < needed {
            rows = rows.checked_mul(2).ok_or(AtlasError::Capacity)?;
        }
        // Locations encode y in sixteen bits, and the two mirrors share
        // one byte budget rather than each claiming the whole allowance.
        // A texture's height is encoded in a 16-bit row coordinate. Check
        // both addressability and combined memory before extending the Vec.
        let bytes = rows
            .checked_mul(TEXTURE_WIDTH)
            .and_then(|n| n.checked_mul(std::mem::size_of::<T>()))
            .and_then(|n| n.checked_add(other_bytes))
            .ok_or(AtlasError::Capacity)?;
        if rows > u16::MAX as usize || bytes > budget {
            return Err(AtlasError::Capacity);
        }
        self.texels.resize(rows * TEXTURE_WIDTH, T::default());
        Ok(())
    }

    /// Records one texel; adjacent edits are coalesced into a row span.
    pub(super) fn mark(&mut self, index: usize) {
        // Selecting a reused low page must not hide ink in a higher live page.
        self.high_water = self.high_water.max(index + 1);
        let row = (index / TEXTURE_WIDTH) as u16;
        let column = (index % TEXTURE_WIDTH) as u16;
        // Band lists are written before their headers, so a later header
        // can extend the last span backward as well as forward. Both ends
        // must change or the shader sees an unwritten zero-count header.
        if let Some(last) = self.dirty.last_mut()
            && last.row == row
            && column <= last.end
        {
            last.start = last.start.min(column);
            last.end = last.end.max(column + 1);
            return;
        }
        self.dirty.push(DirtySpan {
            row,
            start: column,
            end: column + 1,
        });
    }

    /// Advances to a row boundary when a shader read cannot wrap.
    pub(super) fn align_row(&mut self) {
        let remainder = self.cursor % TEXTURE_WIDTH;
        if remainder != 0 {
            self.cursor += TEXTURE_WIDTH - remainder;
        }
    }

    /// Marks occupied rows for restoration without changing addresses or geometry.
    pub(super) fn restore(&mut self) {
        self.dirty.clear();
        for start in (0..self.high_water).step_by(TEXTURE_WIDTH) {
            self.dirty.push(DirtySpan {
                row: (start / TEXTURE_WIDTH) as u16,
                start: 0,
                end: (self.high_water - start).min(TEXTURE_WIDTH) as u16,
            });
        }
    }

    /// Combines touching intervals after all transactional writes have succeeded.
    /// Headers are written after lists and pages can be revisited out of order,
    /// so merging only the last appended span cannot identify every adjacency.
    pub(super) fn coalesce_dirty(&mut self) {
        self.dirty
            .sort_unstable_by_key(|span| (span.row, span.start));
        let mut written = 0;
        for read in 0..self.dirty.len() {
            let span = self.dirty[read];
            // Sort order makes the previous union the only possible overlap.
            if written > 0
                && self.dirty[written - 1].row == span.row
                && span.start <= self.dirty[written - 1].end
            {
                self.dirty[written - 1].end = self.dirty[written - 1].end.max(span.end);
            } else {
                self.dirty[written] = span;
                written += 1;
            }
        }
        // Reuse the allocation; no texel bytes are copied or widened across gaps.
        self.dirty.truncate(written);
    }

    /// Invalidates logical contents while reusing allocated rows.
    pub(super) fn reset(&mut self) {
        self.cursor = 0;
        self.high_water = 0;
        self.dirty.clear();
    }
}

#[cfg(test)]
/// Publication must preserve exact dirty coverage and eliminate touching writes.
mod tests;
