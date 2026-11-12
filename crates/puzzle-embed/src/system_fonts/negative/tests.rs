//! Negative candidate lookup must skip work without aliasing or allocating.
#![allow(
    clippy::unwrap_used,
    reason = "test fixture setup and negative-cache proof"
)]
use super::*;
use puzzle_term::text::{font_set::FontStyle, presentation::Presentation};

/// Positive fallback elsewhere must not erase an earlier candidate failure.
#[test]
fn repeats_are_allocation_free_and_geometry_and_font_do_not_alias() {
    let fonts = super::super::NativeFonts::new(
        vec![],
        false,
        false,
        puzzle_term::frame_builder::context::TextContext::new(
            crate::font_data::bundled_font_set().unwrap(),
            14.,
            1.,
        )
        .unwrap()
        .metrics(),
    );
    let mut placement = fonts.probe_placement();
    let request = ClusterRequest {
        cluster: "漢",
        style: FontStyle::Regular,
        presentation: Presentation::Text,
    };
    let mut cache = Cache::default();
    cache.insert(request, &placement, 17, false);
    let measured = allocation_counter::measure(|| {
        for _ in 0..1000 {
            assert!(cache.contains(request, &placement, 17, false));
        }
    });
    assert_eq!(measured.count_total, 0);
    assert!(!cache.contains(request, &placement, 18, false));
    assert!(!cache.contains(
        ClusterRequest {
            cluster: "字",
            ..request
        },
        &placement,
        17,
        false
    ));
    placement.size_px *= 2.;
    assert!(!cache.contains(request, &placement, 17, false));
    // An explicit source-policy change permits a new evaluation, never a timed expiry.
    cache.clear();
    placement.size_px /= 2.;
    assert!(!cache.contains(request, &placement, 17, false));
}

/// A cmap-positive invisible glyph must never repeat failed extraction under warm output.
#[test]
fn no_outline_candidate_is_evaluated_only_once() {
    let metrics = puzzle_term::frame_builder::context::TextContext::new(
        crate::font_data::bundled_font_set().unwrap(),
        15.,
        2.,
    )
    .unwrap()
    .metrics();
    let mut fonts = super::super::NativeFonts::new(vec![], false, true, metrics);
    fonts.ensure_index();
    fonts.timing = Some(std::sync::Arc::new(super::super::timing::Timing::default()));
    let placement = fonts.probe_placement();
    let request = ClusterRequest {
        cluster: "\u{3000}",
        style: FontStyle::Regular,
        presentation: Presentation::Text,
    };
    assert!(fonts.resolve(request, &placement).is_none());
    assert!(!fonts.negative.clusters.is_empty());
    // The existing 32-file/512-MiB traversal cap can stop before all candidates.
    // Subsequent queries finish unvisited faces, rather than retrying completed failures.
    let face_count = fonts.index.as_ref().unwrap().faces.len();
    let mut before = fonts.timing.as_ref().unwrap().report();
    for _ in 0..face_count {
        assert!(fonts.resolve(request, &placement).is_none());
        let next = fonts.timing.as_ref().unwrap().report();
        if next["extraction"] == before["extraction"] {
            break;
        }
        before = next;
    }
    assert!(before["extraction"]["count"].as_u64().unwrap() <= face_count as u64);
    for _ in 0..10 {
        assert!(fonts.resolve(request, &placement).is_none());
    }
    let after = fonts.timing.as_ref().unwrap().report();
    assert_eq!(before["extraction"], after["extraction"]);
    assert_eq!(before["mapping"], after["mapping"]);
}

/// Pressure removes the oldest individual fact while preserving recently accessed ones.
#[test]
fn eviction_keeps_recent_failures_without_clearing_the_working_set() {
    let metrics = puzzle_term::frame_builder::context::TextContext::new(
        crate::font_data::bundled_font_set().unwrap(),
        15.,
        2.,
    )
    .unwrap()
    .metrics();
    let fonts = super::super::NativeFonts::new(vec![], false, false, metrics);
    let p = fonts.probe_placement();
    let r = ClusterRequest {
        cluster: "漢",
        style: FontStyle::Regular,
        presentation: Presentation::Text,
    };
    let mut cache = Cache::default();
    cache.insert(r, &p, 1, false);
    cache.contains(r, &p, 2, false);
    cache.insert(r, &p, 2, false);
    // Refresh the older insertion before admitting another unrelated failure.
    assert!(cache.contains(r, &p, 1, false));
    assert!(cache.evict());
    assert!(cache.contains(r, &p, 1, false));
    assert!(!cache.contains(r, &p, 2, false));
    assert!(!cache.clusters.is_empty());
}
