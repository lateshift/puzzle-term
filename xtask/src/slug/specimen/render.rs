//! Device-pixel placement of em-space oracle glyphs (design §8.2, §9.6).
//!
//! Rows use integer origins and baselines, matching the intended terminal
//! placement. The specimen is diagnostic; M4 supplies full font metrics,
//! shaping and cap-height adjustments to the production text system.
//! A light panel and a dark panel share every row, making the same coverage
//! visible as black-on-white and white-on-dark without changing Slug's math.
//!
//! The font probe produces em-space paths with y up. PNG rows grow downward,
//! so each pixel centre maps to `(x-origin)/scale` and
//! `(baseline-y)/scale`. The inverse scale also supplies `ems_per_pixel`
//! to the reference sampler, replacing GPU `fwidth` analytically. Sampling
//! at pixel centres is necessary for comparison with M3's fragment shader.
//!
//! Glyph origins and baselines are rounded to integer device pixels. This
//! isolates Slug's edge behavior from accidental half-pixel shifts in the
//! diagnostic layout. The specimen deliberately does not apply hinting,
//! cap-height snapping or optical-weight adjustment, which would change
//! what the unchanged reference coverage itself looks like.
//!
//! Both color panels use the same sampled coverage. The light panel
//! subtracts it from white to produce black ink; the dark panel adds it
//! toward white from a fixed dark background. This predicts the gamma-space
//! blend described in design §9.7, without introducing a second rasterizer.
//!
//! The shape bbox bounds the per-glyph sampling rectangle. We add one
//! device pixel on each side, matching the shader quad's dilation region,
//! and clip the loop to the panel. Otherwise a fractional edge just outside
//! the mathematical outline could be omitted from the PNG. The image is
//! capped at 4096 pixels per axis to keep the local tool's memory bounded.
//!
//! The returned ink count measures actual nonzero oracle samples, not the
//! white background. Acceptance uses it to reject an empty current render.
use super::Prepared;
use puzzle_slug::{
    SlugAtlas,
    atlas::ShapeHandle,
    reference::{OracleImage, sample},
};
use std::collections::BTreeMap;

/// Separates rows and keeps glyphs off panel borders.
const MARGIN: u32 = 12;
/// Gives the two color treatments an unambiguous visible boundary.
const PANEL_GAP: u32 = 24;
/// The dark treatment uses the terminal theme's near-black background.
const DARK: u8 = 14;

/// Sizes the sheet from actual advances and the requested device scales.
/// Width is shared by all rows so the light and dark samples line up.
pub(super) fn blank_sheet(
    text: &str,
    glyphs: &BTreeMap<char, Prepared>,
    sizes: &[u32],
    dprs: &[u32],
) -> Result<OracleImage, String> {
    let mut panel_width = 0_u32;
    let mut height = MARGIN;
    for dpr in dprs {
        for size in sizes {
            let device_size = size.saturating_mul(*dpr);
            // The probe's advance is in ems; multiply only at placement time
            // so a prepared glyph serves all requested sizes and DPRs.
            let row_width = text
                .chars()
                .map(|ch| {
                    let advance = glyphs.get(&ch).map_or(0.0, |entry| entry.1);
                    (advance * device_size as f32).round().max(1.0) as u32
                })
                .sum::<u32>();
            panel_width = panel_width.max(row_width.saturating_add(MARGIN * 2));
            height = height.saturating_add(row_height(device_size));
        }
    }
    height = height.saturating_add(MARGIN);
    let width = panel_width.saturating_mul(2).saturating_add(PANEL_GAP);
    // Bound total pixels before allocating the RGBA buffer; a custom text
    // string can otherwise produce a much wider row than the review text.
    if panel_width == 0 || width > 4096 || height > 4096 {
        return Err("specimen dimensions must fit within 4096 pixels".into());
    }
    let mut image = OracleImage {
        width,
        height,
        pixels: vec![0; width as usize * height as usize * 4],
    };
    for (index, pixel) in image.pixels.chunks_exact_mut(4).enumerate() {
        // The left treatment is paper-white; the right treatment has a
        // dark terminal background, with a neutral gap between panels.
        let x = index as u32 % width;
        let background = if x < panel_width {
            255
        } else if x < panel_width + PANEL_GAP {
            128
        } else {
            DARK
        };
        pixel.copy_from_slice(&[background, background, background, 255]);
    }
    Ok(image)
}

