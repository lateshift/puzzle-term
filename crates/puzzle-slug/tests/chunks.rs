//! Public chunk bounds and winding/AA equivalence at shared ownership edges (§11.2).
#![allow(
    clippy::expect_used,
    reason = "test fixtures require successful geometry preparation"
)]
use kurbo::{Affine, BezPath, Rect, Shape};
use proptest::prelude::*;
use puzzle_slug::{ShapeKey, SlugAtlas, atlas::FillRule, chunks, reference};

proptest! {
    #[test]
    fn chunk_limits_hold_under_zoom_rotation_and_shear(
        w in 1.0f64..900.0, h in 1.0f64..900.0,
        scale in 0.2f64..3.0, angle in -3.0f64..3.0, shear in -0.8f64..0.8
    ) {
        let path = Rect::new(0.2, 0.3, w + 0.2, h + 0.3).to_path(0.01);
        let transform = Affine::rotate(angle) * Affine::new([scale, 0.0, shear, scale, 0.0, 0.0]);
        let pieces = chunks::prepare(&path, transform).expect("valid rectangle");
        prop_assert!(!pieces.is_empty());
        for piece in pieces {
            let bbox = piece.shape.bbox;
            prop_assert!(bbox[2] - bbox[0] <= 256.0);
            prop_assert!(bbox[3] - bbox[1] <= 256.0);
            // Inspect the exact packed inputs rather than the chunk's diagnostic field.
            for band in piece.bands.horizontal.iter().chain(&piece.bands.vertical) {
                prop_assert!(band.len() <= 64);
            }
        }
    }
}

#[test]
fn guarded_chunks_preserve_holes_curves_and_fractional_translation() {
    // A large curved contour and a reversed rectangular hole exercise winding,
    // not just isolated convex fills. Translation makes integer ownership edges
    // fractional in path space, exposing insufficient antialiasing guard bands.
    let mut path =
        BezPath::from_svg("M0,20 Q320,-80 640,20 L640,170 Q320,270 0,170 Z").expect("fixture SVG");
    path.move_to((170.2, 60.3));
    path.line_to((170.2, 140.3));
    path.line_to((470.2, 140.3));
    path.line_to((470.2, 60.3));
    path.close_path();
    let transform = Affine::rotate(0.12);
    let prepared = chunks::prepare(&path, transform).expect("bounded partition");
    assert!(prepared.len() > 1);
    let mut atlas = SlugAtlas::default();
    let full = atlas
        .prepare(ShapeKey(1), &(transform * &path), FillRule::NonZero, 0)
        .expect("full oracle");
    let pieces: Vec<_> = prepared
        .iter()
        .enumerate()
        .map(|(i, chunk)| {
            let handle = atlas
                .prepare_quadratics(
                    ShapeKey(i as u64 + 2),
                    chunk.shape.clone(),
                    chunk.bands.clone(),
                    FillRule::NonZero,
                    0,
                )
                .expect("chunk atlas");
            (chunk.owner, handle)
        })
        .collect();
    compare_pixels(&atlas, full, &pieces);
}

/// Samples every pixel once through its owner, including both sides of all seams.
fn compare_pixels(
    atlas: &SlugAtlas,
    full: puzzle_slug::ShapeHandle,
    pieces: &[(Rect, puzzle_slug::ShapeHandle)],
) {
    for y in -60..290 {
        for x in -40..670 {
            let at = [x as f32 + 0.13, y as f32 - 0.31];
            let source = reference::sample(atlas, full, at, [1.0; 2]).expect("source coverage");
            let mut owners = 0;
            let mut coverage = 0.0;
            for (owner, handle) in pieces {
                // Scissor edges use ceil in device space, matching the frame painter.
                if x >= (owner.x0 + 0.37).ceil() as i32
                    && x < (owner.x1 + 0.37).ceil() as i32
                    && y >= (owner.y0 + 0.81).ceil() as i32
                    && y < (owner.y1 + 0.81).ceil() as i32
                {
                    owners += 1;
                    coverage =
                        reference::sample(atlas, *handle, at, [1.0; 2]).expect("chunk coverage");
                }
            }
            assert!(owners <= 1, "double blend at {x},{y}");
            assert!(
                (source - coverage).abs() <= 0.003,
                "seam at {x},{y}: source={source} chunk={coverage}"
            );
        }
    }
}

#[test]
fn dense_plot_keeps_every_actual_band_bounded() {
    let mut path = BezPath::new();
    path.move_to((0.0, 80.0));
    for index in 1..5000 {
        let x = index as f64 / 8.0;
        path.line_to((x, 80.0 + (x * 0.14).sin() * 45.0));
    }
    // Expand the whole dashed stroke first: round joins/caps are shared by
    // overlapping chunk neighborhoods rather than reconstructed at cut edges.
    let stroke = kurbo::Stroke::new(2.0)
        .with_caps(kurbo::Cap::Round)
        .with_join(kurbo::Join::Round);
    let path = kurbo::stroke(path.iter(), &stroke, &kurbo::StrokeOpts::default(), 0.001);
    let pieces = chunks::prepare(&path, Affine::IDENTITY).expect("dense stroke chunks");
    assert!(pieces.len() > 1);
    for piece in pieces {
        for band in piece.bands.horizontal.iter().chain(&piece.bands.vertical) {
            assert!(band.len() <= 64);
        }
    }
}
