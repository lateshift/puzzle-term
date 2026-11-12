//! Cold discovery must return immediately and swap in only completed whole-cluster ink.
#![allow(
    clippy::unwrap_used,
    reason = "fixture failures must fail the test at their source"
)]
use super::*;
use std::sync::mpsc;
/// Immediate producer stands in for already indexed, available loaded faces.
struct Immediate;
impl Driver for Immediate {
    fn validates_during_prepare(&self) -> bool {
        true
    }
    fn prepare_now(
        &mut self,
        _: ClusterRequest<'_>,
        _: &GlyphPlacement,
        _: bool,
    ) -> Option<Result<Option<ClusterGlyph>, String>> {
        Some(Ok(Some(ClusterGlyph::Outline(
            kurbo::BezPath::from_svg("M0,0 L1,0 L1,1 Z").unwrap(),
        ))))
    }
    fn submit(&mut self, _: Job) -> bool {
        panic!("first draw unnecessarily queued")
    }
    fn poll(&mut self) -> Option<Completion> {
        None
    }
    fn pending(&self) -> bool {
        false
    }
}
/// Admitted synchronous ink belongs to the first draw and validates coverage once.
#[test]
fn available_cold_ink_is_drawn_without_a_provisional_frame() {
    let mut source = Deferred::new(Box::new(Immediate));
    let r = request("漢");
    assert!(PlatformFallback::may_prepare(&mut source, r));
    let ClusterGlyph::Outline(path) =
        PlatformFallback::prepare(&mut source, r, &placement()).unwrap()
    else {
        panic!("covered first draw became provisional");
    };
    assert!(!path.is_empty());
    assert!(PlatformFallback::can_draw(&mut source, r));
    assert!(
        PlatformFallback::take_ready(&mut source).is_none(),
        "drawn ink queued twice"
    );
}
/// A stalled producer cannot make source calls wait; tests own completion timing.
struct Fake {
    jobs: mpsc::Sender<Job>,
    ready: mpsc::Receiver<Completion>,
    count: usize,
}

/// A first-draw producer validates its actual result rather than a discarded probe.
struct FirstDraw(Fake);

/// Metadata pressure must not forget an accepted validation and prepare it again.
#[test]
fn pending_validation_survives_registry_pressure() {
    let (jobs, rx) = mpsc::channel();
    let (tx, ready) = mpsc::channel();
    let mut source = Deferred::new(Box::new(FirstDraw(Fake {
        jobs,
        ready,
        count: 0,
    })));
    let p = placement();
    let original = request("漢");
    let _ = PlatformFallback::prepare(&mut source, original, &p).unwrap();
    let first = rx.try_recv().unwrap();
    // A deliberately stalled driver exercises admission rather than timing or retries.
    for c in 0xc000..0xd001 {
        let text = char::from_u32(c).unwrap().to_string();
        let _ = PlatformFallback::prepare(&mut source, request(&text), &p).unwrap();
    }
    let admitted = rx.try_iter().count();
    let _ = PlatformFallback::prepare(&mut source, original, &p).unwrap();
    assert!(rx.try_recv().is_err(), "pending validation was forgotten");
    assert!(admitted < 4096, "metadata admission exceeded its bound");
    let path = kurbo::BezPath::from_svg("M0,0 L1,0 L1,1 Z").unwrap();
    tx.send(Completion {
        job: first,
        covered: true,
        result: Ok(Some(ClusterGlyph::Outline(path.clone()))),
    })
    .unwrap();
    assert!(PlatformFallback::poll_changed(&mut source));
    let (_, ClusterGlyph::Outline(got)) = PlatformFallback::take_ready(&mut source).unwrap() else {
        panic!("accepted ink was discarded");
    };
    assert_eq!(got, path);
}
impl Driver for FirstDraw {
    fn validates_during_prepare(&self) -> bool {
        true
    }
    fn submit(&mut self, job: Job) -> bool {
        self.0.submit(job)
    }
    fn poll(&mut self) -> Option<Completion> {
        self.0.poll()
    }
    fn pending(&self) -> bool {
        self.0.pending()
    }
}

