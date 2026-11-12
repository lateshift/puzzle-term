//! Declarative annotations preserve text identity and consume input before VT (§11.3–11.4).
use super::*;
use crate::overlay::{
    anchor::{Anchor, MarkerRange},
    decoration::*,
    draw::CursorIcon,
    geometry::Cell,
    provider::{PointerEvent, PointerKind},
};
use puzzle_frame::ZSlot;
/// Uses caller-owned markers; private rewrite witnesses must never release them.
fn annotation(session: &mut TestHost, auto_remove: bool) -> Decoration {
    let line = session
        .buffer()
        .line_ids()
        .expect("valid test fixture")
        .start()
        .to_owned();
    let a = session
        .add_marker(Cell { line, column: 0 })
        .expect("valid test fixture");
    let b = session
        .add_marker(Cell { line, column: 4 })
        .expect("valid test fixture");
    Decoration {
        anchor: Anchor::Range(MarkerRange { start: a, end: b }),
        kind: DecorationKind::Highlight { fill: 0xff336699 },
        slot: ZSlot::BelowText,
        auto_remove,
        region: Some(RegionSpec::Tooltip("annotation tooltip".into())),
    }
}
/// Reflow is not a rewrite; an identical overwrite is, even when the pixels look unchanged.
#[test]
fn auto_remove_preserves_reflow_but_rejects_rewrites_and_eviction() {
    let (mut session, _, _) = fixture();
    let now = Instant::now();
    session.feed(b"alpha\r\nbeta", now);
    let source = annotation(&mut session, true);
    let id = session
        .decorations_mut()
        .add(source.clone())
        .expect("valid test fixture");
    let persistent = session
        .decorations_mut()
        .add(Decoration {
            auto_remove: false,
            ..source
        })
        .expect("valid test fixture");
    session.feed(b"!", now);
    assert!(session.decorations_mut().get(id).is_some());
    session.terminal.resize(40, 24);
    session.reconcile_decorations();
    assert!(
        session.decorations_mut().get(id).is_some(),
        "reflow retains source identity"
    );
    session.feed(b"\x1b[Halpha", now);
    assert!(
        session.decorations_mut().get(id).is_none(),
        "identical rewrite invalidates old source"
    );
    assert!(session.decorations_mut().get(persistent).is_some());
    let source = annotation(&mut session, true);
    let evicted = session
        .decorations_mut()
        .add(source)
        .expect("valid test fixture");
    // Borrow the source owner once before splitting its disjoint fields.
    let vt = &mut session.session.terminal;
    vt.primary.ring.set_limits(0, usize::MAX, &mut vt.tracked);
    session.feed(&b"\r\n".repeat(40), now);
    assert!(session.decorations_mut().get(evicted).is_none());
    assert!(
        session.decorations_mut().get(persistent).is_some(),
        "non-auto declarations remain addressable"
    );
}
/// Both platform hosts use the detached router; terminal tracking never receives a handled click.
#[test]
fn detached_pointer_consumes_before_selection_and_tracking_at_two_dprs() {
    for dpr in [1.0, 2.0] {
        for tracking in [false, true] {
            pointer_case(dpr, tracking);
        }
    }
}
/// Routes model input against presented regions while the view keeps its sealed rows.
fn pointer_case(dpr: f32, tracking: bool) {
    let (mut session, writes, _) = fixture();
    let mut context = super::overlays::text_context(dpr);
    let now = Instant::now();
    session.set_text_context(&context);
    session.feed(
        if tracking {
            b"\x1b[?1003h\x1b[?1006halpha"
        } else {
            b"alpha"
        },
        now,
    );
    let spec = annotation(&mut session, true);
    session
        .decorations_mut()
        .add(spec)
        .expect("valid test fixture");
    session.draw(now, &mut context).expect("valid test fixture");
    session.presented();
    writes.lock().expect("valid test fixture").clear();
    let rect = session
        .overlay_geometry()
        .cell_rect(Cell {
            line: session.buffer().cursor().expect("valid test fixture").line,
            column: 1,
        })
        .expect("valid test fixture");
    let point = [rect.origin[0] + 0.5, rect.origin[1] + 0.5];
    // Parser work can proceed after capture, while callbacks borrow frozen visible rows.
    session.feed(b"\x1b[2;1Hother row", now);
    for kind in [PointerKind::Press, PointerKind::Move, PointerKind::Release] {
        let result = session.route_overlay_pointer(
            &PointerEvent {
                kind,
                point,
                button: Some(0),
                modifiers: 0,
            },
            now,
        );
        assert!(result.handled, "captured move/release must remain consumed");
        assert_eq!(result.cursor, CursorIcon::Pointer);
    }
    let result = session.route_overlay_pointer(
        &PointerEvent {
            kind: PointerKind::Leave,
            point,
            button: None,
            modifiers: 0,
        },
        now,
    );
    assert_eq!(result.cursor, CursorIcon::Default);
    assert!(session.selection_range().is_none());
    assert!(writes.lock().expect("valid test fixture").is_empty());
}
/// Invalid replacement SVG must retain the prior declaration and marker lifetime.
#[test]
fn failed_decoration_update_preserves_old_definition() {
    let (mut session, _, _) = fixture();
    session.feed(b"alpha", Instant::now());
    let spec = annotation(&mut session, true);
    let id = session
        .decorations_mut()
        .add(spec.clone())
        .expect("valid test fixture");
    let invalid = Decoration {
        kind: DecorationKind::Shape {
            svg_path: "invalid".into(),
            fit: ShapeFit::Cell,
            paint: crate::overlay::draw::Paint {
                color: 0xffffffff,
                gradient: None,
            },
        },
        ..spec.clone()
    };
    assert!(session.decorations_mut().update(id, invalid).is_err());
    assert_eq!(session.decorations_mut().get(id), Some(&spec));
    assert!(session.decorations_mut().remove(id));
    assert!(!session.decorations_mut().remove(id));
    if let Anchor::Range(range) = spec.anchor {
        assert!(matches!(
            session.marker(range.start),
            crate::vt::tracked::PositionState::Present { .. }
        ));
    }
}
