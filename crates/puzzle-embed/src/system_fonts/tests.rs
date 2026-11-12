//! Native fallback contracts without depending on installed-family order.
#![allow(
    clippy::unwrap_used,
    reason = "contract fixture failures must fail these tests"
)]
use super::*;
use puzzle_term::text::{
    cluster_source::ClusterRequest, font_set::FontStyle, presentation::Presentation,
};

/// Full-screen queue pressure must converge after application output stops.
mod flood;

/// Completion publication is tested while every viewport row keeps changing.
mod activity;
/// Loaded mappings remain bounded and retired glyph sources remain recoverable.
mod residency;

/// Adjacent covered characters must reuse the loaded face instead of remapping its file.
#[test]
fn successful_face_mapping_is_reused() {
    let mut source = NativeFonts::new(vec![], false, true, metrics());
    source.ensure_index();
    source.timing = Some(std::sync::Arc::new(timing::Timing::default()));
    for cluster in ["漢", "字"] {
        let request = ClusterRequest {
            cluster,
            style: FontStyle::Regular,
            presentation: Presentation::Text,
        };
        assert!(source.resolve(request, &source.probe_placement()).is_some());
    }
    let report = source.timing.as_ref().unwrap().report();
    assert_eq!(report["mapping"]["count"], 1, "usable face was remapped");
}

/// Uses bundled primary geometry, independent of any installed font revision.
fn metrics() -> CellMetrics {
    TextContext::new(crate::font_data::bundled_font_set().unwrap(), 14.0, 1.0)
        .unwrap()
        .metrics()
}
/// A deliberately empty environment must select the builtin last resort.
#[test]
fn empty_system_has_only_tofu() {
    let mut fonts = crate::font_data::bundled_font_set().unwrap();
    let mut source = NativeFonts::new(vec![], false, false, metrics());
    assert!(
        source
            .resolve(
                ClusterRequest {
                    cluster: "漢",
                    style: FontStyle::Regular,
                    presentation: Presentation::Text
                },
                &source.probe_placement()
            )
            .is_none()
    );
    assert_eq!(fonts.choose_face("漢", FontStyle::Regular).unwrap(), None);
    // Exercise the final frame path: an empty native chain must still draw our box.
    let mut context = TextContext::new(fonts, 14.0, 1.0).unwrap();
    let mut builder = puzzle_term::frame_builder::FrameBuilder::new(&context);
    let mut vt = puzzle_term::vt::terminal::Terminal::new(8, 2, 0, 0);
    vt.advance("\x1b[?25l漢".as_bytes());
    let mut frame = puzzle_term::frame::RenderFrame::default();
    builder.build(&mut context, &vt, &mut frame).unwrap();
    assert!(
        !frame.slugs.is_empty(),
        "builtin tofu must provide visible geometry"
    );
    assert!(frame.rasters.is_empty());
}
/// User family order wins even when both registered fonts cover the cluster.
#[test]
fn configured_family_order() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/fonts/JetBrainsMono-Variable.ttf"),
    )
    .unwrap();
    for order in [["First", "Second"], ["Second", "First"]] {
        let mut source = NativeFonts::new(
            order.iter().map(|s| s.to_string()).collect(),
            false,
            false,
            metrics(),
        );
        for name in ["First", "Second"] {
            source.collection.register_fonts(
                bytes.clone().into(),
                Some(fontique::FontInfoOverride {
                    family_name: Some(name),
                    ..Default::default()
                }),
            );
        }
        let r = ClusterRequest {
            cluster: "A",
            style: FontStyle::Regular,
            presentation: Presentation::Text,
        };
        assert!(source.resolve(r, &source.probe_placement()).is_some());
        assert_eq!(source.last_family.as_deref(), Some(order[0]));
    }
}

/// Real installed fonts must draw whole emoji sequences and missing scripts.
#[test]
fn installed_complete_cluster_resolution() {
    let m = metrics();
    let mut source = NativeFonts::new(vec![], false, true, m);
    for cluster in [
        "漢",
        "字",
        "か",
        "な",
        "한",
        "글",
        "𝄞",
        "🦀",
        "✌️",
        "☺️",
        "👨‍👩‍👧‍👦",
        "🇩🇪",
        "👍🏽",
    ] {
        let r = ClusterRequest {
            cluster,
            style: FontStyle::Regular,
            presentation: puzzle_term::text::presentation::presentation(cluster),
        };
        assert!(
            source.resolve(r, &source.probe_placement()).is_some(),
            "missing installed cluster {cluster}"
        );
        crate::test_support::report(format_args!(
            "system cluster={cluster} family={:?} peak_mapped_bytes={} retained_font_copy_bytes=0",
            source.last_family, source.peak_mapped_bytes
        ));
    }
}

