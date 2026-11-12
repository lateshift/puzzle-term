//! Native find-box shadows clip fixed track targets, without disabling thumb drag.
use puzzle_overlay::{
    builtins::scrollbar::{Scrollbar, ScrollbarView},
    draw::{CursorIcon, DrawList, Primitive, RegionId},
    geometry::{Rect, TextGeometry},
    model::OverlayModel,
    models::Models,
    provider::Theme,
    regions::PresentedRegions,
    registry::ProviderId,
    shapes::ShapeCache,
    view::{OverlayView, ViewCtx},
};
use puzzle_vt::terminal::Terminal;

/// Logical bounds deliberately have a nonzero origin, like a padded surface.
fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        origin: [x, y],
        size: [width, height],
    }
}

/// Exercise the real scrollbar painter and the last-presented hit table, without GPU work.
fn targets(covered: &[Rect], dpr: f32) -> (Vec<(Rect, RegionId)>, PresentedRegions) {
    let terminal = Terminal::new(10, 4, 0, 0);
    let geometry = TextGeometry {
        dpr,
        viewport: rect(0.0, 0.0, 100.0, 120.0),
        grid: rect(0.0, 0.0, 100.0, 120.0),
        ..TextGeometry::default()
    };
    let theme = Theme::default();
    let shapes = ShapeCache::default();
    let models = Models {
        track: rect(90.0, 10.0, 6.0, 100.0),
        thumb: rect(90.0, 10.0, 6.0, 20.0),
        occluded_rects: Some(covered.into()),
        ..Models::default()
    };
    let ctx = ViewCtx {
        geometry: &geometry,
        buffer: terminal.buffer(),
        time: 0.0,
        theme: &theme,
        reduced_motion: true,
        shapes: &shapes,
        text: None,
        decorations: &[],
        models: &models,
    };
    let state = Scrollbar::default().view_state();
    let mut draws = DrawList::default();
    ScrollbarView.build(state.as_ref(), &ctx, &mut draws);
    // Resolve exactly as the view registry does before capturing presented regions.
    for draw in &mut draws.draws {
        draw.resolved = draw.anchor.sample(&terminal, None);
    }
    let regions = draws
        .draws
        .iter()
        .filter_map(|draw| match draw.primitive {
            Primitive::Region(bounds, id, _) => Some((bounds, id)),
            _ => None,
        })
        .collect();
    let mut presented = PresentedRegions::default();
    presented.capture(&geometry, std::iter::once((ProviderId(1), &draws)));
    (regions, presented)
}

#[test]
fn scrollbar_top_shadow_clips_only_track_band_and_preserves_full_thumb() {
    for dpr in [1.0, 2.0] {
        // Only one logical pixel of horizontal overlap, and four points at the top.
        let (regions, presented) = targets(&[rect(0.0, 0.0, 91.0, 14.0)], dpr);
        assert_eq!(
            regions,
            vec![
                (rect(90.0, 14.0, 6.0, 96.0), RegionId(0)),
                (rect(90.0, 10.0, 6.0, 20.0), RegionId(1)),
            ]
        );
        // The full thumb wins painter order, and the exposed track still pages.
        assert_eq!(
            presented
                .hit([93.0, 11.0])
                .map(|(_, id, cursor)| (id, cursor)),
            Some((RegionId(1), CursorIcon::Grab))
        );
        assert_eq!(
            presented.hit([93.0, 60.0]).map(|(_, id, _)| id),
            Some(RegionId(0))
        );
    }
}

#[test]
fn scrollbar_disjoint_bands_and_complete_thumb_coverage() {
    // Unsorted, overlapping obstacles form one top band and one middle band.
    let (regions, presented) = targets(
        &[
            rect(89.0, 50.0, 8.0, 5.0),
            rect(89.0, 18.0, 8.0, 12.0),
            rect(89.0, 0.0, 8.0, 22.0),
        ],
        1.0,
    );
    assert_eq!(
        regions,
        vec![
            (rect(90.0, 30.0, 6.0, 20.0), RegionId(0)),
            (rect(90.0, 55.0, 6.0, 55.0), RegionId(0)),
        ]
    );
    assert!(presented.hit([93.0, 20.0]).is_none());
    assert!(presented.hit([93.0, 52.0]).is_none());
    // Covering the thumb's left half cannot remove its full drag target.
    let (regions, _) = targets(&[rect(0.0, 0.0, 93.0, 40.0)], 1.0);
    assert!(regions.contains(&(rect(90.0, 10.0, 6.0, 20.0), RegionId(1))));
    // Two adjoining host panels can jointly cover the entire thumb.
    let (regions, _) = targets(
        &[rect(0.0, 0.0, 93.0, 40.0), rect(93.0, 0.0, 10.0, 40.0)],
        1.0,
    );
    assert!(regions.iter().all(|(_, id)| *id != RegionId(1)));
}
