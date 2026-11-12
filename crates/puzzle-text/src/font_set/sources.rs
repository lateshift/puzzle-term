//! Ownership and identities for platform-supplied cluster ink (§8.3).
//!
//! A placement stores a compact key rather than borrowing a UTF-8 string.
//! Complete requests are retained once per cluster/style, while lookup uses
//! a hash candidate verified against the original bytes to avoid collisions.
//!
//! Source identities never use the first-character choice cache as a glyph
//! cache. For example two ZWJ families start with the same man character but
//! need different bitmaps. First-character lookup chooses only a source;
//! this registry retains the complete request behind each glyph key.
//!
//! All mutations occur on the host's frame-building thread. Send on the
//! traits allows the core to move threads during construction, without any
//! internal mutex or dependence on a browser/native synchronization API.
//! Sources are boxed at this cold seam, outside the per-byte parser path.
//!
//! At most 4096 composed requests of at most 32 UTF-8 bytes are retained. Cached hits
//! compare borrowed strings and reuse an ID without allocating another copy.
//! Capacity pressure retires one request; monotonic IDs never alias atlas ink.
//! Atlas memory and bitmap lifetime remain downstream of this string store.
//! Single scalars use reversible IDs instead of that evictable string registry.
//! Their prepared ink remains addressable across complete Unicode flood cycles.
//! Style and presentation are encoded independently of the scalar's cell width.
//! Tagged scalar IDs never overlap the monotonic composed-cluster namespace.
use super::{FontSet, FontStyle};
use crate::{
    cluster_source::{
        ClusterGlyph, ClusterRequest, EMOJI_FACE, EmojiSource, PLATFORM_FACE, PlatformFallback,
    },
    glyph::{GlyphKey, GlyphPlacement, HintMode},
    presentation::Presentation,
};
use rustc_hash::{FxHashMap, FxHasher};
use std::hash::{Hash, Hasher};

/// Browser continuation/publication policy is independent of complete native sources.
mod continuation;
/// Final native validation negatives retain exact device geometry, bounded by bytes.
pub(super) mod missing;
/// Reversible scalar keys leave composed-cluster metadata within its existing bound.
mod scalar;

/// Retained request for one non-font placement identity.
struct Request {
    /// Original selector/joiner sequence passed unchanged to the environment.
    cluster: String,
    /// Weight/slant choice passed independently of any family name.
    style: FontStyle,
    /// Presentation separates text and emoji uses of the same base.
    presentation: Presentation,
    /// The verified hash bucket is removed when this request retires.
    hash: u64,
}

/// Host-supplied implementations and bounded source string storage.
#[derive(Default)]
pub(super) struct Sources {
    /// Last applied families/metrics distinguish real policy changes from repeated setup.
    pub(super) synchronous_policy: Option<(Vec<String>, crate::metrics::CellMetrics)>,
    /// A cmap-positive but undrawable candidate must not reopen fonts on every warm frame.
    pub(super) missing: missing::Cache,
    /// Alternating policies prevent a continuously ready source from starving the other.
    next_platform: bool,
    /// Optional step-four emoji implementation.
    pub(super) emoji: Option<Box<dyn EmojiSource>>,
    /// Optional step-five system font implementation.
    pub(super) platform: Option<Box<dyn PlatformFallback>>,
    /// Desktop complete policies share one lock-free font owner.
    pub(super) synchronous: Option<Box<dyn crate::cluster_source::synchronous::SynchronousSources>>,
    /// Monotonic IDs remain unambiguous when old request metadata is retired.
    requests: FxHashMap<u32, Request>,
    /// Retirement removes one oldest identity while preserving every other source key.
    order: std::collections::VecDeque<u32>,
    /// IDs never alias old atlas descriptors, even after individual request eviction.
    next_id: u32,
    /// Hash candidates retain collision buckets, not unchecked identities.
    indices: FxHashMap<u64, Vec<u32>>,
    /// Complete cluster identities versus reversible scalar keys.
    metrics: Option<crate::cache_metrics::CacheMetrics>,
    /// Eligibility positives/negatives; native uses immutable scalar coverage tables.
    coverage: Option<crate::cache_metrics::CacheMetrics>,
}

