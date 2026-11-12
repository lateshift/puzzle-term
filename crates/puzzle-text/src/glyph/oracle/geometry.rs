//! Device crop and inverse affine sampling match the vector backend's ink fit.
//!
//! Bounds arrive in font em units with y upward. Raster crops use physical pixels
//! with y downward and are measured relative to the neutral placement baseline.
//! A shared InkFit supplies uniform scale and translation, preserving cell spans.
//! Synthetic italic adds an x shear; synthetic bold adds a second integer-offset
//! draw. Both affect the crop, so their overhang cannot be silently clipped.
//! Sampling inverts that complete map at physical pixel centers, including the
//! crop's floor-rounded offset. The returned derivative replaces shader fwidth
//! analytically: horizontal em coordinate varies along both device axes under shear.
//! One antialiasing pixel around each bound allows the oracle's edge coverage.
//! RasterAtlas adds its own transparent gutter outside this returned sample crop.
//! The 2046-pixel limit leaves that gutter inside the existing 2048-pixel page.
//! An oversized or malformed request returns an error before allocating pixels.
use crate::glyph::GlyphPlacement;

/// One bounded crop, keeping pixel geometry separate from coverage evaluation.
pub(crate) struct SampleGrid {
    /// Crop top-left relative to the placement baseline.
    pub(crate) offset: [i32; 2],
    /// Horizontal device-pixel count, excluding atlas borders.
    pub(crate) width: u32,
    /// Vertical device-pixel count, excluding atlas borders.
    pub(crate) height: u32,
    /// Fitted em→device scale used by the inverse transform.
    pub(crate) size: f32,
    /// Additional bold draw's device offset.
    pub(crate) bold: f32,
    /// Analytic em-coordinate fwidth, including synthetic italic shear.
    pub(crate) derivative: [f32; 2],
    /// Dimensionless italic slope in the em→device matrix.
    shear: f32,
    /// Fitting translation relative to the baseline origin.
    pub(crate) translation: [f32; 2],
}

/// Computes geometry once per overflow glyph, never once per pixel.
impl SampleGrid {
    /// Applies the same neutral-placement fitting as the vector emitter (§8.4).
    pub(crate) fn new(
        p: &GlyphPlacement,
        bbox: [f32; 4],
        fitting: bool,
        italic: bool,
        bold: bool,
    ) -> Result<Self, String> {
        // Procedural boxes already match their cells; per-part fitting would distort them.
        let fit = if fitting && p.key.face < crate::builtin::TOFU_FACE {
            crate::fitting::fit(p, bbox.map(|v| v * p.size_px), p.color_glyph, p.baseline_px)
        } else {
            crate::fitting::InkFit {
                scale: 1.0,
                offset: [0.0; 2],
            }
        };
        let size = p.size_px * fit.scale;
        // An inverse matrix is meaningful only for finite positive device geometry.
        if !size.is_finite() || size <= 0.0 {
            return Err("invalid oracle device size".into());
        }
        // Twelve degrees is the same missing-italic policy used by the vector emitter.
        let shear = if italic {
            12.0_f32.to_radians().tan()
        } else {
            0.0
        };
        // Missing weight uses a second draw, not an expansion of the outline itself.
        let bold = if bold {
            (p.size_px / 24.0).round().max(1.0)
        } else {
            0.0
        };
        let (offset, width, height) = crop(bbox, size, shear, fit.offset, bold)?;
        // Store the inverse's factors once, avoiding fitting work inside the pixel loop.
        Ok(Self {
            offset,
            width,
            height,
            size,
            bold,
            shear,
            translation: fit.offset,
            derivative: [(1.0 + shear.abs()) / size, 1.0 / size],
        })
    }

    /// Native grid crops omit the extra oracle margin outside the rounded ink bounds.
    pub(crate) fn grid(
        p: &GlyphPlacement,
        bbox: [f32; 4],
        style: [bool; 3],
    ) -> Result<Self, String> {
        let mut grid = Self::new(p, bbox, style[0], style[1], style[2])?;
        // Pixels beyond floor(min)..ceil(max) have no intersection with the shape.
        // RasterAtlas retains its own transparent gutter for interpolation and neighbors.
        // Moving the origin inward preserves baseline-relative placement of every retained texel.
        grid.offset = grid.offset.map(|v| v + 1);
        grid.width = grid.width.saturating_sub(2).max(1);
        grid.height = grid.height.saturating_sub(2).max(1);
        Ok(grid)
    }

    /// Inverts the em→device affine at the center of a cropped pixel.
    pub(crate) fn at(&self, x: u32, y: u32) -> [f32; 2] {
        // Translation is undone before scale; crop-local indices become absolute device offsets.
        let dx = (self.offset[0] as f32 + x as f32 + 0.5 - self.translation[0]) / self.size;
        let dy = (self.offset[1] as f32 + y as f32 + 0.5 - self.translation[1]) / self.size;
        // Undo y inversion and the forward italic shear in their coupled coordinates.
        [dx + self.shear * dy, -dy]
    }
}

/// Includes synthetic italic overhang, bold offset and one anti-aliasing border.
fn crop(
    bbox: [f32; 4],
    size: f32,
    shear: f32,
    translation: [f32; 2],
    bold: f32,
) -> Result<([i32; 2], u32, u32), String> {
    // Affine extrema occur at transformed corners, including negative descenders.
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for x in [bbox[0], bbox[2]] {
        for y in [bbox[1], bbox[3]] {
            let corner = [
                // The fitting offset belongs to device space, after font scaling.
                (x + shear * y) * size + translation[0],
                -y * size + translation[1],
            ];
            for i in 0..2 {
                bounds[i] = bounds[i].min(corner[i]);
                bounds[i + 2] = bounds[i + 2].max(corner[i]);
            }
        }
    }
    // Crop limits match the shelf atlas's two transparent border pixels (§10.3).
    let offset = [
        (bounds[0] - 1.0).floor() as i32,
        (bounds[1] - 1.0).floor() as i32,
    ];
    let width = (bounds[2] + bold + 1.0).ceil() as i64 - i64::from(offset[0]);
    // Wider arithmetic prevents an externally supplied coordinate from wrapping a crop.
    let height = (bounds[3] + 1.0).ceil() as i64 - i64::from(offset[1]);
    if !(1..=2046).contains(&width) || !(1..=2046).contains(&height) {
        // The existing page size is unchanged; this helper never grows an oversized atlas.
        return Err("oracle crop exceeds raster page".into());
    }
    Ok((offset, width as u32, height as u32))
}
