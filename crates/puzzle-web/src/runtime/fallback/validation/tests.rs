//! Coverage facts suppress repeated scalar probes without hiding boxes in clusters.
use super::*;
use puzzle_term::text::glyph::{GlyphKey, HintMode};
use wasm_bindgen_test::wasm_bindgen_test;

/// Device geometry is explicit so validation and publication use identical samples.
fn placement() -> GlyphPlacement {
    GlyphPlacement {
        key: GlyphKey {
            face: 0,
            glyph_id: 0,
            instance: 0,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        cell_column: 0,
        origin_px: [0, 0],
        baseline_px: 24.0,
        size_px: 24.0,
        advance_px: 32,
        ink_limit_px: 32,
        cell_height_px: 36,
        cell_span: 2,
        color: u32::MAX,
        effect: false,
        color_glyph: false,
    }
}
/// A request's presentation chooses the same CSS order for scalar and final ink.
fn request(cluster: &str) -> ClusterRequest<'_> {
    ClusterRequest {
        cluster,
        style: FontStyle::Regular,
        presentation: Presentation::Text,
    }
}

/// One scalar reuses its validation ink; multi-scalar probes become permanent facts.
#[wasm_bindgen_test]
fn device_sized_validation_reuses_ink_and_scalar_facts() {
    let mut source = BrowserFallback::new(&[]);
    let p = placement();
    assert!(
        source
            .prepare_covered(request("漢"), &p, false)
            .unwrap()
            .is_some()
    );
    let before = source.operations.0.get();
    assert!(
        source
            .prepare_covered(request("字"), &p, false)
            .unwrap()
            .is_some()
    );
    let after = source.operations.0.get();
    assert_eq!((after[0] - before[0], after[2] - before[2]), (1, 1));
    assert_eq!(after[5] - before[5], 0);
    assert_eq!(after[6] - before[6], 0);
    // The shared scalar 漢 is already validated; only unseen 語 needs a probe.
    let before = after;
    assert!(
        source
            .prepare_covered(request("漢\u{200d}語\u{fe0e}"), &p, false)
            .unwrap()
            .is_some()
    );
    let after = source.operations.0.get();
    assert_eq!(after[5] - before[5], 1);
    assert_eq!((after[0] - before[0], after[2] - before[2]), (2, 2));
    assert!(
        source
            .prepare_covered(request("語\u{200d}字"), &p, false)
            .unwrap()
            .is_some()
    );
    assert_eq!(source.operations.0.get()[5], after[5]);
    assert_eq!(
        source.indexed_slots, 3,
        "ignorables must never acquire coverage slots"
    );
}

/// Supported ink elsewhere in a composed cluster cannot conceal an unsupported scalar.
#[wasm_bindgen_test]
fn mixed_cluster_rejects_missing_scalar_and_remembers_negative() {
    let mut source = BrowserFallback::new(&[]);
    let p = placement();
    let r = request("漢\u{200d}\u{10ffff}");
    assert!(source.prepare_covered(r, &p, false).unwrap().is_none());
    let calls = source.operations.0.get();
    assert!(
        source
            .prepare_covered(request("字\u{200d}\u{10ffff}"), &p, false)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        source.operations.0.get(),
        calls,
        "known negatives need no Canvas work"
    );
}
