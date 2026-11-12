//! D-50 ownership regressions: publication, skipped slots and presented regions.
#![allow(
    clippy::expect_used,
    reason = "typed fixture state is part of the test contract"
)]
use puzzle_frame::ZSlot;
use puzzle_overlay::{
    anchor::Anchor,
    draw::*,
    geometry::*,
    model::*,
    model_registry::ModelRegistry,
    models::{Action, Models},
    provider::*,
    regions::PresentedRegions,
    shapes::ShapeCache,
    view::*,
    view_registry::*,
};
use puzzle_vt::terminal::Terminal;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// A button model counts only real immutable-state publications.
struct Button {
    /// Instrumentation cannot influence behavior or view versions.
    copies: Arc<AtomicUsize>,
    /// Its source edit changes the visible button's position.
    x: f32,
}
impl OverlayModel for Button {
    fn name(&self) -> &'static str {
        "split-button"
    }
    fn wants(&self) -> EventKinds {
        EventKinds::QUERY
    }
    fn view_state(&mut self) -> SharedState {
        self.copies.fetch_add(1, Ordering::Relaxed);
        Arc::new(self.x)
    }
    fn on_pointer(&mut self, event: &PointerEvent, _: RegionId, ctx: &mut ModelCtx<'_>) -> Handled {
        if event.kind == PointerKind::Release {
            // A completed click changes model state, not a view's mutable cache.
            self.x = 60.0;
            ctx.requests.action.set(Some(Action::Bottom));
            ctx.request_frame();
        }
        Handled::Yes
    }
}
/// Losing this view's cache cannot lose a button interaction or source position.
struct ButtonView;
impl OverlayView for ButtonView {
    fn build(&mut self, state: &dyn ViewState, _: &ViewCtx<'_>, out: &mut DrawList) {
        let x = *state
            .as_any()
            .downcast_ref::<f32>()
            .expect("button publishes f32");
        out.push(
            Anchor::Viewport(Rect::default()),
            ZSlot::Top,
            Primitive::Region(
                Rect {
                    origin: [x, 0.0],
                    size: [20.0, 20.0],
                },
                RegionId(0),
                CursorIcon::Pointer,
            ),
        );
    }
}

