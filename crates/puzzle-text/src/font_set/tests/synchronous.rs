//! D-41 complete sources may never turn covered glyphs into provisional boxes.
use super::*;
use crate::cluster_source::{
    ClusterGlyph, ClusterRequest,
    synchronous::{SourceKind, SynchronousSources},
};

/// An eligible candidate can fail validation or violate the synchronous seam.
struct Candidate(bool);
impl SynchronousSources for Candidate {
    /// These behavior probes use fixed primary geometry and no platform catalog.
    fn configure(&mut self, _: &[String], _: crate::metrics::CellMetrics) {}
    /// Reserve a source request so preparation, rather than family selection, decides.
    fn eligible(&mut self, _: ClusterRequest<'_>, _: SourceKind) -> bool {
        true
    }
    /// A malformed source must not leak its Pending result into a drawable frame.
    fn prepare(
        &mut self,
        _: ClusterRequest<'_>,
        _: &GlyphPlacement,
        _: SourceKind,
    ) -> Result<Option<ClusterGlyph>, String> {
        Ok(self.0.then_some(ClusterGlyph::Pending(0x4e00)))
    }
}

/// Native preparation rejects Pending instead of returning a covered diagnostic box.
#[test]
fn complete_source_cannot_return_provisional_ink() {
    let (mut fonts, mut placement) = b_fixture();
    fonts.set_synchronous_sources(Box::new(Candidate(true)));
    placement.key = fonts
        .source_key(
            "一",
            FontStyle::Regular,
            crate::cluster_source::PLATFORM_FACE,
        )
        .unwrap_or_else(|error| panic!("source key: {error}"));
    assert!(fonts.prepare_source(&placement).is_err());
}

/// Exhausted actual validation has a final negative identity, not an outstanding job.
#[test]
fn complete_negative_has_no_pending_glyph_work() {
    let (mut fonts, mut placement) = b_fixture();
    fonts.set_synchronous_sources(Box::new(Candidate(false)));
    placement.key = fonts
        .source_key(
            "一",
            FontStyle::Regular,
            crate::cluster_source::PLATFORM_FACE,
        )
        .unwrap_or_else(|error| panic!("source key: {error}"));
    assert!(matches!(
        fonts.prepare_source(&placement),
        Ok(ClusterGlyph::Missing(0x4e00))
    ));
    assert!(!fonts.sources_pending());
}

/// Counts completed validation, allowing the public FontSet seam to expose retries.
struct CountingCandidate(std::sync::Arc<std::sync::atomic::AtomicUsize>);
impl SynchronousSources for CountingCandidate {
    /// Configuration changes no font files in this deterministic source.
    fn configure(&mut self, _: &[String], _: crate::metrics::CellMetrics) {}
    /// Cmap eligibility deliberately passes although actual candidate ink does not.
    fn eligible(&mut self, _: ClusterRequest<'_>, _: SourceKind) -> bool {
        true
    }
    /// Returning None means every candidate was checked, rather than transient work.
    fn prepare(
        &mut self,
        _: ClusterRequest<'_>,
        _: &GlyphPlacement,
        _: SourceKind,
    ) -> Result<Option<ClusterGlyph>, String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(None)
    }
}

/// Warm missing glyphs do no host work; changed geometry/policy retries actual validation.
#[test]
fn completed_missing_retries_only_for_changed_identity_or_policy() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let (mut fonts, mut p) = b_fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    fonts.set_synchronous_sources(Box::new(CountingCandidate(calls.clone())));
    let cell = crate::metrics::CellMetrics {
        width_px: 8,
        height_px: 16,
        baseline_px: 12,
        device_size: 12.0,
        cap_snapped: false,
        underline_px: 1,
    };
    fonts.configure_synchronous_sources(&[], cell);
    p.key = fonts
        .source_key(
            "一",
            FontStyle::Regular,
            crate::cluster_source::PLATFORM_FACE,
        )
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(matches!(
        fonts.prepare_source(&p),
        Ok(ClusterGlyph::Missing(0x4e00))
    ));
    let counts = allocation_counter::measure(|| {
        for _ in 0..100 {
            assert!(matches!(
                fonts.prepare_source(&p),
                Ok(ClusterGlyph::Missing(0x4e00))
            ));
        }
        fonts.configure_synchronous_sources(&[], cell);
        assert!(matches!(
            fonts.prepare_source(&p),
            Ok(ClusterGlyph::Missing(0x4e00))
        ));
    });
    assert_eq!(counts.count_total, 0);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    p.size_px *= 2.0;
    assert!(fonts.prepare_source(&p).is_ok());
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    fonts.configure_synchronous_sources(&["Another family".into()], cell);
    assert!(fonts.prepare_source(&p).is_ok());
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    fonts.set_synchronous_sources(Box::new(CountingCandidate(calls.clone())));
    assert!(fonts.prepare_source(&p).is_ok());
    assert_eq!(calls.load(Ordering::Relaxed), 4);
}
