//! Completed negatives survive capacity pressure and retry different source/device inputs.
use super::*;
use crate::glyph::{GlyphKey, HintMode};

/// No actual font is needed: these are the exact inputs seen by a host source.
fn placement(id: u32) -> GlyphPlacement {
    GlyphPlacement {
        key: GlyphKey {
            face: crate::cluster_source::PLATFORM_FACE,
            glyph_id: id,
            instance: 0,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        cell_column: 0,
        origin_px: [0, 0],
        baseline_px: 12.0,
        size_px: 15.0,
        advance_px: 8,
        ink_limit_px: 16,
        cell_height_px: 18,
        cell_span: 2,
        color: u32::MAX,
        effect: false,
        color_glyph: false,
    }
}

/// A two-entry ceiling evicts the cold entry individually, never the whole cache.
#[test]
fn capacity_retains_recent_results_and_warm_lookup_allocates_nothing() {
    let mut cache = Cache::new(8 * std::mem::size_of::<(Key, Entry)>());
    let (a, b, c) = (placement(1), placement(2), placement(3));
    cache.insert(&a, 1);
    cache.insert(&b, 2);
    assert_eq!(cache.get(&a), Some(1));
    cache.insert(&c, 3);
    assert_eq!(cache.get(&b), None);
    let counts = allocation_counter::measure(|| {
        for _ in 0..100 {
            assert_eq!(cache.get(&a), Some(1));
            assert_eq!(cache.get(&c), Some(3));
        }
    });
    assert_eq!(counts.count_total, 0);
}

/// A missed strike or outline variant cannot suppress another size, cluster or source.
#[test]
fn result_identity_includes_source_cluster_and_device_geometry() {
    let mut cache = Cache::default();
    let a = placement(1);
    cache.insert(&a, 1);
    let mut other = a;
    other.key.glyph_id = 2;
    assert_eq!(cache.get(&other), None);
    other = a;
    other.key.face = crate::cluster_source::EMOJI_FACE;
    assert_eq!(cache.get(&other), None);
    other = a;
    other.size_px = 30.0;
    assert_eq!(cache.get(&other), None);
    other = a;
    other.ink_limit_px += 1;
    assert_eq!(cache.get(&other), None);
    other = a;
    other.cell_height_px += 1;
    assert_eq!(cache.get(&other), None);
    other = a;
    other.baseline_px += 1.0;
    assert_eq!(cache.get(&other), None);
    // Moving the same ink to another row/column changes no preparation input.
    other = a;
    other.origin_px = [100, 100];
    other.cell_column = 10;
    assert_eq!(cache.get(&other), Some(1));
    cache.clear();
    assert_eq!(cache.get(&a), None);
}

/// Pending and errors are transient, even for a cmap-positive source.
struct Transient(bool);
impl crate::cluster_source::synchronous::SynchronousSources for Transient {
    /// No catalog or geometry state is needed by this rejection witness.
    fn configure(&mut self, _: &[String], _: crate::metrics::CellMetrics) {}
    /// Eligibility is independent from the eventual complete validation result.
    fn eligible(
        &mut self,
        _: crate::cluster_source::ClusterRequest<'_>,
        _: crate::cluster_source::synchronous::SourceKind,
    ) -> bool {
        true
    }
    /// Simulates both an invalid provisional completion and a recoverable source error.
    fn prepare(
        &mut self,
        _: crate::cluster_source::ClusterRequest<'_>,
        _: &GlyphPlacement,
        _: crate::cluster_source::synchronous::SourceKind,
    ) -> Result<Option<crate::cluster_source::ClusterGlyph>, String> {
        if self.0 {
            Ok(Some(crate::cluster_source::ClusterGlyph::Pending(1)))
        } else {
            Err("temporary source failure".into())
        }
    }
}

/// A transient failure must remain retryable instead of poisoning future frames.
#[test]
fn provisional_or_error_results_are_never_cached() {
    let mut cache = Cache::default();
    let p = placement(1);
    for pending in [false, true] {
        let request = crate::cluster_source::ClusterRequest {
            cluster: "一",
            style: crate::font_set::FontStyle::Regular,
            presentation: crate::presentation::Presentation::Text,
        };
        assert!(prepare(&mut Transient(pending), &mut cache, request, &p).is_err());
        assert_eq!(cache.get(&p), None);
    }
}