#[test]
fn unchanged_models_make_no_state_copy_across_three_slots() {
    let terminal = Terminal::new(10, 2, 0, 0);
    let geometry = geometry();
    let theme = Theme::default();
    let models = Models::default();
    let requests = Requests::default();
    let copies = Arc::new(AtomicUsize::new(0));
    let mut registry = button_registry(copies.clone());
    let ctx = ModelCtx {
        geometry: &geometry,
        time: 0.0,
        theme: &theme,
        reduced_motion: false,
        models: &models,
        decorations: &[],
        requests: &requests,
    };
    let mut slots: [OverlaySnapshot; 3] = std::array::from_fn(|_| OverlaySnapshot::default());
    let mut views = ViewRegistry::default();
    registry.update(&ctx);
    for slot in &mut slots {
        registry.publish(slot);
        views.accept(slot);
    }
    assert_eq!(copies.load(Ordering::Relaxed), 1);
    // Warming all slots does not invoke a clean model or manufacture another state.
    let measured = allocation_counter::measure(|| {
        for _ in 0..20 {
            registry.notify(EventKinds::GRID);
            assert!(!registry.needs_build(1.0));
            registry.update(&ctx);
            for slot in &mut slots {
                registry.publish(slot);
                views.accept(slot);
            }
        }
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(copies.load(Ordering::Relaxed), 1);
    let shapes = ShapeCache::default();
    let draw = ViewCtx {
        geometry: &geometry,
        buffer: terminal.buffer(),
        time: 0.0,
        theme: &theme,
        reduced_motion: false,
        shapes: &shapes,
        text: None,
        decorations: &[],
        models: &models,
    };
    assert!(views.build(&slots[2], &draw, &terminal, None));
    assert!(!views.build(&slots[0], &draw, &terminal, None));
}

#[test]
fn skipped_registration_arrives_and_input_uses_presented_regions() {
    let terminal = Terminal::new(10, 2, 0, 0);
    let geometry = geometry();
    let theme = Theme::default();
    let models = Models::default();
    let requests = Requests::default();
    let shapes = ShapeCache::default();
    let mut ctx = ModelCtx {
        geometry: &geometry,
        time: 0.0,
        theme: &theme,
        reduced_motion: false,
        models: &models,
        decorations: &[],
        requests: &requests,
    };
    let draw = ViewCtx {
        geometry: &geometry,
        buffer: terminal.buffer(),
        time: 0.0,
        theme: &theme,
        reduced_motion: false,
        shapes: &shapes,
        text: None,
        decorations: &[],
        models: &models,
    };
    let mut registry = ModelRegistry::default();
    registry.add(
        Box::new(Button {
            copies: Arc::new(AtomicUsize::new(0)),
            x: 0.0,
        }),
        Box::new(ButtonView),
    );
    registry.update(&ctx);
    let mut skipped = OverlaySnapshot::default();
    let mut newest = OverlaySnapshot::default();
    registry.publish(&mut skipped);
    registry.publish(&mut newest);
    let mut views = ViewRegistry::default();
    // Drain accepts registrations from skipped publications before recycling them.
    views.accept(&mut skipped);
    views.accept(&mut newest);
    assert!(views.build(&newest, &draw, &terminal, None));
    assert_presented_routing(
        &mut registry,
        &mut views,
        &mut newest,
        &mut ctx,
        &draw,
        &terminal,
    );
}

/// Logical surface geometry is independent of device DPI or glyph availability.
fn geometry() -> TextGeometry {
    TextGeometry {
        viewport: Rect {
            origin: [0.0; 2],
            size: [100.0, 30.0],
        },
        grid: Rect {
            origin: [0.0; 2],
            size: [100.0, 30.0],
        },
        dpr: 1.0,
        ..TextGeometry::default()
    }
}

/// New drawing remains invisible to input until its successful presentation message.
fn assert_presented_routing(
    registry: &mut ModelRegistry,
    views: &mut ViewRegistry,
    newest: &mut OverlaySnapshot,
    ctx: &mut ModelCtx<'_>,
    draw: &ViewCtx<'_>,
    terminal: &Terminal,
) {
    let mut presented = PresentedRegions::default();
    presented.capture(ctx.geometry, views.lists());
    registry.presented(presented);
    let event = PointerEvent {
        kind: PointerKind::Press,
        point: [5.0, 5.0],
        button: Some(0),
        modifiers: 0,
    };
    assert!(registry.route_pointer(&event, ctx).handled);
    assert!(
        registry
            .route_pointer(
                &PointerEvent {
                    kind: PointerKind::Release,
                    ..event
                },
                ctx
            )
            .handled
    );
    assert_eq!(ctx.requests.action.take(), Some(Action::Bottom));
    registry.update(ctx);
    registry.publish(newest);
    assert!(views.build(newest, draw, terminal, None));
    // This new view moved the target to x=60, but its frame has not been presented.
    assert!(registry.route_pointer(&event, ctx).handled);
    registry.route_pointer(
        &PointerEvent {
            kind: PointerKind::Release,
            ..event
        },
        ctx,
    );
    let mut presented = PresentedRegions::default();
    presented.capture(ctx.geometry, views.lists());
    registry.presented(presented);
    assert!(!registry.route_pointer(&event, ctx).handled);
    assert!(
        registry
            .route_pointer(
                &PointerEvent {
                    point: [65.0, 5.0],
                    ..event
                },
                ctx
            )
            .handled
    );
}

/// Registers a real interacting pair, independent of rendering or slot ownership.
fn button_registry(copies: Arc<AtomicUsize>) -> ModelRegistry {
    let mut registry = ModelRegistry::default();
    registry.add(Box::new(Button { copies, x: 0.0 }), Box::new(ButtonView));
    registry
}

#[test]
fn dirty_payloads_recycle_without_changing_live_publications() {
    use puzzle_overlay::model::state_pool::StatePool;
    let mut pool = StatePool::default();
    let mut retained: [SharedState; 3] = std::array::from_fn(|i| pool.publish(&i));
    // The next value cannot overwrite any of the three outstanding snapshots.
    let newest = pool.publish(&3usize);
    for (i, state) in retained.iter().enumerate() {
        assert_eq!(state.as_ref().as_any().downcast_ref::<usize>(), Some(&i));
    }
    retained[0] = newest;
    // Releasing slots permits safe reuse; dirty publications need no warm allocation.
    let allocations = allocation_counter::measure(|| {
        for i in 4..100usize {
            let value = pool.publish(&i);
            retained[i % 3] = value;
            assert_eq!(
                retained[i % 3].as_ref().as_any().downcast_ref::<usize>(),
                Some(&i)
            );
        }
    });
    assert_eq!(allocations.count_total, 0);
}
