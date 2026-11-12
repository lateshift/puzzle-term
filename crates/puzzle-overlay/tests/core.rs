//! Public overlay contracts: tracked lifetimes, geometry and truly idle callbacks.
#![allow(
    clippy::unwrap_used,
    reason = "test fixtures require a retained draw and geometry endpoint"
)]
use puzzle_frame::ZSlot;
use puzzle_overlay::{
    OverlayRegistry,
    anchor::{Anchor, MarkerRange, Resolved},
    draw::{DrawList, Primitive, RectSpec},
    geometry::{Cell, Line, Range, Rect, TextGeometry},
    model::{ModelCtx, OverlayModel, SharedState, ViewState},
    provider::{EventKinds, Requests, Theme},
    view::{OverlayView, ViewCtx},
    view_registry::{OverlaySnapshot, ViewRegistry},
};
use puzzle_vt::{
    terminal::Terminal,
    tracked::{LineId, PositionState},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// A normal extension counts work and paints one tracked cell.
struct Counter {
    count: Arc<AtomicUsize>,
    anchor: Anchor,
}
impl OverlayView for Counter {
    fn build(&mut self, _state: &dyn ViewState, _: &ViewCtx<'_>, out: &mut DrawList) {
        self.count.fetch_add(1, Ordering::Relaxed);
        out.push(
            self.anchor,
            ZSlot::AboveText,
            Primitive::Rect(RectSpec {
                rect: Rect {
                    origin: [0.0; 2],
                    size: [1.0; 2],
                },
                color: u32::MAX,
                radius: 0.0,
                stroke: 0.0,
            }),
        );
        out.draws.last_mut().unwrap().auto_remove = true;
    }
}
#[test]
fn anchors_follow_output_reflow_and_eviction() {
    let mut terminal = Terminal::new(8, 2, 3, 4096);
    terminal.advance(b"abcdefghij");
    let marker = terminal.tracked.track(LineId(1), 1);
    let range = Anchor::Range(MarkerRange {
        start: marker,
        end: marker,
    });
    terminal.advance(b"\r\nmore\r\nlast");
    assert_eq!(
        terminal.tracked.get(marker),
        PositionState::Present {
            line: LineId(1),
            column: 1
        }
    );
    // Reflow changes the endpoint's row/column through the shared registry.
    terminal.resize(4, 2);
    let PositionState::Present { line, column } = terminal.tracked.get(marker) else {
        panic!("retained after reflow")
    };
    assert_eq!(
        range.sample(&terminal, None),
        Resolved::Cells(Range {
            start: Cell { line, column },
            end: Cell { line, column }
        })
    );
    let count = Arc::new(AtomicUsize::new(0));
    let mut registry = OverlayRegistry::default();
    let mut views = ViewRegistry::default();
    let mut slot = OverlaySnapshot::default();
    registry.add_view(
        "core-counter",
        EventKinds::GRID,
        Box::new(Counter {
            count,
            anchor: range,
        }),
    );
    build_anchor(&mut registry, &mut views, &mut slot, &terminal);
    for _ in 0..10 {
        terminal.advance(b"\r\nevict");
    }
    build_anchor(&mut registry, &mut views, &mut slot, &terminal);
    assert_eq!(terminal.tracked.get(marker), PositionState::Gone);
    assert_eq!(views.lists().next().unwrap().1.draws.len(), 0);
}
/// Rebuilds a tracked anchor against the current bounded terminal window.
fn build_anchor(
    registry: &mut OverlayRegistry,
    views: &mut ViewRegistry,
    slot: &mut OverlaySnapshot,
    terminal: &Terminal,
) {
    build(
        registry,
        views,
        slot,
        &Requests::default(),
        &ViewCtx {
            geometry: &TextGeometry::default(),
            buffer: terminal.buffer(),
            time: 0.0,
            theme: &Theme::default(),
            reduced_motion: false,
            text: None,
            decorations: &[],
            models: &puzzle_overlay::models::Models::default(),
            shapes: &puzzle_overlay::shapes::ShapeCache::default(),
        },
        terminal,
    );
}
#[test]
fn registered_idle_provider_does_no_work_or_allocation() {
    let terminal = Terminal::new(8, 2, 0, 0);
    let count = Arc::new(AtomicUsize::new(0));
    let mut registry = OverlayRegistry::default();
    let mut views = ViewRegistry::default();
    let mut slot = OverlaySnapshot::default();
    registry.add_view(
        "core-counter",
        EventKinds::GRID,
        Box::new(Counter {
            count: count.clone(),
            anchor: Anchor::Viewport(Rect::default()),
        }),
    );
    let geometry = TextGeometry::default();
    let theme = Theme::default();
    let requests = Requests::default();
    let shapes = puzzle_overlay::shapes::ShapeCache::default();
    let ctx = ViewCtx {
        geometry: &geometry,
        buffer: terminal.buffer(),
        time: 0.0,
        theme: &theme,
        reduced_motion: false,
        text: None,
        decorations: &[],
        models: &puzzle_overlay::models::Models::default(),
        shapes: &shapes,
    };
    assert!(build(
        &mut registry,
        &mut views,
        &mut slot,
        &requests,
        &ctx,
        &terminal
    ));
    let info = allocation_counter::measure(|| {
        registry.notify(EventKinds::POINTER);
        assert!(!registry.needs_build(1.0));
        assert!(!build(
            &mut registry,
            &mut views,
            &mut slot,
            &requests,
            &ctx,
            &terminal
        ));
    });
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert_eq!(info.count_total, 0);
}
#[test]
fn logical_cells_ranges_and_hits_at_two_dprs() {
    for dpr in [1.0, 2.0] {
        let geometry = TextGeometry {
            lines: vec![
                Line {
                    version: 0,
                    id: LineId(7),
                    y: 4.0,
                },
                Line {
                    version: 0,
                    id: LineId(9),
                    y: 20.0,
                },
            ],
            cell: [8.0, 16.0],
            inset: 4.0,
            columns: 8,
            dpr,
            ..Default::default()
        };
        let pos = Cell {
            line: LineId(9),
            column: 3,
        };
        assert_eq!(
            geometry.cell_rect(pos).unwrap().device(dpr).origin,
            [28.0 * dpr, 20.0 * dpr]
        );
        assert_eq!(geometry.hit_test([29.0, 21.0]).cell, Some(pos));
        let mut rects = Vec::new();
        geometry.range_rects(
            Range {
                start: Cell {
                    line: LineId(7),
                    column: 6,
                },
                end: pos,
            },
            &mut rects,
        );
        assert_eq!(
            rects.iter().map(|r| r.size[0]).collect::<Vec<_>>(),
            [16.0, 32.0]
        );
        assert!(geometry.hit_test([1.0, 21.0]).cell.is_none());
    }
}

/// A finite model keeps updates alive through its endpoint.
struct Animated(Arc<AtomicUsize>);
impl OverlayModel for Animated {
    fn name(&self) -> &'static str {
        "finite-animation"
    }
    fn wants(&self) -> EventKinds {
        EventKinds::GRID
    }
    fn update(&mut self, ctx: &ModelCtx<'_>) {
        self.0.fetch_add(1, Ordering::Relaxed);
        ctx.animate_until(2.0);
    }
    fn view_state(&mut self) -> SharedState {
        Arc::new(())
    }
}
/// Animation endpoints are model work; registering/removing during drawing loses no state.
#[test]
fn registration_survives_skipped_frames_and_animation_finishes() {
    let mut registry = OverlayRegistry::default();
    let mut views = ViewRegistry::default();
    let mut slot = OverlaySnapshot::default();
    let count = Arc::new(AtomicUsize::new(0));
    let id = registry.add(
        Box::new(Animated(count.clone())),
        Box::new(Counter {
            count: Arc::new(AtomicUsize::new(0)),
            anchor: Anchor::Pointer,
        }),
    );
    let terminal = Terminal::new(8, 2, 0, 0);
    let geometry = TextGeometry::default();
    let theme = Theme::default();
    let requests = Requests::default();
    let shapes = puzzle_overlay::shapes::ShapeCache::default();
    for time in [0.0, 1.0, 2.0] {
        assert!(build(
            &mut registry,
            &mut views,
            &mut slot,
            &requests,
            &ViewCtx {
                geometry: &geometry,
                buffer: terminal.buffer(),
                time,
                theme: &theme,
                reduced_motion: false,
                text: None,
                decorations: &[],
                models: &puzzle_overlay::models::Models::default(),
                shapes: &shapes
            },
            &terminal
        ));
    }
    assert_eq!(count.load(Ordering::Relaxed), 3);
    assert!(!registry.needs_build(3.0));
    let new = registry.add_view(
        "core-counter",
        EventKinds::GRID,
        Box::new(Counter {
            count,
            anchor: Anchor::Pointer,
        }),
    );
    assert_ne!(new, id);
    registry.remove(id);
    registry.remove(new);
    assert!(registry.is_empty());
}

