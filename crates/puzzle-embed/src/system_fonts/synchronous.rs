//! Startup discovery transfers its sole font owner to synchronous rendering.
//!
//! The index is built once in the background, independently of primary ASCII.
//! A fallback lookup before discovery finishes joins that startup thread once.
//! Afterwards every mapped face, parser and source result belongs directly to
//! the render thread. There is no glyph mailbox, admission clock or result lane.
use super::NativeFonts;
use puzzle_term::text::{
    cluster_source::{
        ClusterGlyph, ClusterRequest,
        synchronous::{SourceKind, SynchronousSources},
    },
    glyph::GlyphPlacement,
    metrics::CellMetrics,
};
use std::{sync::Arc, thread::JoinHandle};

/// The host retains diagnostic counters while its context owns all font state.
pub struct Service {
    /// Installation transfers this source exactly once into its text context.
    source: Option<Source>,
    /// Scalar stage observations contain neither mapped fonts nor prepared ink.
    #[cfg(feature = "lock-metrics")]
    timing: Option<Arc<super::timing::Timing>>,
}

impl Service {
    /// Starts metadata discovery without blocking window creation on font files.
    pub fn new(metrics: CellMetrics, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        // Counters are optional: ordinary windows keep no per-call sample vectors.
        let timing = std::env::var_os("PUZZLE_GLYPH_TIMING")
            .map(|_| Arc::new(super::timing::Timing::default()));
        // Discovery and later rendering write scalar observations into this shared owner.
        let measured = timing.clone();
        // Only startup metadata work runs here; no channel accepts glyph requests.
        let discovery = std::thread::Builder::new()
            .name("puzzle-font-discovery".into())
            .spawn(move || {
                // Defer system enumeration until the background thread has started.
                let mut fonts = NativeFonts::new(vec![], false, true, metrics);
                // Attach measurement before enumeration so cold index cost stays visible.
                fonts.timing = measured;
                // Indexing releases every temporary mapping before returning its owner.
                fonts.ensure_index();
                // This sole startup notification carries no glyph completion or budget.
                wake();
                // Returning ownership ends this thread instead of waiting for glyph jobs.
                fonts
            })
            .map_err(|e| format!("font discovery: {e}"))?;
        // The host keeps counters; installation moves the remaining state into core.
        Ok(Self {
            // The unjoined handle is inert until a real fallback lookup needs its index.
            source: Some(Source {
                // A thread handle owns startup completion without a shared mutable font lock.
                discovery: Some(discovery),
                // No second collection is created while startup discovery is in progress.
                fonts: None,
                // Installation supplies configured ordering before the first lookup.
                families: vec![],
                metrics,
            }),
            #[cfg(feature = "lock-metrics")]
            timing,
        })
    }
    /// Moves the complete owner into the frame-building context, retaining only counters.
    pub(super) fn take_source(&mut self) -> Option<Source> {
        // Subsequent settings changes configure that same context-owned source.
        self.source.take()
    }
    /// Copies optional stage observations directly from the render-owned font source.
    #[cfg(feature = "lock-metrics")]
    pub fn timing_report(&self) -> serde_json::Value {
        // Reporting cannot obtain or mutate the font collection on another thread.
        self.timing
            .as_ref()
            .map_or(serde_json::Value::Null, |t| t.report())
    }
}

/// Both fallback policies borrow this single directly owned font collection.
pub(super) struct Source {
    /// A join is needed only on the first fallback lookup, never on the input thread.
    discovery: Option<JoinHandle<NativeFonts>>,
    /// Successful startup transfers collection, index and mappings together.
    fonts: Option<NativeFonts>,
    /// Policy changes retain discovery rather than constructing another font index.
    families: Vec<String>,
    /// Primary grid geometry is supplied by the owning text context.
    metrics: CellMetrics,
}

impl Source {
    /// Obtains startup ownership once; a failed discovery worker remains a recoverable negative.
    fn fonts(&mut self) -> Option<&mut NativeFonts> {
        // Taking the handle makes the potentially blocking join strictly one-time.
        if let Some(discovery) = self.discovery.take() {
            // Input and PTY parsing continue while this render-thread call waits.
            match discovery.join() {
                Ok(mut fonts) => {
                    // Apply policy chosen while metadata discovery was still running.
                    fonts.families.clone_from(&self.families);
                    // Actual placements use the latest primary baseline and cell geometry.
                    fonts.metrics = self.metrics;
                    // Collection, index and any successful maps now have one render owner.
                    self.fonts = Some(fonts);
                }
                Err(_) => tracing::error!(
                    event = "font.discovery_failed",
                    "startup font discovery stopped"
                ),
            }
        }
        // Later calls borrow directly, with no mutex or worker handoff.
        self.fonts.as_mut()
    }
}

/// Joins startup discovery when a surface closes before its first fallback lookup.
impl Drop for Source {
    /// The render owner drops this source independently of Session messages and host mailboxes.
    fn drop(&mut self) {
        // An unused startup task must not outlive surface/context teardown.
        // Joining runs on the rendering owner; UI input has already returned.
        // If lookup took the handle, all font state is already owned locally.
        if let Some(discovery) = self.discovery.take() {
            // The returned font owner drops here, releasing catalog and file metadata.
            if discovery.join().is_err() {
                tracing::error!(
                    event = "font.discovery_failed",
                    "startup discovery stopped at teardown"
                );
            }
        }
    }
}

/// Both source identities are policies of the same directly owned catalog.
impl SynchronousSources for Source {
    /// Applies the current family order and geometry while preserving mapped faces.
    fn configure(&mut self, families: &[String], metrics: CellMetrics) {
        // Family ordering is cold configuration, never a per-glyph copied policy.
        self.families = families.to_vec();
        self.metrics = metrics;
        // A still-running index reads this policy when its ownership transfers.
        if let Some(fonts) = self.fonts.as_mut() {
            if fonts.families != self.families {
                fonts.negative.clear();
            }
            fonts.families.clone_from(&self.families);
            fonts.metrics = metrics;
        }
    }
    /// Cmap eligibility reserves an identity without extracting or discarding geometry.
    fn eligible(&mut self, request: ClusterRequest<'_>, kind: SourceKind) -> bool {
        let Some(fonts) = self.fonts() else {
            return false;
        };
        fonts
            .index
            .as_ref()
            .is_some_and(|index| !index.missing(request.cluster, matches!(kind, SourceKind::Emoji)))
    }
    /// Prepares every candidate synchronously at the actual requested placement.
    fn prepare(
        &mut self,
        request: ClusterRequest<'_>,
        placement: &GlyphPlacement,
        kind: SourceKind,
    ) -> Result<Option<ClusterGlyph>, String> {
        let Some(fonts) = self.fonts() else {
            return Ok(None);
        };
        // Color-only and general fallback reuse exactly the same loaded-face registry.
        fonts.emoji_only = matches!(kind, SourceKind::Emoji);
        // Validation owns the drawn shape. No scheduling allowance can defer this call.
        let ink = fonts.resolve_draw(request, placement, false);
        // Successful extraction transfers the sole owned ink result to the ordinary atlas.
        if ink.is_some() || !fonts.emoji_only {
            return Ok(ink);
        }
        // A malformed authored-color candidate must not hide usable ordinary system ink.
        fonts.emoji_only = false;
        Ok(fonts.resolve_draw(request, placement, false))
    }
}
