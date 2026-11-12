//! Overlay-first routing protects selection/reporting and exact painter clipping (§11.4).
#![allow(
    clippy::expect_used,
    reason = "assertions identify failed pointer fixtures"
)]
use puzzle_frame::ZSlot;
use puzzle_overlay::{
    OverlayRegistry,
    anchor::Anchor,
    draw::{CursorIcon, DrawList, Primitive, RegionId},
    geometry::{Rect, TextGeometry},
    model::{ModelCtx, OverlayModel, SharedState, ViewState},
    provider::{EventKinds, Handled, PointerEvent, PointerKind, Requests, Theme},
    regions::PresentedRegions,
    shapes::ShapeCache,
    view::{OverlayView, ViewCtx},
    view_registry::{OverlaySnapshot, ViewRegistry},
};
use puzzle_vt::terminal::Terminal;
use std::sync::{Arc, Mutex};

/// Events prove transitions and captured releases reach only their addressed owner.
struct Button(Arc<Mutex<Vec<PointerKind>>>);
impl OverlayModel for Button {
    fn name(&self) -> &'static str {
        "button"
    }
    fn wants(&self) -> EventKinds {
        EventKinds::POINTER
    }
    fn view_state(&mut self) -> SharedState {
        Arc::new(())
    }
    fn on_pointer(&mut self, event: &PointerEvent, _: RegionId, _: &mut ModelCtx<'_>) -> Handled {
        self.0.lock().expect("pointer test state").push(event.kind);
        if matches!(event.kind, PointerKind::Press | PointerKind::Move) {
            Handled::Yes
        } else {
            Handled::No
        }
    }
}
/// Region drawing owns neither click history nor pointer capture.
struct ButtonView;
impl OverlayView for ButtonView {
    fn build(&mut self, _state: &dyn ViewState, _: &ViewCtx<'_>, out: &mut DrawList) {
        let anchor = Anchor::Viewport(Rect::default());
        let bounds = Rect {
            origin: [0.0; 2],
            size: [80.0; 2],
        };
        out.push(
            anchor,
            ZSlot::Top,
            Primitive::ClipPush(Rect {
                origin: [10.0; 2],
                size: [20.0; 2],
            }),
        );
        out.push(
            anchor,
            ZSlot::Top,
            Primitive::Region(bounds, RegionId(1), CursorIcon::Pointer),
        );
        out.push(anchor, ZSlot::Top, Primitive::ClipPop);
    }
}
#[test]
fn clips_hover_and_captured_release_share_one_router() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut registry = OverlayRegistry::default();
    let id = registry.add(Box::new(Button(Arc::clone(&events))), Box::new(ButtonView));
    let terminal = Terminal::new(10, 5, 0, 0);
    let geometry = TextGeometry {
        viewport: Rect {
            origin: [0.0; 2],
            size: [100.0; 2],
        },
        grid: Rect {
            origin: [4.0; 2],
            size: [80.0; 2],
        },
        ..Default::default()
    };
    let requests = Requests::default();
    let theme = Theme::default();
    let shapes = ShapeCache::default();
    let mut ctx = ModelCtx {
        geometry: &geometry,
        time: 0.0,
        theme: &theme,
        reduced_motion: false,
        requests: &requests,
        decorations: &[],
        models: &puzzle_overlay::models::Models::default(),
    };
    let regions = presented(&mut registry, &ctx, &terminal, &shapes);

    // A region's larger bounds cannot bypass its enclosing clip.
    assert!(regions.hit([5.0; 2]).is_none());
    assert!(regions.hit([15.0; 2]).is_some());
    registry.presented(regions);
    exercise(&mut registry, &mut ctx, id, &events);
}
/// Captured ownership survives leaving the region, then ends on release/removal.
fn exercise(
    registry: &mut OverlayRegistry,
    ctx: &mut ModelCtx<'_>,
    id: puzzle_overlay::registry::ProviderId,
    events: &Mutex<Vec<PointerKind>>,
) {
    let event = |kind, point| PointerEvent {
        kind,
        point,
        button: Some(0),
        modifiers: 0,
    };
    assert!(
        registry
            .route_pointer(&event(PointerKind::Press, [15.0; 2]), ctx)
            .handled
    );
    // Leaving the rectangle still sends the captured move and consumes the paired release.
    assert!(
        registry
            .route_pointer(&event(PointerKind::Move, [90.0; 2]), ctx)
            .handled
    );
    assert!(
        registry
            .route_pointer(&event(PointerKind::Release, [90.0; 2]), ctx)
            .handled
    );
    assert_eq!(
        *events.lock().expect("pointer test state"),
        [
            PointerKind::Enter,
            PointerKind::Press,
            PointerKind::Leave,
            PointerKind::Move,
            PointerKind::Release
        ]
    );
    assert!(
        !registry
            .route_pointer(&event(PointerKind::Release, [90.0; 2]), ctx)
            .handled
    );
    // Removal cannot redirect an old capture to a later registration.
    registry.route_pointer(&event(PointerKind::Press, [15.0; 2]), ctx);
    registry.remove(id);
    assert!(
        !registry
            .route_pointer(&event(PointerKind::Release, [15.0; 2]), ctx)
            .handled
    );
}

/// Presented tables, not future build geometry, authorize a fresh gesture.
fn presented(
    registry: &mut OverlayRegistry,
    ctx: &ModelCtx<'_>,
    terminal: &Terminal,
    shapes: &ShapeCache,
) -> PresentedRegions {
    registry.update(ctx);
    let mut snapshot = OverlaySnapshot::default();
    registry.publish(&mut snapshot);
    let mut views = ViewRegistry::default();
    views.accept(&mut snapshot);
    views.build(
        &snapshot,
        &ViewCtx {
            geometry: ctx.geometry,
            buffer: terminal.buffer(),
            time: ctx.time,
            theme: ctx.theme,
            reduced_motion: ctx.reduced_motion,
            shapes,
            text: None,
            decorations: &[],
            models: ctx.models,
        },
        terminal,
        None,
    );
    let mut regions = PresentedRegions::default();
    regions.capture(ctx.geometry, views.lists());
    regions
}
