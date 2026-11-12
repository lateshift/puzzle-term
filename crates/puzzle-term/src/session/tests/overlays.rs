//! Session/external-frame contracts for placement geometry and provider lifetimes.
use super::*;
use crate::overlay::{
    anchor::Anchor,
    draw::{DrawList, Primitive, RectSpec},
    geometry::Rect,
    model::ViewState,
    provider::EventKinds,
    view::{OverlayView, ViewCtx},
};
/// Extension paints a fixed panel and counts only actual callback work.
struct Panel(Arc<AtomicUsize>);
impl OverlayView for Panel {
    fn build(&mut self, _state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        self.0.fetch_add(1, Ordering::Relaxed);
        // Fresh glyph metadata is available even on the provider's first build.
        assert!(!ctx.geometry.placements.is_empty());
        assert!(
            ctx.buffer
                .line(ctx.geometry.visible_lines().next().expect("visible row"))
                .is_some()
        );
        out.push(
            Anchor::Viewport(Rect::default()),
            puzzle_frame::ZSlot::Top,
            Primitive::Rect(RectSpec {
                rect: Rect {
                    origin: [1.0; 2],
                    size: [3.0; 2],
                },
                color: 0xffccbbaa,
                radius: 0.0,
                stroke: 0.0,
            }),
        );
    }
}
/// Both scales use the grid's neutral placements, before any backend ink changes.
#[test]
fn geometry_equals_grid_placements_at_two_dprs() {
    for dpr in [1.0, 2.0] {
        geometry_at_scale(dpr);
    }
}
/// Runs the actual external handoff used by the native shell.
fn geometry_at_scale(dpr: f32) {
    let mut context = text_context(dpr);
    let (mut session, _, _) = fixture();
    let now = Instant::now();
    session.set_text_context(&context);
    session.feed(b"plain -> text", now);
    let count = Arc::new(AtomicUsize::new(0));
    session.overlays_mut().add_view(
        "session-panel",
        EventKinds::GRID,
        Box::new(Panel(count.clone())),
    );
    let mut snapshot = crate::frame_snapshot::FrameSnapshot::default();
    let mut builder = FrameBuilder::new(&context);
    assert_eq!(
        session
            .publish_frame(
                now,
                &mut snapshot,
                context.atlas_generation(),
                builder.style_epoch()
            )
            .expect("snapshot"),
        crate::PublishOutcome::Published
    );
    // Output arriving after capture must belong to the next overlay frame too.
    session.feed(b"!", now);
    let mut frame = RenderFrame::default();
    builder
        .build_published(&mut context, &mut snapshot, &mut frame)
        .expect("frame");
    check_geometry(&builder, &context, &frame, dpr);
    session.builder = Some(builder);
    assert_eq!(count.load(Ordering::Relaxed), 1);
    session.draw(now, &mut context).expect("late output frame");
    assert_eq!(count.load(Ordering::Relaxed), 2);
    check_idle(&mut session, &mut context, now, &count);
}
/// Geometry and submitted grid instances must agree in both coordinate systems.
fn check_geometry(builder: &FrameBuilder, context: &TextContext, frame: &RenderFrame, dpr: f32) {
    let geometry = builder.text_geometry();
    let first = geometry.placements.first().expect("plain glyph");
    assert_eq!(first.baseline[0], geometry.inset);
    assert_eq!(
        first.baseline[1],
        geometry.inset + context.metrics().baseline_px as f32 / dpr
    );
    assert_eq!(first.cells, 1);
    assert_eq!(first.cell.column, 0);
    let draw = frame.slugs.first().expect("glyph instance");
    assert_eq!(first.baseline, draw.t.map(|v| v / dpr));
    assert!(
        frame
            .rects
            .iter()
            .any(|r| r.color == 0xffccbbaa && r.rect == [dpr, dpr, 3.0 * dpr, 3.0 * dpr])
    );
}
/// A registered provider must not manufacture callbacks, frames or heap requests.
fn check_idle(
    session: &mut TestHost,
    context: &mut TextContext,
    now: Instant,
    count: &AtomicUsize,
) {
    session.set_cursor_settings(
        cursor_settings::CursorSettings {
            blink: cursor_settings::CursorBlink::Off,
            ..Default::default()
        },
        now,
    );
    session.draw(now, context);
    let allocations = allocation_counter::measure(|| {
        assert!(session.draw(now, context).is_none());
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(count.load(Ordering::Relaxed), 2);
}
/// Only bundled fonts are used so the neutral placements are reproducible.
pub(super) fn text_context(dpr: f32) -> TextContext {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    ))
    .expect("font fixture");
    let fonts =
        puzzle_text::font_set::FontSet::new(puzzle_text::FontData::new(bytes, 0), None, None)
            .expect("font set");
    TextContext::new(fonts, 14.0, dpr).expect("metrics")
}

/// Slot-specific clip stacks restore the parent and never submit empty GPU scissors.
#[test]
fn nested_clips_restore_and_slots_keep_submission_order() {
    let (mut session, _, _) = fixture();
    let mut context = text_context(1.0);
    let now = Instant::now();
    session.set_text_context(&context);
    session
        .overlays_mut()
        .add_view("session-clips", EventKinds::ALL, Box::new(Clips));
    let frame = session.draw(now, &mut context).expect("clip frame");
    assert!(
        frame
            .batches
            .windows(2)
            .all(|pair| pair[0].slot <= pair[1].slot)
    );
    let clips = frame
        .batches
        .iter()
        .filter(|batch| batch.slot == puzzle_frame::ZSlot::Top)
        .map(|batch| batch.scissor)
        .collect::<Vec<_>>();
    assert_eq!(
        clips,
        [
            Some(puzzle_frame::RectU32 {
                x: 3,
                y: 3,
                width: 4,
                height: 4
            }),
            Some(puzzle_frame::RectU32 {
                x: 0,
                y: 0,
                width: 10,
                height: 10
            }),
            None
        ]
    );
    let below = frame
        .batches
        .iter()
        .find(|batch| batch.slot == puzzle_frame::ZSlot::BelowText)
        .expect("below text");
    assert!(below.scissor.is_some());
}
/// A filled rectangle makes painter order observable in the submitted arrays.
fn panel(color: u32) -> Primitive {
    Primitive::Rect(RectSpec {
        rect: Rect {
            origin: [0.0; 2],
            size: [16.0; 2],
        },
        color,
        radius: 0.0,
        stroke: 0.0,
    })
}

/// Paints inside/outside a nested clip, with a later lower-slot rectangle.
struct Clips;
impl OverlayView for Clips {
    fn build(&mut self, _state: &dyn ViewState, _: &ViewCtx<'_>, out: &mut DrawList) {
        let anchor = Anchor::Viewport(Rect::default());
        let slot = puzzle_frame::ZSlot::Top;
        for primitive in [
            Primitive::ClipPush(Rect {
                origin: [-2.0; 2],
                size: [12.0; 2],
            }),
            Primitive::ClipPush(Rect {
                origin: [3.0; 2],
                size: [4.0; 2],
            }),
            panel(0xff000001),
            Primitive::ClipPop,
            panel(0xff000002),
            Primitive::ClipPop,
            panel(0xff000003),
        ] {
            out.push(anchor, slot, primitive);
        }
        out.push(anchor, puzzle_frame::ZSlot::BelowText, panel(0xff000004));
    }
}

/// Ink-only effects preserve neutral queries and cache restoration.
mod effects;