impl Sources {
    /// Reserves a whole-grapheme emoji choice, possibly awaiting authoritative ink.
    /// Native first-draw validation does not claim coverage from cmap eligibility.
    /// A provisional choice gets its own identity; Pending remains diagnostic tofu
    /// until the owned complete-cluster preparation reaches the ordinary atlas.
    pub(super) fn emoji_covers(&mut self, request: ClusterRequest<'_>) -> bool {
        // Complete metadata checks can wait once for the host index; no ink is discarded.
        if let Some(source) = self.synchronous.as_mut() {
            let covered = source.eligible(
                request,
                crate::cluster_source::synchronous::SourceKind::Emoji,
            );
            // Native eligibility consults immutable scalar tables, not a string cache.
            crate::cache_metrics::CacheMetrics::lookup(&mut self.coverage, covered, 0);
            return covered;
        }
        self.emoji
            .as_mut()
            .is_some_and(|source| source.may_prepare(request))
    }

    /// Checks a complete platform choice or reserves its first validation job.
    /// Confirmed negatives fall through to later sources; unknown choices get
    /// actual placement metrics before any expensive preparation is requested.
    pub(super) fn platform_covers(&mut self, request: ClusterRequest<'_>) -> bool {
        // Complete metadata checks can wait once for the host index; no ink is discarded.
        if let Some(source) = self.synchronous.as_mut() {
            let covered = source.eligible(
                request,
                crate::cluster_source::synchronous::SourceKind::Platform,
            );
            crate::cache_metrics::CacheMetrics::lookup(&mut self.coverage, covered, 0);
            return covered;
        }
        self.platform
            .as_mut()
            .is_some_and(|source| source.may_prepare(request))
    }

    /// Tries platform fonts, then the emoji source for text presentation.
    pub(super) fn last_resort(&mut self, request: ClusterRequest<'_>) -> Option<u32> {
        // VS15 retains emoji as the final source, after ordinary system text.
        if self.platform_covers(request) {
            Some(PLATFORM_FACE)
        } else if request.presentation == Presentation::Text && self.emoji_covers(request) {
            Some(EMOJI_FACE)
        } else {
            None
        }
    }

    /// Assigns a stable source request ID without allocating on a cache hit.
    fn key(&mut self, request: ClusterRequest<'_>, face: u32) -> Result<GlyphKey, String> {
        // Input strings originate in bounded terminal clusters. Enforce that
        // bound here too for callers using the text layer without a Terminal.
        if request.cluster.len() > 32 {
            return Err("source cluster exceeds 32-byte bound".into());
        }
        if let Some(key) = scalar::key(request, face) {
            if let Some(metrics) = &mut self.metrics {
                // Scalar keys encode Unicode/style directly; no text bytes are hashed.
                metrics.scalar_keys += 1;
            }
            return Ok(key);
        }
        // Hash all appearance selectors; equal bytes in another style still
        // need a separate glyph preparation, even with the same source face.
        let mut hash = FxHasher::default();
        (request.cluster, request.style, request.presentation).hash(&mut hash);
        let hash = hash.finish();
        // A hash match is a candidate, not proof that the strings are equal.
        let old = self.indices.get(&hash).and_then(|ids| {
            ids.iter().copied().find(|id| {
                let old = &self.requests[id];
                old.cluster == request.cluster
                    && old.style == request.style
                    && old.presentation == request.presentation
            })
        });
        // Reuse a verified identity; only truly new composed requests consume metadata.
        // Composed identities hash bounded bytes, then validate exact request equality.
        crate::cache_metrics::CacheMetrics::lookup(
            &mut self.metrics,
            old.is_some(),
            request.cluster.len(),
        );
        let id = match old {
            Some(id) => id,
            None => self.register(request, hash)?,
        };
        // Hinting lives in the source's device-size request, so this neutral
        // identity uses an unhinted suffix rather than a primary-font ppem.
        Ok(GlyphKey {
            face,
            glyph_id: id,
            instance: request.style as u32,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        })
    }
    /// Admits a new request without growing the 4,096-entry metadata allowance.
    fn register(&mut self, request: ClusterRequest<'_>, hash: u64) -> Result<u32, String> {
        // Retire metadata and its collision bucket together, keeping the bound exact.
        if self.requests.len() >= 4096
            && let Some(old) = self.order.pop_front()
            && let Some(retired) = self.requests.remove(&old)
        {
            if let Some(metrics) = &mut self.metrics {
                metrics.evictions += 1;
            }
            // A colliding request can survive the retirement of its neighbor.
            if let Some(ids) = self.indices.get_mut(&retired.hash) {
                ids.retain(|id| *id != old);
            }
            // Empty buckets would otherwise grow with every new Unicode scalar.
            if self.indices.get(&retired.hash).is_some_and(Vec::is_empty) {
                self.indices.remove(&retired.hash);
            }
        }
        // Monotonic identities leave stale atlas entries unambiguous through eviction.
        let id = self.next_id;
        if id >= scalar::TAG {
            return Err("source identity exhausted".into());
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("source identity exhausted")?;
        // The retained string belongs to this ID until individual eviction.
        self.requests.insert(
            id,
            Request {
                cluster: request.cluster.into(),
                style: request.style,
                presentation: request.presentation,
                hash,
            },
        );
        // FIFO retirement is independent of frame/atlas key lifetime.
        // A row already containing prepared ink keeps its downstream descriptor.
        // A later cold layout gets a fresh ID, so delayed work cannot replace
        // a different cluster after the string allowance wraps.
        self.order.push_back(id);
        self.indices.entry(hash).or_default().push(id);
        Ok(id)
    }
}

impl FontSet {
    /// Installs the host's emoji source before any row is laid out.
    pub fn set_emoji_source(&mut self, source: Box<dyn EmojiSource>) {
        // Changing the source may replace a previous emoji-first text face.
        self.selection_cache
            .retain(|(_, _, presentation), _| *presentation != Presentation::Emoji);
        self.sources.emoji = Some(source);
    }

