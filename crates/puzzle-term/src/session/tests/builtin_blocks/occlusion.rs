//! A native find box cannot cover command-copy ink or leave its old hit target.
use super::*;
use crate::{SessionMessage, overlay::geometry::Rect};

/// Hover a real OSC block through its presented observer, at either device scale.
pub(super) fn hovered(dpr: f32) -> (TestHost, TextContext, Instant, Rect) {
    let (mut host, _, _) = fixture();
    let mut context = super::super::overlays::text_context(dpr);
    host.set_text_context(&context);
    host.install_model_overlays();
    let now = Instant::now();
    host.feed(
        b"\x1b]133;C\x07one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\n\x1b]133;D;0\x07",
        now,
    );
    host.draw(now, &mut context).expect("output");
    host.presented();
    let block = host.snapshot.overlays.models.blocks[0];
    let row = host.overlay_geometry().line_rect(block.first).expect("row");
    host.route_overlay_pointer(
        &PointerEvent {
            kind: PointerKind::Move,
            point: [row.origin[0] + 10.0, row.origin[1] + 2.0],
            button: None,
            modifiers: 0,
        },
        now,
    );
    host.draw(now, &mut context).expect("hover");
    let original = copy_bounds(&host).expect("copy");
    (host, context, now, original)
}

/// Regions are the actual retained output, not a duplicate of placement arithmetic.
fn copy_bounds(host: &TestHost) -> Option<Rect> {
    host.builder
        .as_ref()
        .expect("builder")
        .overlays
        .registry
        .lists()
        .flat_map(|(_, list)| &list.draws)
        .find_map(|draw| match draw.primitive {
            Primitive::Region(rect, id, _) if id.0 & (1 << 63) != 0 => Some(rect),
            _ => None,
        })
}

#[test]
fn copy_control_slides_left_then_below_or_disappears_with_its_hit_target() {
    for dpr in [1.0, 2.0] {
        let (mut host, mut context, now, original) = hovered(dpr);
        let grid = host.overlay_geometry().grid;
        let height = host.overlay_geometry().cell_size()[1];
        let right = Rect {
            origin: [
                original.origin[0] + original.size[0] / 2.0,
                original.origin[1],
            ],
            size: original.size,
        };
        cover(&mut host, &mut context, now, right);
        let shifted = copy_bounds(&host).expect("left placement");
        assert_eq!(shifted.origin[1], original.origin[1]);
        assert_eq!(shifted.origin[0] + shifted.size[0], right.origin[0]);
        assert!(!shifted.overlaps(right));
        // A full-width box removes the left alternative; skip the partially covered row.
        let top = Rect {
            origin: [grid.origin[0], original.origin[1]],
            size: [grid.size[0], height + 1.0],
        };
        cover(&mut host, &mut context, now, top);
        let below = copy_bounds(&host).expect("next fully visible row");
        assert_eq!(
            below.origin,
            [original.origin[0], original.origin[1] + 2.0 * height]
        );
        assert!(!below.overlaps(top));
        cover(&mut host, &mut context, now, grid);
        assert_eq!(copy_bounds(&host), None);
        assert!(!host.builder.as_ref().expect("builder").overlays.registry.lists()
            .flat_map(|(_, list)| &list.draws)
            .any(|draw| matches!(&draw.primitive, Primitive::Text(text) if text.text == "Copy output")));
    }
}

/// Replacement coverage invalidates the view while preserving its command hover.
fn cover(host: &mut TestHost, context: &mut TextContext, now: Instant, rect: Rect) {
    host.apply(SessionMessage::OccludedRects(vec![rect]), now)
        .expect("coverage");
    host.draw(now, context).expect("occluded hover");
}
