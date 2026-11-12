//! Retains at most eight completed source variants before their first atlas insertion.
//!
//! Coverage facts have a separate 4,096-cluster budget. Device-size variants must
//! not accumulate indefinitely when a user zooms or when a row leaves the viewport
//! before discovery completes. Retiring ink preserves its coverage choice and lets
//! a later visible request prepare the exact variant again (P10).
use super::{ClusterGlyph, Entry, GlyphPlacement, Size};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;

/// Completed ownership follows a small FIFO, independently of pending driver jobs.
#[derive(Default)]
pub(super) struct Ready {
    /// Each entry identifies one completed variant; consumed results leave immediately.
    order: VecDeque<(u64, GlyphPlacement)>,
}

/// The source cache retains no more ink than its eight-result completion lane.
impl Ready {
    /// Tracks a newly owned result and retires the oldest unused variant under pressure.
    pub(super) fn track(
        &mut self,
        entries: &mut FxHashMap<u64, Entry>,
        hash: u64,
        p: GlyphPlacement,
    ) {
        // A delayed duplicate result replaces ownership without occupying two FIFO slots.
        // Eight entries keep this linear removal bounded and off warm atlas hits.
        self.take(hash, Size::of(&p));
        self.order.push_back((hash, p));
        // Pending variants remain in their driver's bounded mailbox. Only completed
        // buffers are retired here, and their cluster's coverage fact remains intact.
        if self.order.len() > 8
            && let Some((old, variant)) = self.order.pop_front()
            && let Some(entry) = entries.get_mut(&old)
        {
            entry.ink.remove(&Size::of(&variant));
        }
    }

    /// Atlas transfer removes the corresponding completion from retained ownership.
    pub(super) fn take(&mut self, hash: u64, size: Size) {
        // Exact device geometry avoids retiring a different zoom or hinting variant.
        self.order
            .retain(|(key, p)| *key != hash || Size::of(p) != size);
    }

    /// Drains retained ownership before another completion batch can retire its ink.
    pub(super) fn pop(
        &mut self,
        entries: &mut FxHashMap<u64, Entry>,
    ) -> Option<(GlyphPlacement, ClusterGlyph)> {
        // Retired metadata may leave no matching variant. Skip it without allocating
        // or making an atlas insertion depend on a future visible-cell lookup.
        while let Some((hash, p)) = self.order.pop_front() {
            if let Some(entry) = entries.get_mut(&hash)
                && let Some(ink) = entry.ink.remove(&Size::of(&p)).flatten()
            {
                return Some((p, ink));
            }
        }
        None
    }

    /// Coverage eviction also releases every completed variant of that candidate.
    pub(super) fn retire(&mut self, hash: u64) {
        // The parent removes the Entry, which owns its pixel/path allocations. This
        // operation removes the remaining FIFO identities without retaining stale keys.
        self.order.retain(|(key, _)| *key != hash);
    }
}
