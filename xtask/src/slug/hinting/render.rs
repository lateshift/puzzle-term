//! Oracle pixels and evidence crops for the D-27 comparison.
//!
//! Hinting changes only the outline handed to the existing Slug atlas.
//! The reference sampler computes coverage unchanged; the requested blend
//! variants act on that scalar afterward. Integer origins and baselines
//! isolate coverage differences from accidental subpixel placement.
/// Isolates the output-image magnification from oracle sampling.
mod crop;
/// Samples packed curves and composites one glyph on both panels.
mod glyph;
use super::variant::Blend;
pub(super) use crop::crop_threefold;
use kurbo::BezPath;
use puzzle_slug::{
    ShapeKey, SlugAtlas,
    atlas::{FillRule, ShapeHandle},
    reference::OracleImage,
};
use std::collections::BTreeMap;

/// One row leaves enough vertical room for a 14 px capital and its descent.
pub(super) const ROW_HEIGHT: u32 = 32;
/// The text begins after labels that can state why exact snapping was skipped.
pub(super) const TEXT_LEFT: u32 = 280;
/// Separates the white and dark panels visibly.
const PANEL_GAP: u32 = 24;
/// D-27's dark panel has the specified #1e1e1e background.
pub(super) const DARK: u8 = 30;

/// Prepared text includes an advance even when a space has no outline.
type Glyphs = BTreeMap<char, (Option<ShapeHandle>, f32)>;

/// Integer device-pixel origin for one text run.
#[derive(Clone, Copy)]
pub(super) struct Position {
    /// Left edge of the first glyph's em square.
    pub(super) left: u32,
    /// Baseline shared by every glyph and both color panels.
    pub(super) baseline: u32,
}

/// Raster settings that stay constant across one text run.
#[derive(Clone, Copy)]
pub(super) struct Style {
    /// Device pixels per em, after optional cap-height snapping.
    pub(super) size: f32,
    /// Coverage-to-color operation selected by the named variant.
    pub(super) blend: Blend,
    /// Sets the reference's W bit for optical sqrt coverage.
    pub(super) weighted: bool,
    /// Keeps the two treatments at matching coordinates.
    pub(super) panel_width: u32,
}

/// Allocates a two-panel sheet with opaque, known background bytes.
pub(super) fn blank(panel_width: u32, rows: u32) -> Result<OracleImage, String> {
    // Each variant has paired backgrounds so changes in edge intensity can
    // be compared at the same coordinates on light and dark surfaces.
    let width = panel_width.saturating_mul(2).saturating_add(PANEL_GAP);
    let height = rows.saturating_mul(ROW_HEIGHT);
    // A finite sheet bound keeps a mistyped size list from allocating an
    // enormous PNG before the reviewer can inspect the generated layout.
    if width == 0 || width > 4096 || height == 0 || height > 4096 {
        return Err("D-27 image exceeds 4096 pixels on an axis".into());
    }
    let mut pixels = vec![0; width as usize * height as usize * 4];
    // Initialize the entire sheet, including the separator, before drawing
    // text. Transparent gaps would confuse ink checks and image viewers.
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        let x = index as u32 % width;
        // Gray is reserved for the gutter; it makes the two comparison
        // backgrounds unambiguous even when a row has mostly whitespace.
        let bg = if x < panel_width {
            255
        } else if x < panel_width + PANEL_GAP {
            128
        } else {
            DARK
        };
        pixel.copy_from_slice(&[bg, bg, bg, 255]);
    }
    Ok(OracleImage {
        width,
        height,
        pixels,
    })
}

/// Converts one row's real outlines through the unchanged Slug packer.
pub(super) fn prepare(
    atlas: &mut SlugAtlas,
    text: &str,
    mut glyph: impl FnMut(char) -> Result<(BezPath, f32), String>,
) -> Result<Glyphs, String> {
    // A row repeats characters but only needs one packed outline per
    // character; the ordered map gives stable preparation across runs.
    let mut result = BTreeMap::new();
    for ch in text.chars() {
        if result.contains_key(&ch) {
            continue;
        }
        let (path, advance) = glyph(ch)?;
        // Spaces keep an advance but have no curves and therefore must not
        // enter the atlas. Their absence is expected, not missing ink.
        let handle = if path.is_empty() {
            None
        } else {
            // A new atlas per row keeps size-specific hinted curves apart.
            Some(
                atlas
                    .prepare(ShapeKey(ch as u64), &path, FillRule::NonZero, 0)
                    .map_err(|error| error.to_string())?,
            )
        };
        result.insert(ch, (handle, advance));
    }
    Ok(result)
}

/// Renders an oracle label or specimen text on both color panels.
pub(super) fn draw_text(
    atlas: &SlugAtlas,
    glyphs: &Glyphs,
    text: &str,
    position: Position,
    style: Style,
    image: &mut OracleImage,
) -> Result<usize, String> {
    // Count sampled ink for a mechanical blank-row check. This is coverage,
    // independent of whether a blend formula yields a faint edge pixel.
    let mut x = position.left;
    let mut ink = 0;
    for ch in text.chars() {
        // Missing entries indicate a preparation error; silently skipping
        // them would make a damaged comparison sheet look legitimate.
        let (handle, advance) = glyphs.get(&ch).ok_or("missing prepared comparison glyph")?;
        if let Some(handle) = handle {
            ink += glyph::draw_glyph(
                atlas,
                *handle,
                Position {
                    left: x,
                    ..position
                },
                style,
                image,
            )?;
        }
        // Every row rounds each em advance to an integer device origin;
        // hinting does not supply or modify the layout advance (D-28).
        x = x.saturating_add((advance * style.size).round().max(1.0) as u32);
    }
    Ok(ink)
}
