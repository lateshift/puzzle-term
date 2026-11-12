//! Independent source colors, anchor coordinates and padding checks protect the image scenes.
//!
//! Geometry checks use authored cell coordinates and source colors, rather than
//! rebuilding the same calculations used by the image assembler. Padding is
//! checked across the entire perimeter, catching both negative-row crops and
//! right-edge reflow mistakes. Saturated quadrant checks avoid filter seams;
//! the separate placeholder seam case compares every pixel against a full quad.
//! The transparent case samples bare paper, while its screenshot also shows
//! real glyphs through alpha for the pending human review.
use super::*;
use puzzle_term::{
    frame::{PipelineKind, RenderFrame},
    text::metrics::CellMetrics,
};
/// Checks source selection and cell origins before any pixels or reference files are considered.
pub(super) fn geometry(
    frame: &RenderFrame,
    name: &str,
    metrics: CellMetrics,
) -> Result<(), String> {
    // A blank frame cannot satisfy this case even when its text background looks valid.
    let images = frame.images.as_ref().ok_or("image sources absent")?;
    // Negative z uses two distinct images so either missing layer is observable.
    let expected = if name == "negative-z" { 2 } else { 1 };
    // Counting sources also detects a mistakenly duplicated upload per placement.
    if images.sources.len() != expected {
        return Err(format!("{name}: wrong source count"));
    }
    // Image batches can share the raster array with emoji, so select by pipeline.
    let quads: Vec<_> = frame
        .batches
        .iter()
        .filter(|batch| matches!(batch.pipeline, PipelineKind::Image(_)))
        // Original raster offsets remain valid even when ordinary glyphs share the array.
        .map(|batch| frame.rasters[batch.instances.start as usize])
        .collect();
    // No raster instance means no image was submitted, regardless of storage success.
    if quads.is_empty() {
        return Err(format!("{name}: no image quads"));
    }
    // Sixteen cells at width twenty become column six on the second ten-cell row.
    if name == "reflow"
        && quads[0].rect[..2]
            != [
                16.0 + 6.0 * metrics.width_px as f32,
                16.0 + metrics.height_px as f32,
            ]
    {
        return Err(format!(
            "reflow: anchor {:?} did not wrap to row 1 column 6",
            quads[0].rect
        ));
    }
    // Seven line feeds on a six-row screen place the original anchor two rows above.
    if name == "half-viewport" && quads[0].rect[1] != 16.0 - 2.0 * metrics.height_px as f32 {
        return Err(format!(
            "half-viewport: anchor {:?} did not scroll",
            quads[0].rect
        ));
    }
    // Selecting the original right half retains its full vertical source extent.
    if name == "source-crop" && quads[0].uv != [0.5, 0.0, 1.0, 1.0] {
        return Err("wrong crop UVs".into());
    }
    Ok(())
}
/// Exact colors are sampled far from bilinear boundaries and glyph outlines.
pub(super) fn pixels(image: &RgbaImage, name: &str, metrics: CellMetrics) -> Result<(), String> {
    if name == "placeholder-seams" {
        return super::seams::verify(image, metrics);
    }
    // Measured cells locate samples while expected source colors remain literal.
    let cell = [metrics.width_px, metrics.height_px];
    // Every outer padding pixel must retain the default paper, including a clipped image.
    // Include both horizontal and vertical padding; no image may paint either inset.
    for y in 0..image.height {
        for x in 0..image.width {
            if (x < 16 || y < 16 || x >= image.width - 16 || y >= image.height - 16)
                && rgb(image, x, y) != [14, 18, 26]
            {
                return Err(format!("{name}: ink escaped grid at {x},{y}"));
            }
        }
    }
    // Saturated interior colors distinguish cropping from merely drawing some ink.
    let (x, y, expected) = match name {
        // Upper-left source quadrant is opaque red in the raw fixture.
        "placeholders" => (17, 17, [255, 0, 0]),
        "rgba" => (16 + cell[0], 16 + cell[0], [255, 0, 0]),
        // The right-half crop begins in the green quadrant, never in the original red.
        "source-crop" => (16 + cell[0], 16 + cell[0], [0, 255, 0]),
        // Authored green cell paper must completely hide the lower image at this point.
        "negative-z" => (17, 17, [0, 255, 0]),
        // This sample follows the tracked cell into the second rewrapped physical row.
        "reflow" => (17 + 6 * cell[0], 17 + cell[1], [255, 0, 0]),
        // The top source half is in history, leaving the lower blue quadrant at grid top.
        "half-viewport" => (17, 17, [0, 0, 255]),
        // Near the lower edge avoids authored ASCII while still testing PNG alpha.
        "png-alpha" => (17, 16 + 6 * cell[0], [7, 9, 141]),
        _ => return Err("unknown image pixel fixture".into()),
    };
    // Two byte values allow GPU unorm rounding, matching the existing golden tolerance.
    let actual = rgb(image, x, y);
    // Any wrong channel identifies the exact pixel rather than only failing a whole PNG.
    if actual
        .into_iter()
        .zip(expected)
        .any(|(a, b)| a.abs_diff(b) > 2)
    {
        return Err(format!(
            "{name}: pixel {x},{y} = {actual:?}, expected {expected:?}"
        ));
    }
    if name == "placeholders" {
        // The four source quadrants cross cell boundaries. A missing inherited
        // tile cannot satisfy these samples, even if the first explicit cell draws.
        // All coordinates are inside the four-by-four prototype rectangle,
        // except the two deliberately unknown references immediately to its right.
        // Samples avoid image edges, so bilinear filtering cannot hide a wrong tile.
        for (x, y, expected) in [
            // The last column inherits its source column from the cells to its left.
            (16 + 3 * cell[0] + cell[0] / 2, 17, [0, 255, 0]),
            // Equal device offsets in x/y select the square source's lower quadrants.
            (17, 16 + 3 * cell[0], [0, 0, 255]),
            (16 + 3 * cell[0], 16 + 3 * cell[0], [255, 255, 0]),
            // Unknown image ID: any tofu or stale source would change this paper.
            (
                16 + 5 * cell[0] + cell[0] / 2,
                16 + cell[1] / 2,
                [14, 18, 26],
            ),
            // Known image but unknown placement: no prototype may be substituted.
            (
                16 + 6 * cell[0] + cell[0] / 2,
                16 + cell[1] / 2,
                [14, 18, 26],
            ),
        ] {
            // Opaque interior texels and untouched paper have exact byte values;
            // unlike the transparent fixture, these samples need no alpha rounding.
            if rgb(image, x, y) != expected {
                return Err(format!(
                    "placeholder pixel {x},{y}: {:?}, expected {expected:?}",
                    rgb(image, x, y)
                ));
            }
        }
    }
    // The screenshot remains available for aspects requiring human eyes (R15).
    Ok(())
}
/// Readbacks are tightly packed RGBA8; fixture coordinates stay inside their grid.
pub(super) fn rgb(image: &RgbaImage, x: u32, y: u32) -> [u8; 3] {
    // The GPU readback helper strips row padding before exposing this byte array.
    let offset = ((y * image.width + x) * 4) as usize;
    // Omit alpha only in these color assertions: all target paper is opaque.
    [
        image.pixels[offset],
        image.pixels[offset + 1],
        image.pixels[offset + 2],
    ]
}
