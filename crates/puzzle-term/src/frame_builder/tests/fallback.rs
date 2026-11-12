//! Completed source ink must survive rapidly replaced Unicode screens (P6).
use super::*;
use puzzle_text::{
    cluster_source::{ClusterGlyph, PLATFORM_FACE},
    deferred::{Completion, Deferred, Driver, Job},
    font_set::FontStyle,
    glyph::{GlyphPlacement, HintMode},
};
use std::sync::mpsc;

/// The test controls publication independently of whether the cluster is still visible.
struct Worker {
    /// Captures exact owned jobs for the simulated background producer.
    jobs: mpsc::Sender<Job>,
    /// Only published results are visible to the frame thread.
    ready: mpsc::Receiver<Completion>,
}
impl Driver for Worker {
    fn submit(&mut self, job: Job) -> bool {
        self.jobs.send(job).is_ok()
    }
    fn poll(&mut self) -> Option<Completion> {
        self.ready.try_recv().ok()
    }
    fn pending(&self) -> bool {
        false
    }
}

/// Scrolling away before publication cannot discard otherwise reusable prepared ink.
#[test]
fn completed_ink_is_cached_without_a_visible_cell() {
    let (jobs, rx) = mpsc::channel();
    let (tx, ready) = mpsc::channel();
    let mut loaded = fonts();
    loaded.set_platform_fallback(Box::new(Deferred::new(Box::new(Worker { jobs, ready }))));
    let mut context = TextContext::new(loaded, 14.0, 2.0).expect("context");
    assert_eq!(
        context
            .fonts
            .as_mut()
            .expect("fonts")
            .choose_face("漢", FontStyle::Regular)
            .expect("choice"),
        None
    );
    tx.send(Completion {
        job: rx.recv().expect("coverage job"),
        covered: true,
        result: Ok(None),
    })
    .expect("coverage result");
    context.begin_frame().expect("coverage publication");
    let key = context
        .fonts
        .as_mut()
        .expect("fonts")
        .source_key("漢", FontStyle::Regular, PLATFORM_FACE)
        .expect("source key");
    let p = placement(key);
    assert_eq!(p.key.hint_mode, HintMode::None);
    assert!(matches!(
        context
            .fonts
            .as_mut()
            .expect("fonts")
            .prepare_source(&p)
            .expect("request"),
        ClusterGlyph::Pending(_)
    ));
    let ink = kurbo::BezPath::from_svg("M0,0 L1,0 L1,1 L0,1 Z").expect("fixture path");
    tx.send(Completion {
        job: rx.recv().expect("ink job"),
        covered: true,
        result: Ok(Some(ClusterGlyph::Outline(ink))),
    })
    .expect("ink result");
    // No VT cell names 漢 here. Publication itself must retain the completed atlas ink.
    context.begin_frame().expect("offscreen publication");
    assert!(
        context.backend.get(key).is_some(),
        "completed offscreen ink never entered the atlas"
    );
}

/// Exact grid geometry is retained with the job after its visible cell disappears.
fn placement(key: puzzle_text::glyph::GlyphKey) -> GlyphPlacement {
    GlyphPlacement {
        key,
        origin_px: [0, 28],
        size_px: 28.0,
        color: 0xffffffff,
        advance_px: 32,
        ink_limit_px: 32,
        cell_height_px: 40,
        baseline_px: 28.0,
        cell_column: 0,
        cell_span: 2,
        effect: false,
        color_glyph: false,
    }
}

/// Blank Braille has no ink by definition; dots and solid/shaded blocks remain checked.
#[test]
fn retained_ink_excludes_only_the_blank_braille_pattern() {
    let mut context = TextContext::new(fonts(), 14.0, 2.0).expect("metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(8, 1, 0, 0);
    terminal.advance("⠀⠁█░▒▓".as_bytes());
    let mut frame = RenderFrame::default();
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("builtin frame");
    let ink = builder.retained_ink_samples(&mut context);
    assert_eq!(ink.len(), 5);
    assert!(ink.iter().all(|cell| cell.ready));
    assert!(ink.iter().any(|cell| cell.key.glyph_id == 0x2801));
    assert!(ink.iter().all(|cell| cell.key.glyph_id != 0x2800));
}

/// JetBrains Mono substitutes empty components before the visible ligature glyph.
#[test]
fn retained_ink_excludes_confirmed_empty_ligature_components() {
    let mut context = TextContext::new(fonts(), 14.0, 2.0).expect("metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(32, 1, 0, 0);
    terminal.advance(b"==> -> === != !== <= >=");
    let mut frame = RenderFrame::default();
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("ligatures");
    // The real font's GSUB rules, rather than fabricated glyph IDs, supply the empty cases.
    assert!(builder.rows[0].sources.iter().any(|source| source.empty));
    let ink = builder.retained_ink_samples(&mut context);
    assert!(!ink.is_empty());
    assert!(ink.iter().all(|sample| sample.ready));
    assert!(ink.iter().all(|sample| sample.key.glyph_id != 1742));
}