/// Reserves descent and a gap so adjacent baselines never collide.
fn row_height(device_size: u32) -> u32 {
    device_size.saturating_mul(3) / 2 + MARGIN
}

/// Samples all requested sizes while reusing prepared atlas handles.
/// Row order groups each DPR's sizes so adjacent lines show size changes.
pub(super) fn draw_rows(
    atlas: &SlugAtlas,
    glyphs: &BTreeMap<char, Prepared>,
    text: &str,
    sizes: &[u32],
    dprs: &[u32],
    image: &mut OracleImage,
) -> Result<usize, String> {
    let mut top = MARGIN;
    let mut ink = 0;
    for dpr in dprs {
        for size in sizes {
            let device_size = size.saturating_mul(*dpr);
            // The baseline is an integer device coordinate, which avoids
            // fractional shifts unrelated to the Slug coverage algorithm.
            let baseline = top.saturating_add(device_size);
            let mut left = MARGIN;
            for ch in text.chars() {
                // A space is represented by `None`: it advances the cursor
                // but should add no false coverage to the ink assertion.
                let (handle, advance) = glyphs
                    .get(&ch)
                    .ok_or_else(|| format!("missing prepared character {ch:?}"))?;
                if let Some(handle) = handle {
                    ink += draw_glyph(atlas, *handle, left, baseline, device_size, image)?;
                }
                left = left.saturating_add((advance * device_size as f32).round().max(1.0) as u32);
            }
            top = top.saturating_add(row_height(device_size));
        }
    }
    Ok(ink)
}

/// Converts a glyph's em bbox to a conservative device-pixel rectangle.
/// Sampling only this box keeps the sheet fast despite many repeated rows.
fn draw_glyph(
    atlas: &SlugAtlas,
    handle: ShapeHandle,
    left: u32,
    baseline: u32,
    size: u32,
    image: &mut OracleImage,
) -> Result<usize, String> {
    let scale = size as f32;
    let bbox = handle.bbox;
    // One pixel of dilation around the outline catches partially covered
    // samples even when its mathematical bbox falls between pixel centers.
    let x0 = (left as f32 + bbox[0] * scale).floor() as i32 - 1;
    let x1 = (left as f32 + bbox[2] * scale).ceil() as i32 + 1;
    let y0 = (baseline as f32 - bbox[3] * scale).floor() as i32 - 1;
    let y1 = (baseline as f32 - bbox[1] * scale).ceil() as i32 + 1;
    let panel_width = (image.width - PANEL_GAP) / 2;
    // The second color treatment is painted from the same byte, so it
    // cannot disagree because of a separate coverage computation.
    let mut ink = 0;
    for y in y0.max(0)..y1.min(image.height as i32) {
        for x in x0.max(0)..x1.min(panel_width as i32) {
            let at = [
                (x as f32 + 0.5 - left as f32) / scale,
                (baseline as f32 - y as f32 - 0.5) / scale,
            ];
            // Analytic inverse scale gives the reference its physical
            // half-pixel window in em units at this device size.
            let coverage =
                sample(atlas, handle, at, [1.0 / scale; 2]).map_err(|error| error.to_string())?;
            let byte = (coverage * 255.0).round().clamp(0.0, 255.0) as u8;
            if byte > 0 {
                paint_pair(image, x as u32, y as u32, panel_width, byte);
                ink += 1;
            }
        }
    }
    Ok(ink)
}

/// Applies one oracle coverage value to both panel colors at the same pixel.
fn paint_pair(image: &mut OracleImage, x: u32, y: u32, panel_width: u32, ink: u8) {
    let light = (y as usize * image.width as usize + x as usize) * 4;
    let dark_x = x + panel_width + PANEL_GAP;
    let dark = (y as usize * image.width as usize + dark_x as usize) * 4;
    // Both treatments use ordinary alpha-over in byte space, matching the
    // gamma-space blending selected for terminal text in design §9.7.
    let on_white = 255 - ink;
    let on_dark = DARK + ((u16::from(ink) * u16::from(255 - DARK)) / 255) as u8;
    image.pixels[light..light + 3].fill(on_white);
    image.pixels[dark..dark + 3].fill(on_dark);
}
