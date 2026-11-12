//! Nonblocking cluster discovery, shared by desktop workers and browser idle tasks.
//!
//! A cold choice queues a bounded request and immediately declines coverage.
//! Completion invalidates provisional rows through the text-context generation.
//! Sized ink follows the same rule: Pending draws tofu using a separate key.
//! The driver owns scheduling and I/O; no thread, lock or JS object enters core.
//! Warm lookups hash borrowed strings and verify their full contents (P1).
//!
//! Coverage and prepared ink have different lifetimes. Unicode benchmarks cycle
//! through far more scalars than fit in the temporary request registry. Completed
//! scalar answers therefore live in sparse two-bit pages for this source's lifetime;
//! only composed strings compete for the registry's 4,096 slots.
//! Temporary entries also own pending device-size variants. Retirement must not
//! discard a later scalar completion: its full producer key can reconstruct the
//! entry for atlas insertion. Ready ownership still has its independent eight-item
//! bound, so persistent coverage never retains a font path or bitmap allocation.
//! Replacing a source drops its pages with the old installed-font snapshot; these
//! facts are never shared across different font settings or platform policies.
use crate::{
    cluster_source::*, font_set::FontStyle, glyph::GlyphPlacement, presentation::Presentation,
};
use rustc_hash::{FxHashMap, FxHasher};
use std::{
    collections::VecDeque,
    hash::{Hash, Hasher},
};

