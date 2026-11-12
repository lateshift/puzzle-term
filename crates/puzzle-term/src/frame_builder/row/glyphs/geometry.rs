//! Retains neutral cluster positions beside backend-specific ink (D-28, §11.1).
//!
//! One cluster may expand into synthetic bold draws or many COLR layers.
//! Geometry reports one neutral placement with their union ink box, never a
//! false extra source cell for each layer. Raster and vector paths share it.
//!
//! The source placement carries its cell column explicitly. Harfrust mark
//! offsets belong to the baseline, and cannot be used to infer a column.
//! This distinction matters for combining marks and ligatures on either
//! raster or vector backends.
//!
//! Ink union includes all transformed corners of every Slug layer and the
//! exact rectangle of every raster component. Synthetic bold can add a
//! second draw, but contributes to the same hit box and source span. Empty
//! outlines deliberately create no ink hit target.
//!
//! Stored coordinates are row relative. Frame assembly attaches the current
//! LineId and row offset, then divides by DPR once for public geometry.
//! Rotating or recoloring ink later cannot rewrite this source-cell record.
use super::CachedRow;
use puzzle_overlay::geometry::{Cell, Glyph, Rect};
use puzzle_text::glyph::GlyphPlacement;

/// Records one placement and the union of all instances it just produced.
pub(in crate::frame_builder) fn retain(
    cache: &mut CachedRow,
    p: &GlyphPlacement,
    first: usize,
    first_raster: usize,
) {
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for draw in &cache.glyphs[first..] {
        // Axis-aligned glyphs have only two distinct extrema. The other two
        // corners cannot enlarge their box; slanted/rotated layers still need four.
        extend_slug(&mut min, &mut max, draw);
    }
    for draw in &cache.rasters[first_raster..] {
        extend(&mut min, &mut max, [draw.rect[0], draw.rect[1]]);
        extend(
            &mut min,
            &mut max,
            [draw.rect[0] + draw.rect[2], draw.rect[1] + draw.rect[3]],
        );
    }
    // Contourless shaping results have advances but no ink and no hit box.
    if !min[0].is_finite() {
        return;
    }
    cache.placements.push(Glyph {
        cell: Cell {
            line: puzzle_vt::tracked::LineId(0),
            column: p.cell_column as usize,
        },
        cells: p.cell_span as usize,
        face: p.key.face,
        glyph_id: p.key.glyph_id,
        baseline: p.origin_px.map(|value| value as f32),
        ink: Rect {
            origin: min,
            size: [max[0] - min[0], max[1] - min[1]],
        },
    });
}
/// Adds a corner to a finite axis-aligned ink union.
fn extend(min: &mut [f32; 2], max: &mut [f32; 2], point: [f32; 2]) {
    for axis in 0..2 {
        min[axis] = min[axis].min(point[axis]);
        max[axis] = max[axis].max(point[axis]);
    }
}

/// Extends the ink union without evaluating redundant axis-aligned corners.
fn extend_slug(min: &mut [f32; 2], max: &mut [f32; 2], draw: &puzzle_frame::SlugInstance) {
    let [x0, y0, x1, y1] = draw.bbox;
    if draw.m[1] == 0.0 && draw.m[2] == 0.0 {
        for [x, y] in [[x0, y0], [x1, y1]] {
            extend(min, max, point(draw, x, y));
        }
    } else {
        // Cross terms can put the extrema at either of the complementary corners.
        for x in [x0, x1] {
            for y in [y0, y1] {
                extend(min, max, point(draw, x, y));
            }
        }
    }
}

/// The same affine expression preserves floating-point evaluation on both paths.
fn point(draw: &puzzle_frame::SlugInstance, x: f32, y: f32) -> [f32; 2] {
    [
        draw.m[0] * x + draw.m[2] * y + draw.t[0],
        draw.m[1] * x + draw.m[3] * y + draw.t[1],
    ]
}

#[cfg(test)]
/// Reflections and off-diagonal transforms retain the complete four-corner bounds.
mod tests;
