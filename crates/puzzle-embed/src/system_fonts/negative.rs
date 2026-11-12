//! Completed candidate failures skip mapping, shaping and duplicate diagnostics.
//! Keys retain exact cluster text, font identity and all preparation geometry.
//! Metadata is bounded by one existing mask page; pressure evicts individual LRU
//! entries, never the working set wholesale. Only source-policy changes clear it.
//! A cmap hit promises a glyph ID, not visible ink. Some installed faces map
//! ideographic spaces to empty outlines, so trying them repeatedly adds both
//! outline work and warning output to every changing Unicode frame.
//! This cache stores failed *candidates*: success in a later fallback family
//! cannot erase an earlier family's failure. Candidate IDs refer to the stable
//! font index, while exact UTF-8 distinguishes clusters without hash collisions.
//! Device geometry is part of the key because a bitmap strike or fitted outline
//! can fail at one size and succeed at another. Colors and origins are excluded.
//! All access stays on the render-owned native font source; no lock is needed.
use puzzle_term::text::{cluster_source::ClusterRequest, glyph::GlyphPlacement};
use std::collections::HashMap;

/// One rejected font/geometry combination with its access recency.
struct Entry {
    /// Index face identity plus appearance and fitted-device geometry.
    key: [u32; 12],
    /// Monotonic access order on the sole font owner.
    used: u64,
}
/// Exact UTF-8 is borrowed on warm lookups, so repeats do not allocate.
#[derive(Default)]
pub(super) struct Cache {
    /// Only cold negative clusters allocate their owned spelling.
    clusters: HashMap<String, Vec<Entry>>,
    /// Conservative retained byte charge including map and vector slack.
    bytes: usize,
    /// Single-owner LRU clock, independent of wall time.
    clock: u64,
}
/// Completed preparation facts belong to this source, independent of atlas residency.
impl Cache {
    /// Checks a complete failure before opening the candidate's font file.
    pub(super) fn contains(
        &mut self,
        r: ClusterRequest<'_>,
        p: &GlyphPlacement,
        face: u32,
        emoji: bool,
    ) -> bool {
        // Increment on every query so both successful negative hits and cold misses
        // participate in the same recency order, without a timer or expiry policy.
        self.clock = self.clock.saturating_add(1);
        let key = key(r, p, face, emoji);
        // Borrow cluster text from the request; hot lookups allocate no String.
        // Multiple font/size failures for one cluster share that spelling.
        let Some(entry) = self
            .clusters
            .get_mut(r.cluster)
            .and_then(|v| v.iter_mut().find(|e| e.key == key))
        else {
            return false;
        };
        entry.used = self.clock;
        true
    }
    /// Records only completed failures, with the same metadata allowance as core negatives.
    pub(super) fn insert(
        &mut self,
        r: ClusterRequest<'_>,
        p: &GlyphPlacement,
        face: u32,
        emoji: bool,
    ) {
        // Four times payload bounds bucket/vector slack without enlarging either atlas.
        let charge =
            4 * (std::mem::size_of::<Entry>() + r.cluster.len() + std::mem::size_of::<String>());
        let limit = (puzzle_term::text::raster::PAGE * puzzle_term::text::raster::PAGE) as usize;
        // Capacity retires one oldest decision at a time. Clearing all failures
        // would restart expensive font traversal for an entire active corpus.
        while self.bytes + charge > limit && self.evict() {}
        // Only the cold insertion takes ownership; glyph pixels and mapped font
        // bytes never enter this metadata cache or inflate its byte allowance.
        self.clusters
            .entry(r.cluster.to_owned())
            .or_default()
            .push(Entry {
                key: key(r, p, face, emoji),
                used: self.clock,
            });
        self.bytes += charge;
    }
    /// Removes one old decision; recent failures stay warm across unrelated misses.
    fn evict(&mut self) -> bool {
        // Eviction is a cold pressure path, so scanning recency avoids a second
        // mutable index and leaves ordinary warm lookups as a borrowed map access.
        let victim = self
            .clusters
            .iter()
            .flat_map(|(cluster, entries)| {
                entries
                    .iter()
                    .enumerate()
                    .map(move |(i, e)| (cluster, i, e.used))
            })
            .min_by_key(|(_, _, used)| *used)
            .map(|(c, i, _)| (c.clone(), i));
        let Some((cluster, index)) = victim else {
            return false;
        };
        if let Some(entries) = self.clusters.get_mut(&cluster) {
            // Order inside one cluster is irrelevant; only the recency stamp matters.
            // Charge the removed fact, including its conservative shared-key allowance.
            entries.swap_remove(index);
            self.bytes -=
                4 * (std::mem::size_of::<Entry>() + cluster.len() + std::mem::size_of::<String>());
            // Remove the spelling only after its final font/geometry fact leaves.
            if entries.is_empty() {
                self.clusters.remove(&cluster);
            }
        }
        true
    }
    /// Only changed family/font policies invalidate complete preparation facts.
    pub(super) fn clear(&mut self) {
        // A family policy change can alter synthesis/selection behind stable IDs.
        // Geometry changes alone keep these facts, separated by the key below.
        self.clusters.clear();
        self.bytes = 0;
    }
}
/// Origins/colors do not change whether ink exists; hinting, fit and appearance do.
fn key(r: ClusterRequest<'_>, p: &GlyphPlacement, face: u32, emoji: bool) -> [u32; 12] {
    [
        face,
        r.style as u32,
        r.presentation as u32,
        p.size_px.to_bits(),
        p.ink_limit_px,
        p.advance_px,
        u32::from(p.cell_span),
        p.cell_height_px,
        p.baseline_px.to_bits(),
        u32::from(p.key.hint_ppem),
        u32::from(p.key.hint_mode as u8),
        u32::from(emoji),
    ]
}

#[cfg(test)]
/// Exact identities, geometry, cache recency and warm allocation contract.
mod tests;