/// Complete discovery identity; hashes are never proof of equality.
#[derive(Clone, PartialEq, Eq)]
pub struct Key {
    /// Bounded whole grapheme, including selectors and joiners.
    pub cluster: String,
    /// Weight/slant independent of primary font identity.
    pub style: FontStyle,
    /// Explicit text/emoji policy follows D-31.
    pub presentation: Presentation,
}
/// One queued coverage probe or device-sized ink request.
pub struct Job {
    /// Full cluster identity retained across the asynchronous boundary.
    pub key: Key,
    /// None probes coverage; Some requests samples/outlines at these exact metrics.
    pub placement: Option<GlyphPlacement>,
    /// Prior authoritative ink already validated this complete cluster and source policy.
    pub validated: bool,
}
/// A completed request owns its geometry, never platform font-table borrows.
pub struct Completion {
    /// Original request prevents a delayed result being attributed to a new key.
    pub job: Job,
    /// Coverage probes return a boolean; sized jobs return their drawable ink.
    pub result: Result<Option<ClusterGlyph>, String>,
    /// Coverage is separate from a failed device-size preparation.
    pub covered: bool,
}
/// Cold scheduling seam. Every method called by the frame path must be nonblocking.
pub trait Driver: Send {
    /// Starts a host-admitted frame allowance; zero preserves background-only callers.
    fn begin_frame(&mut self, _allowance: std::time::Duration) {}
    /// Actual synchronous source work is excluded from ordinary-frame cost prediction.
    fn frame_preparation_time(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
    /// Attempts one cold placement without waiting for another owner or index construction.
    /// None defers unchanged to the worker; successful owned ink is used by this draw.
    fn prepare_now(
        &mut self,
        _request: ClusterRequest<'_>,
        _placement: &GlyphPlacement,
        _validated: bool,
    ) -> Option<Result<Option<ClusterGlyph>, String>> {
        None
    }
    /// True validates and transfers ink in one actual-placement job, rather than
    /// doing an earlier preparation solely to publish a boolean coverage fact.
    fn validates_during_prepare(&self) -> bool {
        false
    }
    /// Enqueues work without waiting; false means the bounded queue is full.
    fn submit(&mut self, job: Job) -> bool;
    /// Takes one already completed result, never waits for its producer.
    fn poll(&mut self) -> Option<Completion>;
    /// Reports outstanding accepted work; this query must not wait for its producer.
    fn pending(&self) -> bool;
    /// Peeks only already available work; a host may request its next frame without polling.
    fn ready(&mut self) -> bool {
        false
    }
    /// An immutable coverage index can reject missing scalars without queueing work.
    fn definitely_missing(&self, _cluster: &str) -> bool {
        false
    }
}
/// Coverage and preparation use different keys: transforms change sample size.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Size([u32; 6]);
/// Keeps only facts and ink awaiting its first atlas insertion.
struct Entry {
    /// Full identity verifies the borrowed hash candidate.
    key: Key,
    /// None means a queued coverage probe; false is a completed negative result.
    covered: Option<bool>,
    /// A queued variant owns no ink; completed ink is moved into the atlas once.
    ink: FxHashMap<Size, Option<ClusterGlyph>>,
}
/// Independent source policy with bounded metadata, never wholesale cache clearing.
pub struct Deferred {
    /// Platform scheduling owner contains only Send-compatible platform state.
    driver: Box<dyn Driver>,
    /// Hash candidates are verified against full UTF-8 and appearance facts.
    entries: FxHashMap<u64, Entry>,
    /// Individual oldest entries leave under pressure; other choices survive.
    order: VecDeque<u64>,
    /// At most eight completed variants await atlas insertion, independent of zoom count.
    ready: ready::Ready,
    /// Completed scalar decisions survive repeated payloads without retaining strings.
    facts: facts::Facts,
}
/// Time-admitted first draws share facts and ownership with deferred completions.
mod admission;
/// Completion publication restores scalar ownership after metadata pressure.
mod completion;
/// Sparse permanent facts keep Unicode cycles from restarting completed discovery.
mod facts;
/// Owned completion retention has a separate budget from coverage metadata.
mod ready;
/// The frame owns facts; the driver owns all expensive work and completion scheduling.
impl Deferred {
    /// Creates a source with no discovery on the calling frame thread.
    pub fn new(driver: Box<dyn Driver>) -> Self {
        // The platform has already chosen its worker/task owner. Construction does not
        // invoke it, so installing fallback cannot delay first-frame presentation.
        Self {
            driver,
            entries: FxHashMap::default(),
            order: VecDeque::new(),
            ready: ready::Ready::default(),
            facts: facts::Facts::default(),
        }
    }
    /// Returns a borrowed-key hash, without allocating on warm rows.
    fn hash(r: ClusterRequest<'_>) -> u64 {
        // Style and presentation remain independent of the requested device size.
        // Size variants share coverage while retaining different owned sample buffers.
        let mut h = FxHasher::default();
        (r.cluster, r.style, r.presentation).hash(&mut h);
        h.finish()
    }
    /// Queues a cold choice while unresolved text continues to use builtin tofu.
    fn covers(&mut self, r: ClusterRequest<'_>) -> bool {
        // An index rejection is permanent for this installed-font snapshot and bypasses
        // the bounded cluster cache, including after a complete Unicode flood.
        if r.cluster.len() > 32 || self.driver.definitely_missing(r.cluster) {
            return false;
        }
        if let Some(covered) = self.facts.get(r) {
            // A completed scalar remains drawable even if its temporary UTF-8 entry left.
            // The downstream atlas can then reuse the scalar's stable source identity.
            return covered;
        }
        if self.driver.validates_during_prepare() {
            // Unknown is never reported as confirmed coverage. Layout can reserve a
            // provisional identity through may_prepare, then request its actual ink.
            return self.entries.get(&Self::hash(r)).is_some_and(|entry| {
                entry.key.cluster == r.cluster
                    && entry.key.style == r.style
                    && entry.key.presentation == r.presentation
                    && entry.covered == Some(true)
            });
        }
        // Borrowed hashing keeps repeated coverage queries allocation-free.
        let hash = Self::hash(r);
        if let Some(e) = self.entries.get(&hash)
            && e.key.cluster == r.cluster
            && e.key.style == r.style
            && e.key.presentation == r.presentation
        {
            // Pending coverage behaves exactly like a completed negative during layout.
            // Only completion generation changes cause the provisional row to rebuild.
            return e.covered.unwrap_or(false);
        }
        self.queue_coverage(r, hash);
        false
    }

