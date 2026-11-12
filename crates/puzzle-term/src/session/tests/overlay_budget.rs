//! Typical overlay cost includes real M8 markers, parsed blocks and declarations.
//! Timing gates run optimized; debug retains the zero-allocation idle contract.
use super::*;
use crate::overlay::{
    anchor::{Anchor, MarkerRange},
    decoration::{Decoration, DecorationKind},
    geometry::Cell,
    provider::EventKinds,
};
use puzzle_bus::hooks::{EventKinds as BusKinds, HookCtx, Observer};
use puzzle_frame::ZSlot;

/// Observing through the public bus enables exactly the product's phase clocks.
struct Observe;
impl Observer<Command, Event> for Observe {
    fn observe(&mut self, _: &Event, _: &mut HookCtx<'_, Command, Event>) {}
}
/// Typical use has a visible scrollbar, three search hits, shell blocks and three annotations.
fn typical(dpr: f32) -> (TestHost, TextContext, Instant) {
    let (mut host, _, _) = fixture();
    host.terminal.resize(240, 70);
    let mut context = super::overlays::text_context(dpr);
    host.set_text_context(&context);
    crate::test_support::overlay_scene::models::install(&mut host).expect("sources");
    let now = Instant::now();
    let ids = host.buffer().line_ids().expect("document");
    let line = host
        .buffer()
        .lines(ids)
        .nth(50)
        .expect("visible match row")
        .id();
    for kind in [
        DecorationKind::Highlight { fill: 0x403399cc },
        DecorationKind::Box {
            stroke: 0xff3399cc,
            radius: 2.0,
        },
        DecorationKind::GutterBadge {
            text: "note".into(),
            color: 0xff3399cc,
        },
    ] {
        let a = host.add_marker(Cell { line, column: 0 }).expect("start");
        let b = host.add_marker(Cell { line, column: 5 }).expect("end");
        host.decorations_mut()
            .add(Decoration {
                anchor: Anchor::Range(MarkerRange { start: a, end: b }),
                kind,
                slot: ZSlot::AboveText,
                auto_remove: true,
                region: None,
            })
            .expect("decoration");
    }
    host.bus_mut()
        .observe(BusKinds::at(16), 0, "overlay-budget", Observe)
        .expect("observer");
    host.draw(now, &mut context).expect("warm frame");
    host.presented();
    crate::test_support::overlay_scene::models::hover(&mut host, now);
    host.draw(now, &mut context).expect("hover frame");
    // All incremental search slices have completed before measuring steady work.
    assert_eq!(host.search_state().count, 3);
    (host, context, now)
}
/// Idle owners never invoke their builders, even with all typical models installed.
#[test]
fn typical_overlays_idle_zero_work_and_allocations() {
    for dpr in [1.0, 2.0] {
        let (mut host, mut context, now) = typical(dpr);
        let later = now + Duration::from_secs(2);
        host.draw(later, &mut context);
        let measured = allocation_counter::measure(|| {
            for _ in 0..100 {
                assert!(host.draw(later, &mut context).is_none());
            }
        });
        assert_eq!(measured.count_total, 0);
    }
}
/// Force legitimate source invalidations so the budget includes every typical painter rebuild.
#[test]
#[cfg_attr(debug_assertions, ignore = "requires the release profile")]
fn typical_overlay_build_budget() {
    for dpr in [1.0, 2.0] {
        measure(dpr);
    }
}
/// Raw wall samples are retained before ordering percentiles; no outlier is removed.
fn measure(dpr: f32) {
    let (mut host, mut context, now) = typical(dpr);
    let mut samples = Vec::with_capacity(240);
    for index in 0..240 {
        let time = now + Duration::from_secs(2) + Duration::from_millis(index * 16);
        host.overlay_changed(EventKinds::ALL);
        host.needs_frame = true;
        host.draw(time, &mut context).expect("dirty frame");
        let stats = host
            .builder
            .as_mut()
            .expect("builder")
            .take_frame_statistics()
            .expect("observed");
        samples.push(stats.cpu_ms[4]);
    }
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m9-overlay-budget");
    std::fs::create_dir_all(&directory).expect("evidence");
    let raw = samples
        .iter()
        .enumerate()
        .map(|(n, ms)| format!("{n},{ms:.9}\n"))
        .collect::<String>();
    std::fs::write(
        directory.join(format!("dpr{dpr}.csv")),
        format!("sample,overlay_ms\n{raw}"),
    )
    .expect("raw samples");
    samples.sort_by(f64::total_cmp);
    let summary = format!(
        "dpr={dpr} samples={} p50_ms={:.9} p95_ms={:.9} max_ms={:.9}\n",
        samples.len(),
        samples[120],
        samples[228],
        samples[239]
    );
    std::fs::write(directory.join(format!("dpr{dpr}.txt")), &summary).expect("summary");
    assert!(samples[239] <= 0.5, "overlay budget: {summary}");
}
