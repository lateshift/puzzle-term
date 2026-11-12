//! Exact Slug oracle coverage followed by variant-specific color blending.
//!
//! The sampler reads packed CPU mirrors, unchanged from M2. Hinting already
//! changed the input curves before this function; optical weighting uses
//! the reference W flag. F1 and F2 modify only the final sRGB pixel value.
//! A one-pixel expansion around the em bbox includes edge coverage beyond
//! the mathematical outline, just as the dilated GPU quad will in M3.
use super::{DARK, PANEL_GAP, Position, Style};
use crate::slug::hinting::variant::composite;
use puzzle_slug::{
    SlugAtlas,
    atlas::ShapeHandle,
    reference::{OracleImage, sample},
};

/// Samples the exact CPU oracle within a dilated glyph bounding rectangle.
pub(super) fn draw_glyph(
    atlas: &SlugAtlas,
    mut handle: ShapeHandle,
    position: Position,
    style: Style,
    image: &mut OracleImage,
) -> Result<usize, String> {
    // The atlas handle is copied because the weight experiment changes the
    // per-draw flag, never the shared packed outline or its other uses.
    if style.weighted {
        // The reference's W flag applies sqrt after its ordinary coverage.
        handle.band_info |= 1 << 29;
    }
    let bbox = handle.bbox;
    // Em-space bounds become device pixels around the integer origin.
    // Y is inverted because outlines are y-up and PNG rows are y-down.
    let x0 = (position.left as f32 + bbox[0] * style.size).floor() as i32 - 1;
    let x1 = (position.left as f32 + bbox[2] * style.size).ceil() as i32 + 1;
    let y0 = (position.baseline as f32 - bbox[3] * style.size).floor() as i32 - 1;
    let y1 = (position.baseline as f32 - bbox[1] * style.size).ceil() as i32 + 1;
    // The extra pixel allows fringe coverage beyond a geometric edge,
    // matching the vertex shader's dilated quad rather than clipping it.
    let mut ink = 0;
    for y in y0.max(0)..y1.min(image.height as i32) {
        for x in x0.max(0)..x1.min(style.panel_width as i32) {
            // Pixel centres map back to em space. The inverse physical
            // scale supplies the reference's analytic fwidth replacement.
            let at = [
                (x as f32 + 0.5 - position.left as f32) / style.size,
                (position.baseline as f32 - y as f32 - 0.5) / style.size,
            ];
            let coverage = sample(atlas, handle, at, [1.0 / style.size; 2])
                .map_err(|error| error.to_string())?;
            // Both panels use the same coverage. Differences in the output
            // then come from color compositing alone, not raster position.
            if coverage > 0.0 {
                paint_pair(image, x as u32, y as u32, style, coverage);
                ink += 1;
            }
        }
    }
    Ok(ink)
}

/// Colors identical coverage on paper-white and #1e1e1e backgrounds.
fn paint_pair(image: &mut OracleImage, x: u32, y: u32, style: Style, a: f32) {
    // Locate matching pixels in the two panels. The gutter is not part of
    // either sample so labels cannot obscure the paired comparison area.
    let first = (y as usize * image.width as usize + x as usize) * 4;
    let second_x = x + style.panel_width + PANEL_GAP;
    let second = (y as usize * image.width as usize + second_x as usize) * 4;
    // The exact sRGB or gamma-space formula comes from D-27, after Slug
    // has produced one coverage for both schemes.
    let dark_ink = composite(a, 255, 0, style.blend);
    let light_ink = composite(a, DARK, 255, style.blend);
    image.pixels[first..first + 3].fill(dark_ink);
    image.pixels[second..second + 3].fill(light_ink);
}