/// The validated outline must be the drawn outline, with no separate coverage extraction.
#[test]
fn actual_placement_validation_transfers_the_same_ink() {
    let (jobs, rx) = mpsc::channel();
    let (tx, ready) = mpsc::channel();
    let mut source = Deferred::new(Box::new(FirstDraw(Fake {
        jobs,
        ready,
        count: 0,
    })));
    let request = request("漢");
    for _ in 0..100 {
        assert!(PlatformFallback::may_prepare(&mut source, request));
        assert!(!PlatformFallback::can_draw(&mut source, request));
    }
    assert!(
        rx.try_recv().is_err(),
        "selection extracted discarded validation ink"
    );
    let p = placement();
    assert!(matches!(
        PlatformFallback::prepare(&mut source, request, &p).unwrap(),
        ClusterGlyph::Pending(_)
    ));
    let job = rx.try_recv().unwrap();
    assert!(job.placement.is_some());
    assert!(!job.validated);
    // A concurrent zoom must share the pending authoritative validation.
    let mut changed = p;
    changed.size_px *= 2.0;
    let _ = PlatformFallback::prepare(&mut source, request, &changed).unwrap();
    assert!(
        rx.try_recv().is_err(),
        "concurrent size repeated validation"
    );
    let path = kurbo::BezPath::from_svg("M0,0 L1,0 L1,1 Z").unwrap();
    tx.send(Completion {
        job,
        covered: true,
        result: Ok(Some(ClusterGlyph::Outline(path.clone()))),
    })
    .unwrap();
    assert!(PlatformFallback::poll_changed(&mut source));
    let (_, ClusterGlyph::Outline(got)) = PlatformFallback::take_ready(&mut source).unwrap() else {
        panic!("validation ink was lost");
    };
    assert_eq!(got, path);
    assert!(PlatformFallback::can_draw(&mut source, request));
    let allocations = allocation_counter::measure(|| {
        for _ in 0..100 {
            assert!(PlatformFallback::can_draw(&mut source, request));
            assert!(PlatformFallback::may_prepare(&mut source, request));
        }
    });
    assert_eq!(allocations.count_total, 0, "warm coverage allocated");
    assert!(rx.try_recv().is_err());
    // A new size requests its actual variant with the prior authoritative fact.
    let _ = PlatformFallback::prepare(&mut source, request, &changed).unwrap();
    assert!(rx.try_recv().unwrap().validated);
}