    /// Legacy structural-coverage drivers own a fact-only job before actual drawing.
    fn queue_coverage(&mut self, r: ClusterRequest<'_>, hash: u64) {
        // Capacity pressure removes one entry, retaining all unrelated cached decisions.
        if !self.make_room() {
            // A full registry of accepted work waits for completion instead of duplicating it.
            return;
        }
        // Only accepted cold jobs need owned UTF-8 across the platform boundary.
        let key = Key {
            cluster: r.cluster.into(),
            style: r.style,
            presentation: r.presentation,
        };
        // A refused submission leaves no pending marker: a later visible redraw can retry.
        // The driver must accept ownership without synchronously discovering coverage.
        if self.driver.submit(Job {
            key: key.clone(),
            placement: None,
            validated: false,
        }) {
            // Coverage is unknown until the platform publishes this exact cluster.
            // No initial bitmap/path allocation accompanies a discovery request.
            // Hash collisions replace only this candidate and its completion ownership.
            // Remove its old FIFO position so adversarial collisions cannot grow the queue.
            if self.entries.contains_key(&hash) {
                self.order.retain(|old| *old != hash);
                self.ready.retire(hash);
            }
            self.entries.insert(
                hash,
                Entry {
                    key,
                    covered: None,
                    ink: FxHashMap::default(),
                },
            );
            // FIFO pressure is local to one entry; it never invalidates every fact.
            // Atlas descriptors have a separate lifetime from this metadata queue.
            self.order.push_back(hash);
        }
    }
    /// Moves a ready variant into its atlas; a cold variant never performs I/O here.
    fn ink(&mut self, r: ClusterRequest<'_>, p: &GlyphPlacement) -> Result<ClusterGlyph, String> {
        // Zoomed raster variants must never reuse a smaller sample buffer.
        let size = Size::of(p);
        // Stable scalar IDs preserve atlas hits independently of temporary metadata.
        // A genuine atlas miss reaches here with the complete original request.
        let hash = Self::hash(r);
        if self.entries.get(&hash).is_none_or(|e| {
            e.key.cluster != r.cluster
                || e.key.style != r.style
                || e.key.presentation != r.presentation
        }) && (self.facts.get(r) == Some(true)
            || (self.driver.validates_during_prepare() && self.eligible(r)))
        {
            // Prepared atlas hits bypass this path. Evicted cold metadata can be recreated
            // from a permanent positive fact without repeating whole-cluster discovery.
            self.restore(r, hash);
        }
        let Some(e) = self.entries.get_mut(&hash) else {
            return Ok(ClusterGlyph::Pending(first(r)));
        };
        // A delayed hash collision or rejected preparation cannot supply another cluster's ink.
        if e.key.cluster != r.cluster
            || e.key.style != r.style
            || e.key.presentation != r.presentation
            || e.covered == Some(false)
        {
            return Ok(ClusterGlyph::Pending(first(r)));
        }
        if let Some(Some(_)) = e.ink.get(&size) {
            // The downstream atlas becomes the sole owner, bounding retained source pixels.
            self.ready.take(Self::hash(r), size);
            return e
                .ink
                .remove(&size)
                .flatten()
                .ok_or("completed source ink missing".into());
        }
        self.cold_ink(r, p, hash, size)
    }

