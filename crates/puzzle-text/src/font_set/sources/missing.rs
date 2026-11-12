//! Bounded completed-negative ink results, distinct from cmap eligibility.
//!
//! A cmap can name a glyph whose outline or strike fails validation in every
//! candidate. Repeating that complete search each frame reopens files and shapes
//! again despite a warm atlas. Cache only the synchronous final Missing result;
//! errors and Pending are never retained. Exact device geometry and source identity
//! prevent a failed strike/size from suppressing a drawable variant.
//!
//! The key holds no UTF-8 allocation: FontSet already owns composed request
//! identities, while scalar identities encode their value and style directly.
//! Recency advances on lookup, including misses, so the next cold insertion can
//! identify an old decision without touching any resident font or raster page.
//! This table is metadata only; its allowance does not enlarge either atlas.
use crate::{glyph::GlyphPlacement, raster::RasterKey};
use rustc_hash::FxHashMap;

/// Geometry, foreground and effect policy cover all inputs supplied to preparation.
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
struct Key(RasterKey, u32, [u32; 2], u8);

/// A codepoint label plus recency; neither contains a font mapping or ink bytes.
#[derive(Clone, Copy)]
struct Entry {
    /// The source's completed diagnostic label survives composed request retirement.
    code: u32,
    /// Every warm lookup protects this entry against the next cold insertion.
    used: u64,
}

/// Metadata is limited to one mask page's byte allowance, with individual LRU eviction.
pub(in crate::font_set) struct Cache {
    /// Fixed-word keys allocate only on a genuinely new completed negative.
    entries: FxHashMap<Key, Entry>,
    /// Four times payload size conservatively covers bucket slack/control and growth.
    limit: usize,
    /// Recency belongs to the sole frame-building owner, requiring no synchronization.
    clock: u64,
}

/// Chooses a finite metadata allowance shared by sources of one text context.
impl Default for Cache {
    /// One existing mask page is a finite byte allowance, independent of corpus size.
    fn default() -> Self {
        Self::new((crate::raster::PAGE * crate::raster::PAGE) as usize)
    }
}

/// Refreshes warm decisions and replaces individual cold ones under its bound.
impl Cache {
    /// Derives a conservative entry ceiling from bytes rather than a glyph-count guess.
    fn new(bytes: usize) -> Self {
        Self {
            entries: FxHashMap::default(),
            limit: (bytes / (4 * std::mem::size_of::<(Key, Entry)>())).max(1),
            clock: 0,
        }
    }
    /// A hit skips complete candidate traversal and refreshes recency without allocation.
    pub(in crate::font_set) fn get(&mut self, p: &GlyphPlacement) -> Option<u32> {
        self.clock = self.clock.saturating_add(1);
        // Missing entries do not manufacture a negative or consult eligibility.
        let entry = self.entries.get_mut(&key(p))?;
        entry.used = self.clock;
        Some(entry.code)
    }
    /// Retires one least-recently-used result, preserving the rest of the warm working set.
    pub(in crate::font_set) fn insert(&mut self, p: &GlyphPlacement, code: u32) {
        // New decisions are cold; pressure cannot discard unrelated recent failures.
        if self.entries.len() >= self.limit
            && let Some(old) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key)
        {
            // Linear victim selection is cold-only; the frequent hit is one map lookup.
            self.entries.remove(&old);
        }
        // Bucket capacity is reusable after eviction; completed facts own no strings.
        self.entries.insert(
            key(p),
            Entry {
                code,
                used: self.clock,
            },
        );
    }
    /// Source replacement or family/metrics changes invalidate all completed decisions.
    pub(in crate::font_set) fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Origins do not alter source ink; baseline, fitting span, device size do; size_px already includes DPR.
fn key(p: &GlyphPlacement) -> Key {
    // Scalar IDs reversibly encode scalar/style/presentation; composed IDs are
    // monotonic complete-cluster identities and cannot alias retired requests.
    Key(
        RasterKey {
            glyph: p.key,
            size: p.size_px.to_bits(),
            fit: [p.ink_limit_px, p.cell_height_px, p.baseline_px.to_bits()],
            // Device size and fit are already physical: no independent DPR input exists.
            dpr: 0,
        },
        p.color,
        [p.advance_px, u32::from(p.cell_span)],
        u8::from(p.effect) | (u8::from(p.color_glyph) << 1),
    )
}

#[cfg(test)]
/// Tiny byte bounds exercise eviction and retention without system fonts.
mod tests;

/// Retains only completed negative answers; transient errors and provisional work retry.
pub(super) fn prepare(
    source: &mut dyn crate::cluster_source::synchronous::SynchronousSources,
    cache: &mut Cache,
    request: crate::cluster_source::ClusterRequest<'_>,
    p: &GlyphPlacement,
) -> Result<crate::cluster_source::ClusterGlyph, String> {
    use crate::cluster_source::{ClusterGlyph, EMOJI_FACE, PLATFORM_FACE, synchronous::SourceKind};
    let kind = match p.key.face {
        EMOJI_FACE => SourceKind::Emoji,
        PLATFORM_FACE => SourceKind::Platform,
        _ => return Err("placement does not name a cluster source".into()),
    };
    let code = request.cluster.chars().next().map_or(0, u32::from);
    // The source finishes every candidate before None becomes a final Missing result.
    let ink = match source.prepare(request, p, kind)? {
        Some(ClusterGlyph::Pending(_)) => {
            return Err("complete source returned provisional ink".into());
        }
        Some(ink) => ink,
        None => ClusterGlyph::Missing(code),
    };
    // Neither a transport/source error nor a Pending result establishes final coverage.
    // Retaining only Missing keeps a transient failure eligible for its next retry.
    if let ClusterGlyph::Missing(code) = ink {
        cache.insert(p, code);
    }
    Ok(ink)
}