/// Composed-cluster rejection must fall through instead of requeueing indefinitely.
#[test]
fn first_draw_rejection_retires_a_composed_choice() {
    let (jobs, rx) = mpsc::channel();
    let (tx, ready) = mpsc::channel();
    let mut source = Deferred::new(Box::new(FirstDraw(Fake {
        jobs,
        ready,
        count: 0,
    })));
    let request = request("漢\u{3099}");
    assert!(PlatformFallback::may_prepare(&mut source, request));
    let _ = PlatformFallback::prepare(&mut source, request, &placement()).unwrap();
    tx.send(Completion {
        job: rx.try_recv().unwrap(),
        covered: false,
        result: Ok(None),
    })
    .unwrap();
    assert!(PlatformFallback::poll_changed(&mut source));
    assert!(!PlatformFallback::may_prepare(&mut source, request));
    assert!(!PlatformFallback::can_draw(&mut source, request));
    assert!(rx.try_recv().is_err());
}
impl Driver for Fake {
    fn submit(&mut self, job: Job) -> bool {
        self.count += 1;
        self.jobs.send(job).is_ok()
    }
    fn poll(&mut self) -> Option<Completion> {
        let result = self.ready.try_recv().ok();
        self.count -= usize::from(result.is_some());
        result
    }
    fn pending(&self) -> bool {
        self.count != 0
    }
}
/// Selection and rendering are independent asynchronous phases with the same full identity.
#[test]
fn cold_source_returns_tofu_then_swaps_completed_ink() {
    let (jobs, rx) = mpsc::channel();
    let (tx, ready) = mpsc::channel();
    let mut source = Deferred::new(Box::new(Fake {
        jobs,
        ready,
        count: 0,
    }));
    let r = ClusterRequest {
        cluster: "漢",
        style: FontStyle::Regular,
        presentation: Presentation::Text,
    };
    assert!(!PlatformFallback::can_draw(&mut source, r));
    assert!(PlatformFallback::pending(&source));
    let job = rx.try_recv().unwrap();
    // A stalled producer leaves immediate tofu and performs no repeated submission.
    for _ in 0..100 {
        assert!(!PlatformFallback::can_draw(&mut source, r));
    }
    assert!(rx.try_recv().is_err());
    tx.send(Completion {
        job,
        covered: true,
        result: Ok(None),
    })
    .unwrap();
    assert!(PlatformFallback::poll_changed(&mut source));
    assert!(PlatformFallback::can_draw(&mut source, r));
    let p = placement();
    assert!(matches!(
        PlatformFallback::prepare(&mut source, r, &p).unwrap(),
        ClusterGlyph::Pending(0x6f22)
    ));
    let job = rx.try_recv().unwrap();
    let path = kurbo::BezPath::from_vec(vec![
        kurbo::PathEl::MoveTo((0.0, 0.0).into()),
        kurbo::PathEl::LineTo((1.0, 1.0).into()),
    ]);
    tx.send(Completion {
        job,
        covered: true,
        result: Ok(Some(ClusterGlyph::Outline(path.clone()))),
    })
    .unwrap();
    assert!(PlatformFallback::poll_changed(&mut source));
    let ClusterGlyph::Outline(got) = PlatformFallback::prepare(&mut source, r, &p).unwrap() else {
        panic!("completion did not replace provisional tofu")
    };
    assert_eq!(got, path);
    assert!(!PlatformFallback::pending(&source));
}
/// More than 4,096 distinct clusters never clear unrelated decisions or alias old source IDs.
#[test]
fn pressure_keeps_recent_complete_cluster_facts() {
    let (jobs, rx) = mpsc::channel();
    let (tx, ready) = mpsc::channel();
    let mut source = Deferred::new(Box::new(Fake {
        jobs,
        ready,
        count: 0,
    }));
    let mut bytes = [0; 4];
    for n in 0..4200 {
        let r = ClusterRequest {
            cluster: char::from_u32(0x10000 + n).unwrap().encode_utf8(&mut bytes),
            style: FontStyle::Regular,
            presentation: Presentation::Text,
        };
        assert!(!PlatformFallback::can_draw(&mut source, r));
        tx.send(Completion {
            job: rx.try_recv().unwrap(),
            covered: true,
            result: Ok(None),
        })
        .unwrap();
        assert!(PlatformFallback::poll_changed(&mut source));
        assert!(PlatformFallback::can_draw(&mut source, r));
    }
    let r = ClusterRequest {
        cluster: char::from_u32(0x10000 + 4198)
            .unwrap()
            .encode_utf8(&mut bytes),
        style: FontStyle::Regular,
        presentation: Presentation::Text,
    };
    assert!(PlatformFallback::can_draw(&mut source, r));
    assert!(
        rx.try_recv().is_err(),
        "pressure discarded a recent coverage fact"
    );
}

/// Repeated Unicode benchmark cycles must retain completed scalar coverage.
#[test]
fn unicode_cycle_keeps_completed_scalar_facts() {
    let (jobs, rx) = mpsc::channel();
    let (tx, ready) = mpsc::channel();
    let mut source = Deferred::new(Box::new(Fake {
        jobs,
        ready,
        count: 0,
    }));
    let mut bytes = [0; 4];
    // These are distinct CJK scalars, rather than arbitrary composed-cluster strings.
    // More than one metadata allowance reproduces the repeated vtebench payload.
    for scalar in 0x4e00..0x6200 {
        let r = request(char::from_u32(scalar).unwrap().encode_utf8(&mut bytes));
        assert!(!PlatformFallback::can_draw(&mut source, r));
        tx.send(Completion {
            job: rx.try_recv().unwrap(),
            covered: true,
            result: Ok(None),
        })
        .unwrap();
        assert!(PlatformFallback::poll_changed(&mut source));
    }
    for scalar in 0x4e00..0x6200 {
        let r = request(char::from_u32(scalar).unwrap().encode_utf8(&mut bytes));
        assert!(
            PlatformFallback::can_draw(&mut source, r),
            "completed U+{scalar:04X} became provisional on the next Unicode cycle"
        );
    }
    assert!(
        rx.try_recv().is_err(),
        "completed scalar coverage was submitted again"
    );
}

/// Every fixture scalar uses the same native text-presentation policy.
fn request(cluster: &str) -> ClusterRequest<'_> {
    ClusterRequest {
        cluster,
        style: FontStyle::Regular,
        presentation: Presentation::Text,
    }
}

