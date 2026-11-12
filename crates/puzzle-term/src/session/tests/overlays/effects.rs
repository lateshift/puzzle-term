//! Public-provider effects must move ink while preserving source geometry and cells.
use super::*;
use crate::overlay::{
    draw::Effect,
    geometry::{Cell, Range},
};
use kurbo::Affine;
/// One range covers the first three cells, including a coding ligature boundary.
struct Ink(Effect);
impl OverlayView for Ink {
    fn build(&mut self, _state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let line = ctx.geometry.visible_lines().next().expect("line");
        out.push(
            Anchor::Viewport(Rect::default()),
            puzzle_frame::ZSlot::Text,
            Primitive::GlyphEffect(
                Range {
                    start: Cell { line, column: 0 },
                    end: Cell { line, column: 2 },
                },
                self.0,
            ),
        );
    }
}
/// Removal restores the exact ordinary cache and never changes BufferView cells.
#[test]
fn effects_use_unhinted_ink_and_restore_neutral_grid() {
    for dpr in [1.0, 2.0] {
        let (mut session, _, _) = fixture();
        let mut context = text_context(dpr);
        let now = Instant::now();
        session.set_text_context(&context);
        session.feed(b"\x1b[?25lBBB", now);
        let original = session
            .draw(now, &mut context)
            .expect("baseline")
            .slugs
            .clone();
        let prepared = context.prepared_glyphs();
        let id = session.overlays_mut().add_view(
            "effect-contract",
            EventKinds::GRID,
            Box::new(transformed_ink()),
        );
        let (builder, frame) = effect_frame(&mut session, &mut context, now);
        neutral_geometry(&builder, &original, dpr);
        assert_eq!(frame.slugs[0].color, 0x8000ff00);
        assert_ne!(frame.slugs[0].m, original[0].m);
        assert_ne!(frame.slugs[0].t, original[0].t);
        if dpr == 1.0 {
            assert!(
                context.prepared_glyphs() > prepared,
                "effect prepares a distinct unhinted outline"
            );
        }
        session.builder = Some(builder);
        assert_eq!(
            session
                .buffer()
                .line(*session.buffer().line_ids().expect("IDs").start())
                .expect("row")
                .cell(0)
                .expect("cell")
                .text,
            puzzle_vt::view::CellText::Scalar('B')
        );
        session.overlays_mut().remove(id);
        let restored = session.draw(now, &mut context).expect("removed");
        assert_eq!(format!("{:?}", restored.slugs), format!("{:?}", original));
    }
}
/// Builds the sealed effect sample using the same cache warmed by the ordinary baseline.
fn effect_frame(
    session: &mut crate::session::tests::drawing::TestHost,
    context: &mut TextContext,
    now: Instant,
) -> (FrameBuilder, RenderFrame) {
    let mut snapshot = crate::frame_snapshot::FrameSnapshot::default();
    let mut builder = session.builder.take().expect("host builder");
    assert_eq!(
        session
            .publish_frame(
                now,
                &mut snapshot,
                context.atlas_generation(),
                builder.style_epoch()
            )
            .expect("capture"),
        crate::PublishOutcome::Published
    );
    let mut frame = RenderFrame::default();
    builder
        .build_published(context, &mut snapshot, &mut frame)
        .expect("effects");
    (builder, frame)
}

/// Hidden effect ink vanishes without erasing text, paper or source metadata.
#[test]
fn hidden_effect_preserves_text_and_warm_variants_allocate_nothing() {
    let (mut session, _, _) = fixture();
    let mut context = text_context(1.0);
    let now = Instant::now();
    session.set_text_context(&context);
    session.feed(b"\x1b[?25lBBB", now);
    let id = session.overlays_mut().add_view(
        "effect-contract",
        EventKinds::GRID,
        Box::new(Ink(Effect {
            hidden: true,
            ..Default::default()
        })),
    );
    assert!(
        session
            .draw(now, &mut context)
            .expect("hidden")
            .slugs
            .is_empty()
    );
    // Rebuilding provider output for identical effects must reuse all request/instance storage.
    session.overlays_mut().invalidate(id);
    session.draw(now, &mut context);
    let allocations = allocation_counter::measure(|| {
        session.overlays_mut().invalidate(id);
        assert!(
            session
                .draw(now, &mut context)
                .expect("warm")
                .slugs
                .is_empty()
        );
    });
    assert_eq!(allocations.count_total, 0);
    session.overlays_mut().remove(id);
    assert!(
        !session
            .draw(now, &mut context)
            .expect("ordinary")
            .slugs
            .is_empty()
    );
}

/// The provider moves ink while public source metadata retains its baseline and cells.
fn neutral_geometry(builder: &FrameBuilder, original: &[puzzle_frame::SlugInstance], dpr: f32) {
    let geometry = builder.text_geometry();
    assert_eq!(geometry.placements[0].cell.column, 0);
    assert_eq!(
        geometry.placements[0].baseline,
        original[0].t.map(|v| v / dpr)
    );
    assert_eq!(
        geometry
            .cell_rect(geometry.placements[0].cell)
            .expect("cell")
            .origin,
        [geometry.inset; 2]
    );
}

/// A translucent translation/rotation requires vectors even for cached grid glyphs.
fn transformed_ink() -> Ink {
    Ink(Effect {
        color: Some(0xff00ff00),
        opacity: 0.5,
        transform: Affine::translate((5.0, 3.0)) * Affine::rotate(0.2),
        hidden: false,
    })
}

/// The provider selects only a lower reserved cell, never the text's root row.
struct FragmentInk;
impl OverlayView for FragmentInk {
    fn build(&mut self, _: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let line = ctx.geometry.visible_lines().nth(1).expect("lower fragment");
        out.push(
            Anchor::Viewport(Rect::default()),
            puzzle_frame::ZSlot::Text,
            Primitive::GlyphEffect(
                Range {
                    start: Cell { line, column: 1 },
                    end: Cell { line, column: 1 },
                },
                Effect {
                    color: Some(0xff00ff00),
                    ..Default::default()
                },
            ),
        );
    }
}

/// A fragment's effect selects the whole source and composes after its sizing map.
#[test]
fn text_sizing_lower_fragment_effect_retains_whole_block_map() {
    for dpr in [1.0, 2.0] {
        let (mut session, _, _) = fixture();
        let mut context = text_context(dpr);
        let now = Instant::now();
        session.set_text_context(&context);
        session.feed(b"\x1b[?25l\x1b]66;s=2;A\x07", now);
        let original = session
            .draw(now, &mut context)
            .expect("heading")
            .slugs
            .clone();
        let id = session.overlays_mut().add_view(
            "fragment-effect",
            EventKinds::GRID,
            Box::new(FragmentInk),
        );
        let (_, frame) = effect_frame(&mut session, &mut context, now);
        assert_eq!(frame.slugs.len(), original.len());
        assert_eq!(frame.slugs[0].m, original[0].m);
        assert_eq!(frame.slugs[0].t, original[0].t);
        assert_eq!(frame.slugs[0].color, 0xff00ff00);
        session.overlays_mut().remove(id);
    }
}
