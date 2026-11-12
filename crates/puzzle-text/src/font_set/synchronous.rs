//! Installation and policy changes for the complete host source (D-41).
//! The applied policy is retained by value so repeated frame/context setup does
//! not flush negative validation facts. Only a different family order or metrics
//! asks the source to configure; only family changes invalidate dependent ink.
use super::FontSet;
use crate::{cluster_source::synchronous::SynchronousSources, metrics::CellMetrics};

impl FontSet {
    /// Replaces asynchronous policies with one directly owned complete source.
    pub fn set_synchronous_sources(&mut self, source: Box<dyn SynchronousSources>) {
        // No producer can prepare a duplicate glyph after the synchronous takeover.
        self.preparation = None;
        // The asynchronous policies relinquish their job/result channel ownership.
        self.sources.emoji = None;
        self.sources.platform = None;
        // Both native presentation policies now borrow the same mapped-face owner.
        self.sources.synchronous = Some(source);
        // A replacement catalog may draw ink rejected by the previous source.
        self.sources.missing.clear();
        self.sources.synchronous_policy = None;
        self.selection_cache.clear();
    }
    /// Configures geometry and returns whether family selection invalidates source ink.
    pub fn configure_synchronous_sources(
        &mut self,
        families: &[String],
        metrics: CellMetrics,
    ) -> bool {
        // Reapplying unchanged settings must preserve the complete negative working set.
        if self
            .sources
            .synchronous_policy
            .as_ref()
            .is_some_and(|(old, cell)| old == families && *cell == metrics)
        {
            return false;
        }
        // Families identify the catalog; metrics identify only a sized ink variant.
        // Comparing them separately keeps reusable outlines and old sized facts alive.
        let changed = self
            .sources
            .synchronous_policy
            .as_ref()
            .is_none_or(|(old, _)| old != families);
        // The host owns one source. Configuring it is cold geometry/policy work,
        // while its retained keys distinguish every size used before and after a move.
        if let Some(source) = self.sources.synchronous.as_mut() {
            source.configure(families, metrics);
            // Save both values so identical subsequent settings become a complete no-op.
            self.sources.synchronous_policy = Some((families.to_vec(), metrics));
            // Geometry is already part of each negative and bitmap cache key.
            // Only a different family policy can change an existing source identity.
            if changed {
                self.sources.missing.clear();
                self.selection_cache.clear();
            }
        }
        changed
    }
}