/// Providers cannot reach offscreen rows or accidentally observe later parser edits.
#[test]
fn visible_snapshot_is_frozen_and_warm_copy_is_allocation_free() {
    let mut terminal = Terminal::new(8, 2, 16, 8192);
    terminal.advance(
        "past\r\n\x1b]8;id=one;https://example.test\x07é\x1b]8;;\x07\r\n\x1b]133;A\x07now"
            .as_bytes(),
    );
    let mut snapshot = puzzle_overlay::buffer::BufferSnapshot::default();
    let top = terminal.screen().ring.scrollback_len();
    let first = terminal.screen().ring.visible(0).unwrap().id;
    let past = terminal.screen().ring.retained(0).unwrap().id;
    snapshot.capture(&terminal, top, 2);
    let view = snapshot.terminal().buffer();
    assert!(view.line(first).is_some());
    assert!(view.line(past).is_none());
    assert_eq!(view.size(), (8, 2));
    // Multiple dirty-provider captures must reuse sparse clusters and metadata storage.
    let allocations = allocation_counter::measure(|| {
        for _ in 0..4 {
            snapshot.capture(&terminal, top, 2);
        }
    });
    assert_eq!(allocations.count_total, 0);
    terminal.advance(b"later");
    assert!(
        !snapshot
            .terminal()
            .buffer()
            .text(
                snapshot.terminal().buffer().line_ids().unwrap(),
                Default::default()
            )
            .contains("later")
    );
    terminal.advance(b"\x1b[?1049hALT");
    snapshot.capture(&terminal, 0, 2);
    assert_eq!(
        snapshot.terminal().buffer().active(),
        puzzle_vt::terminal::ActiveScreen::Alternate
    );
}

