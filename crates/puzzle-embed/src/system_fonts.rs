//! Native discovery is a cold byte-only seam into our neutral font engine.
//!
//! Fontique’s read-fonts version never supplies types to skrifa or harfrust.
//! Successful faces retain bounded virtual mappings for later extraction; glyphs
//! retain owned Slug geometry or samples, with no font-byte heap copy.
//! The normal window installs these sources. Harness/goldens deliberately do not.
//! One startup thread constructs the index and transfers its font owner to the
//! render thread. Every subsequent glyph miss finishes there synchronously;
//! neither glyph queues nor a mutex connect rendering to a preparation worker.
use puzzle_term::{frame_builder::context::TextContext, text::metrics::CellMetrics};
/// Neutral actual-placement geometry for explicit diagnostic probes.
mod coverage;
/// Actual-placement validation owns the same ink that reaches the ordinary atlas.
mod draw;
/// Immutable system cmap inversion, constructed only off the live frame path.
mod index;
/// Cold family/script lookup, byte budgets and warning identities.
mod lookup;
/// Completed per-candidate failures preserve bounded recency and suppress duplicate work.
mod negative;
/// Bounded successful mappings serve subsequent synchronous cache misses.
mod resident;
/// Evidence tools build once through the same complete native sources.
pub mod settle;
/// One background index owner transfers directly into synchronous rendering.
mod synchronous;
/// Optional preparation-stage counters for real-workload diagnostics.
mod timing;
/// Host-owned index lifetime remains independent of text-context settings.
pub use synchronous::Service;

/// One font owner serves emoji and general fallback in ordinary windows.
pub struct NativeFonts {
    /// Startup discovery constructs metadata; rendering later owns the same collection.
    collection: fontique::Collection,
    /// Absent in ordinary windows; shared counters never retain glyph or font data.
    timing: Option<std::sync::Arc<timing::Timing>>,
    /// All installed cmap candidates are enumerated once by the discovery owner.
    index: Option<std::sync::Arc<index::Index>>,
    /// Index construction and unsuccessful candidates use transient file ownership.
    files: fontique::SourceCache,
    /// Successful loaded faces are retired by recency when count or byte limits are exceeded.
    resident: resident::Resident,
    /// User families precede generic/script fallback in this exact order.
    families: Vec<String>,
    /// True for the emoji seam, which accepts authored color only.
    emoji_only: bool,
    /// Controls the deliberate forced-empty test environment.
    system: bool,
    /// Index construction activates platform enumeration exactly once.
    initialized: bool,
    /// Each script's OS policy is resolved once and reused for later candidates.
    scripts: std::collections::HashSet<[u8; 4]>,
    /// Current primary baseline and cell geometry, never foreign advances.
    metrics: CellMetrics,
    /// Diagnostics are once per family/font, not once per missing glyph.
    warned: std::collections::HashSet<(fontique::FamilyId, usize)>,
    /// Exact cluster/font/device failures survive later successful fallback choices.
    negative: negative::Cache,
    /// Peak borrowed bytes in any cold query; mapped bytes are not copied to heap.
    pub peak_mapped_bytes: usize,
    /// Last matched family identifies observed installed-font resolution in probes.
    pub last_family: Option<String>,
}

/// Creates sources with no system font files opened or decoded yet.
impl NativeFonts {
    /// The system flag allows deterministic empty/bundled-only discovery tests.
    pub fn new(
        families: Vec<String>,
        emoji_only: bool,
        system: bool,
        metrics: CellMetrics,
    ) -> Self {
        Self {
            collection: fontique::Collection::new(fontique::CollectionOptions {
                shared: false,
                system_fonts: false,
            }),
            timing: None,
            index: None,
            files: fontique::SourceCache::default(),
            resident: resident::Resident::new(),
            families,
            emoji_only,
            system,
            initialized: false,
            scripts: Default::default(),
            metrics,
            warned: Default::default(),
            negative: Default::default(),
            peak_mapped_bytes: 0,
            last_family: None,
        }
    }
}
/// Installs system sources only in a live window or explicitly opted-in probe.
pub fn install(context: &mut TextContext, families: &[String]) -> Result<(), String> {
    install_with_wake(context, families, std::sync::Arc::new(|| {}), &mut None)
}

/// Hosts wake when the startup index completes; a cold lookup waits once if necessary.
pub fn install_with_wake(
    context: &mut TextContext,
    families: &[String],
    wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    service: &mut Option<Service>,
) -> Result<(), String> {
    // Diagnostic counters observe complete ink without changing the atlas budget.
    if std::env::var_os("PUZZLE_GLYPH_TIMING").is_some() {
        context.measure_completions();
    }
    // Monitor and settings changes select new source policies, retaining the host index.
    if service.is_none() {
        *service = Some(Service::new(context.metrics(), wake)?);
    }
    let service = service.as_mut().ok_or("font service absent")?;
    if let Some(mut source) = service.take_source() {
        use puzzle_term::text::cluster_source::synchronous::SynchronousSources;
        source.configure(families, context.metrics());
        context.set_synchronous_sources(Box::new(source))?;
        // Seed the retained family policy before any later geometry-only update.
        context.configure_synchronous_sources(families)
    } else {
        // The context already owns discovery and its bounded mapped faces.
        context.configure_synchronous_sources(families)
    }
}

#[cfg(test)]
/// Family-order, empty-environment and installed-coverage contracts.
mod tests;