/// A slow ink result must still replace tofu after its temporary request metadata retires.
#[test]
fn late_scalar_ink_survives_metadata_retirement() {
    let (jobs, rx) = mpsc::channel();
    let (tx, ready) = mpsc::channel();
    let mut source = Deferred::new(Box::new(Fake {
        jobs,
        ready,
        count: 0,
    }));
    let r = request("漢");
    assert!(!PlatformFallback::can_draw(&mut source, r));
    tx.send(Completion {
        job: rx.try_recv().unwrap(),
        covered: true,
        result: Ok(None),
    })
    .unwrap();
    assert!(PlatformFallback::poll_changed(&mut source));
    let p = placement();
    assert!(matches!(
        PlatformFallback::prepare(&mut source, r, &p).unwrap(),
        ClusterGlyph::Pending(_)
    ));
    let slow = rx.try_recv().unwrap();
    // Hold this sized job while other completed choices exceed the temporary registry.
    let mut bytes = [0; 4];
    for scalar in 0x4e00..0x6200 {
        let other = request(char::from_u32(scalar).unwrap().encode_utf8(&mut bytes));
        assert!(!PlatformFallback::can_draw(&mut source, other));
        tx.send(Completion {
            job: rx.try_recv().unwrap(),
            covered: true,
            result: Ok(None),
        })
        .unwrap();
        assert!(PlatformFallback::poll_changed(&mut source));
    }
    let path = kurbo::BezPath::from_svg("M0,0 L1,1").unwrap();
    tx.send(Completion {
        job: slow,
        covered: true,
        result: Ok(Some(ClusterGlyph::Outline(path.clone()))),
    })
    .unwrap();
    assert!(PlatformFallback::poll_changed(&mut source));
    assert!(PlatformFallback::can_draw(&mut source, r));
    let ClusterGlyph::Outline(got) = PlatformFallback::prepare(&mut source, r, &p).unwrap() else {
        panic!("late ink was discarded after metadata retirement")
    };
    assert_eq!(got, path);
    assert!(rx.try_recv().is_err(), "completed ink was submitted again");
}

/// Zooming while rows leave the viewport must not retain every completed size variant.
#[test]
fn completed_zoom_variants_have_a_separate_retention_budget() {
    let (jobs, rx) = mpsc::channel();
    let (tx, ready) = mpsc::channel();
    let mut source = Deferred::new(Box::new(Fake {
        jobs,
        ready,
        count: 0,
    }));
    let r = ClusterRequest {
        cluster: "漢",
        style: FontStyle::Regular,
        presentation: Presentation::Text,
    };
    assert!(!PlatformFallback::can_draw(&mut source, r));
    tx.send(Completion {
        job: rx.try_recv().unwrap(),
        covered: true,
        result: Ok(None),
    })
    .unwrap();
    assert!(PlatformFallback::poll_changed(&mut source));
    for zoom in 0..32 {
        let mut p = placement();
        p.size_px += zoom as f32;
        assert!(matches!(
            PlatformFallback::prepare(&mut source, r, &p).unwrap(),
            ClusterGlyph::Pending(_)
        ));
        tx.send(Completion {
            job: rx.try_recv().unwrap(),
            covered: true,
            result: Ok(Some(ClusterGlyph::Outline(kurbo::BezPath::new()))),
        })
        .unwrap();
        assert!(PlatformFallback::poll_changed(&mut source));
    }
    // The original coverage answer survives ink pressure, so no font discovery repeats.
    assert!(PlatformFallback::can_draw(&mut source, r));
    assert_eq!(
        source.entries.values().map(|e| e.ink.len()).sum::<usize>(),
        8
    );
    assert!(matches!(
        PlatformFallback::prepare(&mut source, r, &placement()).unwrap(),
        ClusterGlyph::Pending(_)
    ));
    assert!(rx.try_recv().unwrap().placement.is_some());
}

/// Exact device geometry matches the source preparation contract.
fn placement() -> GlyphPlacement {
    GlyphPlacement {
        key: crate::glyph::GlyphKey {
            face: PLATFORM_FACE,
            glyph_id: 1,
            instance: 0,
            hint_ppem: 0,
            hint_mode: crate::glyph::HintMode::None,
        },
        origin_px: [0, 14],
        size_px: 14.0,
        color: 0xffffffff,
        advance_px: 16,
        ink_limit_px: 16,
        cell_height_px: 20,
        baseline_px: 14.0,
        cell_column: 0,
        cell_span: 2,
        effect: false,
        color_glyph: false,
    }
}
