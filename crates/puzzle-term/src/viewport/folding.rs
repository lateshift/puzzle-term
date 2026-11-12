//! Optional user row projection; VT, search and copy retain the entire source.
//! Interval merging depends on attachment count, never omitted row count. Empty
//! registrations neither allocate this cache nor enumerate source watches.
//! A fixed document stamp and one generation per provider skip clean rebuilds.
//!
//! `Viewport::top` remains a physical retained-row index. Keeping that source
//! anchor is essential while output appends and reflow assigns new identities:
//! a compressed display index alone would jump to unrelated text after a fold.
//! Wheel distances and thumb fractions, however, count only displayed rows.
//! Every source-to-display conversion therefore goes through the same map.
//!
//! Hidden anchors land on the separate blank marker row for their interval.
//! A search action first unfolds enclosing intervals, then
//! uses ordinary source navigation. Copy never consults this cache at all.
//! Publications use the inverse mapping to seal cells, cursor, overlays and
//! image clips consistently. The renderer cannot reinterpret the hidden rows.
//!
//! Generations are compared exactly rather than hashed: an accidental collision
//! must never preserve a stale omission. Saturation disables cache reuse instead
//! of wrapping. Each participating owner has retained watch capacity, bounding
//! the generation vector independently of scrollback length. Consumers prepay
//! the two interval buffers, generation scratch and allocation growth peak in
//! their own state ledger; logically unfolding does not release that charge.
use super::Viewport;
use crate::{
    osc::{Registry, SourceRevision},
    vt::Terminal,
};
use puzzle_overlay::folding::RowProjection;

/// Rebuildable geometry cache; no source witness, authority or text is owned here.
#[derive(Default)]
pub(super) struct Folds {
    /// Disjoint omissions and reusable merge scratch, bounded by live watch count.
    pub(super) rows: RowProjection,
    /// Source writes/layout invalidate retained physical indices.
    source: Option<SourceRevision>,
    /// Exact owner generations avoid hash collisions and unbounded row histories.
    versions: Vec<u64>,
    /// Owner commands can update navigation before Session publishes geometry.
    geometry_pending: bool,
}

impl Viewport {
    /// Refresh after source lifetime reconciliation, before input or publication.
    pub(crate) fn update_projection(&mut self, term: &Terminal, owners: &Registry) -> bool {
        if self.projection.is_none() && !owners.has_folds() {
            return false;
        }
        let folds = self.projection.get_or_insert_with(Default::default);
        // This cache spans all owners; owner-specific epochs live in their versions.
        let source = SourceRevision::sample(term, 0, 0);
        let versions = owners.fold_versions();
        let changed = folds.source != Some(source)
            || folds.versions.len() != versions.clone().count()
            || folds
                .versions
                .iter()
                .copied()
                .zip(versions)
                .any(|(old, next)| old != next || next == u64::MAX);
        if !changed {
            return false;
        }
        // No cell or source string is walked. Old capacities survive unfolding.
        folds.versions.clear();
        folds.versions.extend(owners.fold_versions());
        folds.source = Some(source);
        let changed = folds.rows.rebuild_marked(
            term.screen().ring.scrollback_len() + term.size().1,
            owners.fold_ranges(term),
        );
        if changed {
            // Folding is an explicit layout change, not a wheel animation target.
            folds.geometry_pending = true;
            self.motion.cancel();
            self.presentation_pending = true;
        }
        changed
    }

    /// Consume the geometry edge after all command-cascade navigation has finished.
    pub(crate) fn take_projection_change(&mut self) -> bool {
        self.projection.as_mut().is_some_and(|folds| {
            // A reveal can rebuild early to centre a search result. Its later
            // reconciliation still owes every provider the geometry invalidation.
            std::mem::take(&mut folds.geometry_pending)
        })
    }

    /// Default views still use the original physical history bound directly.
    pub(crate) fn display_history(&self, term: &Terminal) -> usize {
        self.projection.as_ref().map_or_else(
            || term.screen().ring.scrollback_len(),
            |folds| folds.rows.len().saturating_sub(term.size().1),
        )
    }

    /// One displayed row always maps to the same retained source for every adapter.
    pub(crate) fn source_row(&self, visible: usize) -> Option<usize> {
        if self.projection.is_some() {
            self.retained_row(self.display_anchor(self.top)?.checked_add(visible)?)
        } else {
            self.top.checked_add(visible)
        }
    }

    /// A user fold replaces this visible source slot with a blank marker row.
    pub(crate) fn marker_row(&self, visible: usize) -> bool {
        self.source_row(visible).is_some_and(|source| {
            self.projection
                .as_ref()
                .is_some_and(|folds| folds.rows.is_marker(source))
        })
    }

    /// Physical source order never relies on numeric LineId continuity.
    pub(super) fn retained_row(&self, display: usize) -> Option<usize> {
        self.projection
            .as_ref()
            .map_or(Some(display), |folds| folds.rows.source_row(display))
    }

    /// An anchor hidden by a newly folded ancestor lands on its visible marker.
    pub(super) fn display_anchor(&self, source: usize) -> Option<usize> {
        self.projection
            .as_ref()
            .map_or(Some(source), |folds| folds.rows.display_anchor(source))
    }

    /// Contiguous source adapters stay untouched until an actual omission exists.
    pub(crate) fn projected(&self) -> bool {
        self.projection
            .as_ref()
            .is_some_and(|folds| folds.rows.has_omissions())
    }

    /// Called only after source markers were rebuilt in physical coordinates.
    pub(crate) fn project_markers(&mut self, rows: usize) {
        if let Some(folds) = &self.projection {
            self.scrollbar.project_rows(&folds.rows, rows);
        }
    }

    /// Source overlays clip to physical extent; geometry skips omitted identities.
    pub(crate) fn source_extent(&self, rows: usize) -> usize {
        self.visible_count(rows)
            .checked_sub(1)
            .and_then(|last| self.source_row(last))
            .map_or(0, |last| last.saturating_sub(self.top) + 1)
    }
}

/// Protocol consumers precharge the shared cache before admitting any watches.
pub(crate) fn storage_bound(intervals: usize) -> usize {
    if intervals == 0 {
        return 0;
    }
    // Each participating owner has at least one witness. Exact generation
    // comparison therefore needs no more entries than the interval allowance.
    let capacity = intervals
        .max(4)
        .checked_next_power_of_two()
        .unwrap_or(usize::MAX);
    RowProjection::storage_bound(intervals)
        .saturating_add(capacity.saturating_mul(2 * size_of::<u64>()))
        // One consumer pays the whole Box; multiple consumers overcharge it safely.
        .saturating_add(size_of::<Folds>())
}