/// Counts Rust heap separately from retained virtual CJK/emoji mappings.
#[test]
fn native_mapping_memory_is_released() {
    let mut source = NativeFonts::new(vec![], false, true, metrics());
    // The approved immutable cmap index is measured separately from retained font bytes.
    source.ensure_index();
    let measured = allocation_counter::measure(|| {
        for cluster in ["漢", "🦀"] {
            let r = ClusterRequest {
                cluster,
                style: FontStyle::Regular,
                presentation: puzzle_term::text::presentation::presentation(cluster),
            };
            assert!(source.resolve(r, &source.probe_placement()).is_some());
        }
    });
    // This counts Rust heap only, not CoreText metadata, resident mmap pages or GPU memory.
    crate::test_support::report(format_args!(
        "native mapping Rust_heap_current={} Rust_heap_peak={} peak_mapped_bytes={} retained_font_copy_bytes=0",
        measured.bytes_current, measured.bytes_max, source.peak_mapped_bytes
    ));
    assert!(
        measured.bytes_current < 4 * 1024 * 1024,
        "discovery retained a large font copy"
    );
}

/// Reproduces the Unicode flood's synchronous discovery stall before repair.
#[test]
fn unicode_discovery_timing() {
    // These unassigned scalars occur in the user's 46,607-scalar Unicode corpus.
    let scalars = [
        0x378, 0x379, 0x380, 0x381, 0x382, 0x383, 0x38b, 0x38d, 0x3a2, 0x530, 0x557, 0x558, 0x58b,
        0x58c, 0x590, 0x5c8, 0x5c9, 0x5ca, 0x5cb, 0x5cc, 0x5cd, 0x5ce, 0x5cf, 0x5eb, 0x5ec, 0x5ed,
        0x5ee, 0x5f5, 0x5f6, 0x5f7, 0x5f8, 0x5f9,
    ];
    let mut source = NativeFonts::new(vec![], false, true, metrics());
    let start = std::time::Instant::now();
    for scalar in scalars.into_iter().filter_map(char::from_u32) {
        let mut bytes = [0; 4];
        let r = ClusterRequest {
            cluster: scalar.encode_utf8(&mut bytes),
            style: FontStyle::Regular,
            presentation: Presentation::Text,
        };
        let before = std::time::Instant::now();
        let covered = source.resolve(r, &source.probe_placement()).is_some();
        crate::test_support::report(format_args!(
            "discovery scalar={:X} covered={covered} ms={:.3}",
            scalar as u32,
            before.elapsed().as_secs_f64() * 1000.0
        ));
    }
    crate::test_support::report(format_args!(
        "discovery 32 scalars total_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.0
    ));
}

/// Separates the one-time index's actual retained Rust heap from mappings and glyph caches.
#[test]
fn unicode_index_memory_and_negative_hits() {
    let metrics = metrics();
    let mut source = None;
    let start = std::time::Instant::now();
    let index = allocation_counter::measure(|| {
        let mut fonts = NativeFonts::new(vec![], false, true, metrics);
        fonts.ensure_index();
        source = Some(fonts);
    });
    let source = source.unwrap();
    crate::test_support::report(format_args!(
        "Unicode index Rust_heap_current={} Rust_heap_peak={} elapsed_ms={:.3} peak_mapped_bytes={}",
        index.bytes_current,
        index.bytes_max,
        start.elapsed().as_secs_f64() * 1000.0,
        source.peak_mapped_bytes
    ));
    let measured = allocation_counter::measure(|| {
        let index = source.index.as_ref().unwrap();
        // Unsupported answers come from a permanent table, even far beyond 4,096 queries.
        for _ in 0..46607 {
            assert!(index.missing("\u{378}", false));
        }
    });
    assert_eq!(
        measured.count_total, 0,
        "negative lookups allocated or rebuilt coverage"
    );
}
