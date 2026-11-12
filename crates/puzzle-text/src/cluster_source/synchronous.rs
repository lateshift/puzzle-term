//! Complete environment ink owned by one frame-building thread (D-41).
//!
//! Emoji and general fallback share a single source owner, so mapped faces and
//! discovery metadata need neither duplicate ownership nor a glyph-worker lock.
//! The browser may continue using the separate asynchronous source traits.
use super::{ClusterGlyph, ClusterRequest};
use crate::{glyph::GlyphPlacement, metrics::CellMetrics};

/// Selects the authored-color policy or the general installed-font policy.
#[derive(Clone, Copy)]
pub enum SourceKind {
    /// Prefer authored color for emoji presentation.
    Emoji,
    /// Any drawable installed face may complete the requested cluster.
    Platform,
}

/// A host-owned complete source; implementations must never return Pending.
pub trait SynchronousSources: Send {
    /// Updates family ordering and grid geometry while retaining loaded mappings.
    fn configure(&mut self, families: &[String], metrics: CellMetrics);
    /// Checks immutable coverage metadata without extracting disposable probe ink.
    /// The host may wait once for its startup index before answering.
    fn eligible(&mut self, request: ClusterRequest<'_>, kind: SourceKind) -> bool;
    /// Finishes lookup, validation and preparation before returning owned ink.
    /// None means that every candidate failed to supply drawable cluster ink.
    fn prepare(
        &mut self,
        request: ClusterRequest<'_>,
        placement: &GlyphPlacement,
        kind: SourceKind,
    ) -> Result<Option<ClusterGlyph>, String>;
}
