//! Natural annotation bounds, layout policy and common layer opacity.
//!
//! Measurement uses all layer corners before an annotation affine is applied.
//! Logical units are exposed to providers; shared glyph emitters keep device units.
//! Paint opacity preserves dynamic foreground stops for selection and cursors.
//! The ink union differs from natural width: whitespace advances without drawing.
//! Empty ink consequently reports zero bounds without erasing the text advance.
//! All color layers share placement fitting but may have different local bounds.
//! Their transformed corners must all participate in provider measurement.
use crate::frame_builder::CachedRow;
use puzzle_overlay::{draw::TextSpec, geometry::Rect};
use puzzle_text::{font_set::FontStyle, hinting::HintingSetting, layout::LayoutOptions};
/// Measures all vector-layer corners and bitmap crops, without reading terminal rows.
/// The union includes every color layer, including layers outside the base outline.
/// Matrix-mapped corners provide conservative bounds under italic and fallback fit.
/// Raster crops already contain the source's fitted baseline-relative offset.
/// Empty ink reports an empty rectangle while natural advance remains measurable.
/// DPR conversion happens once here so provider panel sizing stays logical.
pub(super) fn bounds(row: &CachedRow, dpr: f32) -> Rect {
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    // Accumulate in device coordinates before the single DPR conversion below.
    let mut extend = |p: [f32; 2]| {
        for axis in 0..2 {
            lo[axis] = lo[axis].min(p[axis]);
            hi[axis] = hi[axis].max(p[axis]);
        }
    };
    for draw in &row.glyphs {
        // An affine maps each source bounding-box corner independently.
        for x in [draw.bbox[0], draw.bbox[2]] {
            for y in [draw.bbox[1], draw.bbox[3]] {
                extend([
                    draw.m[0] * x + draw.m[2] * y + draw.t[0],
                    draw.m[1] * x + draw.m[3] * y + draw.t[1],
                ]);
            }
        }
    }
    for draw in &row.rasters {
        // Cropped bitmap bounds already include baseline-relative source fitting.
        extend([draw.rect[0], draw.rect[1]]);
        extend([draw.rect[0] + draw.rect[2], draw.rect[1] + draw.rect[3]]);
    }
    if !lo[0].is_finite() {
        // A whitespace-only annotation has advance but no visible ink union.
        return Rect::default();
    }
    Rect {
        origin: lo.map(|v| v / dpr),
        size: [(hi[0] - lo[0]) / dpr, (hi[1] - lo[1]) / dpr],
    }
}

/// Free-text settings select unhinted natural layout independently of the grid size.
/// Style IDs select the same primary and synthetic faces as ordinary terminal ink.
/// Natural annotations have no cursor-break boundary or terminal symbol overflow.
/// Outline hinting stays disabled even when the final transform is identity.
/// The returned options borrow no provider text and contain no platform handles.
/// Unknown style IDs are rejected rather than silently mapped to regular.
pub(super) fn options(spec: &TextSpec, dpr: f32) -> Result<LayoutOptions<'static>, String> {
    let style = match spec.style {
        0 => FontStyle::Regular,
        1 => FontStyle::Bold,
        2 => FontStyle::Italic,
        3 => FontStyle::BoldItalic,
        _ => return Err("free-text style must be 0..3".into()),
    };
    Ok(LayoutOptions {
        size: spec.size,
        dpr,
        origin_px: [0; 2],
        style,
        // Free text may transform after layout, so ppem-specific shapes are unsuitable.
        hinting: HintingSetting::None,
        ligatures: true,
        features: &[],
        cursor_byte: None,
        transformed: true,
        symbol_overflow: false,
        builtin_glyphs: false,
        // Apply whole-annotation alpha after emission, including palette color layers.
        color: spec.color | 0xff000000,
    })
}

/// Multiplies whole-annotation alpha into every solid and authored color layer once.
/// Paint variants multiply authored colors; instance alpha handles foreground stops.
/// Reusable rows keep their original geometric and clip coordinates.
pub(super) fn alpha(
    backend: &mut puzzle_text::glyph::slug::SlugBackend,
    row: &mut CachedRow,
    color: u32,
) -> Result<(), String> {
    let opacity = (color >> 24) as f32 / 255.0;
    if opacity == 1.0 {
        return Ok(());
    }
    for draw in &mut row.glyphs {
        // Palette alpha lives in the paint row; dynamic stop alpha lives in the instance.
        if draw.paint != 0 {
            draw.paint = backend.prepare_paint_effect(draw.paint, draw.color, None, opacity)?;
        }
        let alpha = ((draw.color >> 24) as f32 * opacity).round() as u32;
        draw.color = (draw.color & 0x00ffffff) | (alpha << 24);
    }
    Ok(())
}