/// A one-thread host calls the same separated update/publication/build surfaces.
fn build(
    registry: &mut OverlayRegistry,
    views: &mut ViewRegistry,
    slot: &mut OverlaySnapshot,
    requests: &Requests,
    ctx: &ViewCtx<'_>,
    terminal: &Terminal,
) -> bool {
    registry.update(&ModelCtx {
        geometry: ctx.geometry,
        time: ctx.time,
        theme: ctx.theme,
        reduced_motion: ctx.reduced_motion,
        models: ctx.models,
        decorations: ctx.decorations,
        requests,
    });
    registry.publish(slot);
    views.accept(slot);
    views.build(slot, ctx, terminal, registry.pointer)
}

/// A lower visible fragment owns a frozen payload even after live slot reuse.
#[test]
fn text_sizing_overlay_window_is_bounded_frozen_and_allocation_free() {
    let mut terminal = Terminal::new(12, 4, 10, 32_768);
    terminal.advance(b"\x1b]66;s=3;A\x07\x1b[4;1H\r\n");
    let line = terminal.screen().ring.retained(1).unwrap().id;
    let mut snapshot = puzzle_overlay::buffer::BufferSnapshot::default();
    snapshot.capture(&terminal, 1, 4);
    let block = snapshot
        .terminal()
        .buffer()
        .line(line)
        .unwrap()
        .multicell(1)
        .unwrap();
    assert_eq!(block.text, "A");
    assert_eq!(block.fragments[0].offset, 1);
    assert_eq!(block.fragments.len(), 2);
    #[cfg(debug_assertions)]
    snapshot
        .terminal()
        .screen()
        .ring
        .check_multicells()
        .unwrap();
    let allocations = allocation_counter::measure(|| snapshot.capture(&terminal, 1, 4));
    assert_eq!(allocations.count_total, 0);
    terminal.advance(b"\x1b[2J\x1b[H\x1b]66;s=2;B\x07");
    assert_eq!(
        snapshot
            .terminal()
            .buffer()
            .line(line)
            .unwrap()
            .multicell(1)
            .unwrap()
            .text,
        "A"
    );
    snapshot.capture(&terminal, 1, 4);
    #[cfg(debug_assertions)]
    snapshot
        .terminal()
        .screen()
        .ring
        .check_multicells()
        .unwrap();
}

/// Erasing the last block returns detached views to the ordinary no-table path.
#[test]
fn text_sizing_retired_overlay_store_has_no_sparse_work_or_allocations() {
    let mut terminal = Terminal::new(120, 40, 0, 0);
    terminal.advance(b"\x1b]66;s=2;A\x07");
    let mut snapshot = puzzle_overlay::buffer::BufferSnapshot::default();
    snapshot.capture(&terminal, 0, 40);
    terminal.advance(b"\x1b[2J");
    assert!(
        terminal
            .screen()
            .ring
            .multicells
            .as_ref()
            .unwrap()
            .is_empty()
    );
    snapshot.capture(&terminal, 0, 40);
    assert!(snapshot.terminal().screen().ring.multicells.is_none());
    let measured = allocation_counter::measure(|| {
        for _ in 0..8 {
            snapshot.capture(&terminal, 0, 40);
        }
    });
    assert_eq!(measured.count_total, 0);
}
