//! Real Canvas readiness must precede inline ink; budget refusal stays asynchronous.
#![allow(
    clippy::unwrap_used,
    reason = "browser fixture failures are test assertions"
)]
use super::*;
use puzzle_term::text::{
    cluster_source::{ClusterGlyph, PlatformFallback},
    font_set::FontStyle,
    glyph::{GlyphKey, GlyphPlacement, HintMode},
    presentation::Presentation,
};
use std::time::Duration;
use wasm_bindgen_test::wasm_bindgen_test;

/// Independent logical cells share one tested device geometry.
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
/// All three scripts must independently earn readiness from real browser ink.
fn request(text: &str) -> ClusterRequest<'_> {
    ClusterRequest {
        cluster: text,
        style: FontStyle::Regular,
        presentation: Presentation::Text,
    }
}

/// A family's first completed background draw enables later uncached clusters.
#[wasm_bindgen_test(async)]
async fn background_draw_enables_later_first_frame_canvas_ink() {
    for (first, later, refused) in [("漢", "語", "字"), ("か", "な", "も"), ("한", "글", "말")]
    {
        let (mut source, _) = source(&[], "puzzle-readiness-test");
        let p = placement();
        PlatformFallback::begin_frame(&mut source, Duration::from_secs(1));
        assert!(matches!(
            PlatformFallback::prepare(&mut source, request(first), &p).unwrap(),
            ClusterGlyph::Pending(_)
        ));
        // A second cold character waits for readiness rather than committing to
        // a backlog of background jobs that cannot later take the inline path.
        assert!(matches!(
            PlatformFallback::prepare(&mut source, request(later), &p).unwrap(),
            ClusterGlyph::Pending(_)
        ));
        let start = web_time::Instant::now();
        while !PlatformFallback::poll_changed(&mut source) {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "worker did not finish its first draw"
            );
            yield_task().await;
        }
        assert!(matches!(
            PlatformFallback::take_ready(&mut source),
            Some((_, ClusterGlyph::Bitmap(_)))
        ));
        PlatformFallback::begin_frame(&mut source, Duration::from_secs(1));
        assert!(
            matches!(
                PlatformFallback::prepare(&mut source, request(later), &p).unwrap(),
                ClusterGlyph::Bitmap(_)
            ),
            "ready family flashed provisional ink"
        );
        assert!(PlatformFallback::frame_preparation_time(&source) > Duration::ZERO);
        // A zero allowance refuses the next uncached cluster without a Canvas call.
        PlatformFallback::begin_frame(&mut source, Duration::ZERO);
        assert!(matches!(
            PlatformFallback::prepare(&mut source, request(refused), &p).unwrap(),
            ClusterGlyph::Pending(_)
        ));
        assert_eq!(
            PlatformFallback::frame_preparation_time(&source),
            Duration::ZERO
        );
    }
}

/// Failed ink, another style/script, and an observed overrun never borrow readiness.
#[wasm_bindgen_test]
fn readiness_requires_success_and_revokes_on_overrun() {
    let mut ready = readiness::Readiness::default();
    let p = placement();
    let r = request("漢");
    let cost = Duration::from_micros(100);
    ready.record(r, &p, cost, true, false, false);
    assert!(ready.costs(r, &p).is_none());
    ready.record(r, &p, cost, true, true, false);
    assert_eq!(ready.costs(request("字"), &p), Some([cost; 2]));
    assert!(ready.costs(request("한"), &p).is_none());
    assert!(
        ready
            .costs(
                ClusterRequest {
                    style: FontStyle::Bold,
                    ..r
                },
                &p
            )
            .is_none()
    );
    ready.record(r, &p, cost * 2, false, true, true);
    assert!(
        ready.costs(r, &p).is_none(),
        "an over-budget inline call stayed eligible"
    );
    ready.record(r, &p, cost, true, true, false);
    assert!(
        ready.costs(r, &p).is_some(),
        "background ink did not restore readiness"
    );
    assert!(
        ready
            .costs(
                r,
                &GlyphPlacement {
                    ink_limit_px: p.ink_limit_px * 2,
                    ..p
                }
            )
            .is_none()
    );
}
