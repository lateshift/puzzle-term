//! Warm full native emoji/CJK row rebuilds must remain allocation-free (P1).
#![allow(
    clippy::expect_used,
    reason = "fixture construction must fail the contract"
)]
use super::Fixture;
/// An invalidated full screen exercises source coverage, layout and instance caches.
#[test]
fn native_warm_full_frames_allocate_nothing() {
    for dpr in [1.0, 2.0] {
        for emoji in [true, false] {
            let mut fixture = Fixture::new(emoji, dpr).expect("native fixture");
            let prepared = fixture.context.prepared_glyphs();
            let measured = allocation_counter::measure(|| {
                for _ in 0..4 {
                    fixture.redraw().expect("warm full redraw");
                }
            });
            assert_eq!(measured.count_total, 0, "dpr={dpr} emoji={emoji}");
            assert_eq!(fixture.context.prepared_glyphs(), prepared);
            println!(
                "native warm full frame dpr={dpr} emoji={emoji} allocations=0 raster_bytes={}",
                fixture.context.raster_bytes()
            );
        }
    }
}