    /// Installs the platform implementation; no platform APIs enter this crate.
    /// Loaded text faces remain ahead of this source, so their cached decisions
    /// stay valid when a host initially supplies its environment fallback.
    pub fn set_platform_fallback(&mut self, source: Box<dyn PlatformFallback>) {
        self.sources.platform = Some(source);
    }

    /// Retains a complete source request behind the neutral placement key.
    pub fn source_key(
        &mut self,
        cluster: &str,
        style: FontStyle,
        face: u32,
    ) -> Result<GlyphKey, String> {
        self.sources.key(
            ClusterRequest {
                cluster,
                style,
                presentation: crate::presentation::presentation(cluster),
            },
            face,
        )
    }

    /// Produces source ink through the existing vector/bitmap seam.
    /// A returned bitmap owns its pixels until the atlas copies them. FontSet
    /// stores neither GPU texture addresses nor platform rendering contexts.
    pub fn prepare_source(&mut self, placement: &GlyphPlacement) -> Result<ClusterGlyph, String> {
        // Only complete native results are cached; browser eligibility remains asynchronous.
        if self.sources.synchronous.is_some()
            && let Some(code) = self.sources.missing.get(placement)
        {
            return Ok(ClusterGlyph::Missing(code));
        }
        if let Some(metrics) = &mut self.sources.metrics {
            // Count invocation before lookup: Missing and contourless results are work too.
            metrics.preparations += 1;
        }
        let mut bytes = [0; 4];
        let borrowed = if placement.key.glyph_id & scalar::TAG != 0 {
            // Scalar identities remain valid after every composed-cluster eviction.
            scalar::request(placement.key.glyph_id, &mut bytes)?
        } else {
            let request = self
                .sources
                .requests
                .get(&placement.key.glyph_id)
                .ok_or("invalid source cluster identity")?;
            // Composed requests retain their exact selectors/joiners in the bounded registry.
            ClusterRequest {
                cluster: &request.cluster,
                style: request.style,
                presentation: request.presentation,
            }
        };
        // Complete native policies have no job lane or provisional result to poll.
        if let Some(source) = self.sources.synchronous.as_mut() {
            return missing::prepare(
                source.as_mut(),
                &mut self.sources.missing,
                borrowed,
                placement,
            );
        }
        // The two reserved face IDs select ownership, never an OpenType face.
        match placement.key.face {
            EMOJI_FACE => self
                .sources
                .emoji
                .as_mut()
                .ok_or("emoji source absent")?
                .prepare(borrowed, placement),
            PLATFORM_FACE => self
                .sources
                .platform
                .as_mut()
                .ok_or("platform fallback absent")?
                .prepare(borrowed, placement),
            _ => Err("placement does not name a cluster source".into()),
        }
    }
}

impl FontSet {
    /// Enables fixed counters without changing fallback or string ownership.
    pub fn measure_caches(&mut self) {
        // Frame-level activation retains cumulative counts for chronological differences.
        self.metrics.get_or_insert_with(Default::default);
        self.sources.metrics.get_or_insert_with(Default::default);
        // A positive coverage query is eligibility, distinct from a prepared atlas hit.
        self.sources.coverage.get_or_insert_with(Default::default);
    }
    /// Selection, coverage queries and source identities, in that report order.
    pub fn cache_metrics(&self) -> [crate::cache_metrics::CacheMetrics; 3] {
        // Fixed scalars cross the seam without cloning strings or prepared geometry.
        [
            self.metrics.unwrap_or_default(),
            self.sources.coverage.unwrap_or_default(),
            self.sources.metrics.unwrap_or_default(),
        ]
    }
}