    /// Index eligibility reserves a provisional choice without claiming drawable coverage.
    fn eligible(&self, r: ClusterRequest<'_>) -> bool {
        r.cluster.len() <= 32
            && !self.driver.definitely_missing(r.cluster)
            && self.facts.get(r) != Some(false)
            && self.entries.get(&Self::hash(r)).is_none_or(|entry| {
                entry.key.cluster != r.cluster
                    || entry.key.style != r.style
                    || entry.key.presentation != r.presentation
                    || entry.covered != Some(false)
            })
    }
}
/// Preparation identity follows exact device geometry rather than row position.
impl Size {
    /// Only geometry and hinting affect source preparation, never row x/y or text tint.
    fn of(p: &GlyphPlacement) -> Self {
        Self([
            p.size_px.to_bits(),
            p.ink_limit_px,
            p.cell_height_px,
            p.baseline_px.to_bits(),
            u32::from(p.key.hint_ppem),
            p.key.hint_mode as u32,
        ])
    }
}
/// A pending box labels the first visible scalar, preserving the whole cluster's advance.
fn first(r: ClusterRequest<'_>) -> u32 {
    r.cluster
        .chars()
        .find(|c| !crate::font_set::is_default_ignorable(*c))
        .unwrap_or('�') as u32
}
/// Emoji and platform policies differ in their driver, not their frame-time machinery.
impl EmojiSource for Deferred {
    /// The producer uses the frame allowance for immediate owned ink, without waiting.
    fn begin_frame(&mut self, allowance: std::time::Duration) {
        self.driver.begin_frame(allowance);
    }
    /// Reports admitted preparation, without counting any background work.
    fn frame_preparation_time(&self) -> std::time::Duration {
        self.driver.frame_preparation_time()
    }
    /// Ready ownership alone, never outstanding discovery, justifies another idle frame.
    fn ready(&mut self) -> bool {
        self.driver.ready()
    }
    /// Expensive validation occurs once at the first actual placement on capable drivers.
    fn may_prepare(&mut self, r: ClusterRequest<'_>) -> bool {
        if self.driver.validates_during_prepare() {
            self.eligible(r)
        } else {
            self.covers(r)
        }
    }
    /// Drains only owned results already available, preserving the nonblocking frame seam.
    fn poll_changed(&mut self) -> bool {
        self.changed()
    }
    /// Transfers completed device ink independently of whether its row is still visible.
    fn take_ready(&mut self) -> Option<(GlyphPlacement, ClusterGlyph)> {
        self.ready.pop(&mut self.entries)
    }
    /// Non-frame callers may observe pending discovery without forcing it synchronously.
    fn pending(&self) -> bool {
        self.driver.pending()
    }
    /// Unresolved coverage declines immediately so the fallback chain draws tofu.
    fn can_draw(&mut self, r: ClusterRequest<'_>) -> bool {
        self.covers(r)
    }
    /// Transfers ready ink or queues a device-sized request with a provisional box.
    fn prepare(
        &mut self,
        r: ClusterRequest<'_>,
        p: &GlyphPlacement,
    ) -> Result<ClusterGlyph, String> {
        self.ink(r, p)
    }
}
/// Both sources publish completion with the identical swap-in rule (§8.1).
impl PlatformFallback for Deferred {
    /// The system policy receives the same refresh slack as the emoji policy.
    fn begin_frame(&mut self, allowance: std::time::Duration) {
        self.driver.begin_frame(allowance);
    }
    /// Both policies report their actual admitted work through the same driver seam.
    fn frame_preparation_time(&self) -> std::time::Duration {
        self.driver.frame_preparation_time()
    }
    /// The driver preserves the peeked result until the next budgeted publication turn.
    fn ready(&mut self) -> bool {
        self.driver.ready()
    }
    /// A provisional choice is invalidated when actual preparation rejects its candidate.
    fn may_prepare(&mut self, r: ClusterRequest<'_>) -> bool {
        if self.driver.validates_during_prepare() {
            self.eligible(r)
        } else {
            self.covers(r)
        }
    }
    /// Drains only owned results already available, preserving the nonblocking frame seam.
    fn poll_changed(&mut self) -> bool {
        self.changed()
    }
    /// Transfers completed device ink independently of whether its row is still visible.
    fn take_ready(&mut self) -> Option<(GlyphPlacement, ClusterGlyph)> {
        self.ready.pop(&mut self.entries)
    }
    /// Non-frame callers may observe pending discovery without forcing it synchronously.
    fn pending(&self) -> bool {
        self.driver.pending()
    }
    /// Unresolved coverage declines immediately so the fallback chain draws tofu.
    fn can_draw(&mut self, r: ClusterRequest<'_>) -> bool {
        self.covers(r)
    }
    /// Transfers ready ink or queues a device-sized request with a provisional box.
    fn prepare(
        &mut self,
        r: ClusterRequest<'_>,
        p: &GlyphPlacement,
    ) -> Result<ClusterGlyph, String> {
        self.ink(r, p)
    }
}

#[cfg(test)]
/// Controlled completions protect provisional glyphs and pressure behavior.
mod tests;
