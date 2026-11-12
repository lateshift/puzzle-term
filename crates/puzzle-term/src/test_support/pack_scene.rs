//! First-frame ASCII using a prebuilt unhinted Slug pack (design §8.8).
//!
//! The platform fetches or reads the pack bytes. This module receives an
//! already validated directory and texel planes, places ASCII on the same
//! integer cell grid as the full font, and exposes ordinary Slug instances.
//! It never parses a TTF. The web shell can present useful text while the
//! full font is fetched for shaping and D-27's ppem-specific outlines.
//!
//! A pack is unhinted even on a 1× display. Its first-frame cell grid,
//! however, must use the *hinted* metric policy at low DPR: the later full
//! font does not cap-snap when its light hinter takes over. At high DPR,
//! both pack and full font may use the same bounded cap-height snap.
//! Each glyph's em-space curve data remains untouched; only its affine
//! size and integer cell translation depend on the target display.
use puzzle_frame::slug::SlugInstance;
use puzzle_slug::atlas::ShapeHandle;
use puzzle_text::metrics::{CellMetrics, CellOptions, MetricAdjustment};
use puzzle_text::pack::FontPack;

/// A drawable pack-only text line with its texture planes kept by the pack.
pub struct PackScene {
    /// Validated ready-to-upload curve and band texels.
    pub pack: FontPack,
    /// Glyph placements at the final integer device-pixel cell origins.
    pub instances: Vec<SlugInstance>,
    /// Device-pixel target width after the same rounding as the full font.
    pub width: u32,
    /// Device-pixel target height after the same rounding as the full font.
    pub height: u32,
}

/// Builds a fixed-cell line without opening a source font.
impl PackScene {
    /// Places an ASCII specimen from pack metrics and cmap entries.
    pub fn new(pack: FontPack, text: &str, size: f32, dpr: f32) -> Result<Self, String> {
        // A script or URL may request an extreme size; reject it before
        // multiplying pack metrics into a GPU surface dimension.
        if !size.is_finite() || !(1.0..=256.0).contains(&size) {
            return Err("pack scene size must be 1..=256".into());
        }
        // Pack and full face use one CellMetrics formula. Rounding a
        // logical cell and then scaling it would drift at fractional DPR.
        let metrics = CellMetrics::from_pack(
            pack.metrics,
            CellOptions {
                size,
                dpr,
                line_height: 1.0,
                width: MetricAdjustment::Px(0.0),
                height: MetricAdjustment::Px(0.0),
                baseline: MetricAdjustment::Px(0.0),
                snap_cap_height: true,
                hinted: dpr < 1.5,
            },
        )?;
        // The pack directory maps Unicode scalars, while the renderer
        // needs only visible glyph instances. Empty-space columns still
        // enter `column`, retaining their width in the following origin.
        let mut instances = Vec::new();
        for (column, ch) in text.chars().enumerate() {
            // The startup subset is ASCII/Latin-1. Other scalars wait for
            // the full font rather than being misread as a pack address.
            let glyph = pack
                .glyphs
                .get(&(ch as u32))
                .ok_or_else(|| format!("pack lacks U+{:04X}", ch as u32))?;
            let Some(handle) = glyph.handle else {
                // A space still consumes its column, but has no contour to
                // upload or draw. Its later glyph keeps the grid origin.
                continue;
            };
            let column = u32::try_from(column).map_err(|_| "pack line is too long")?;
            instances.push(pack_instance(handle, column, metrics)?);
        }
        // A bounded review margin prevents dilation from clipping the
        // last glyph and rejects hostile line lengths before GPU setup.
        let cells = u32::try_from(text.chars().count()).map_err(|_| "pack line too long")?;
        // Checked arithmetic precedes WebGPU's 8192-pixel scene bound;
        // a huge external sample never wraps into a plausible small width.
        let width = 32_u32
            .checked_add(
                cells
                    .checked_mul(metrics.width_px)
                    .ok_or("pack width overflow")?,
            )
            .filter(|value| *value <= 8192)
            .ok_or("pack target exceeds 8192 pixels")?;
        let height = 32_u32
            .checked_add(metrics.height_px)
            .filter(|value| *value <= 8192)
            .ok_or("pack target exceeds 8192 pixels")?;
        // The pack remains owned by the scene so texture planes and handle
        // addresses live through the platform's first draw submission.
        Ok(Self {
            pack,
            instances,
            width,
            height,
        })
    }
}

/// Converts one validated glyph handle to the renderer's instance layout.
fn pack_instance(
    handle: ShapeHandle,
    column: u32,
    metrics: CellMetrics,
) -> Result<SlugInstance, String> {
    // Integer column multiplication preserves fixed-cell placement even
    // when the font's individual glyph advance differs from zero's advance.
    let x = 16_u32
        .checked_add(
            column
                .checked_mul(metrics.width_px)
                .ok_or("pack x overflow")?,
        )
        .ok_or("pack x overflow")?;
    // Slug reads the exact texture location in the pack; only this affine
    // translation and scale change for a different terminal cell or ppem.
    Ok(SlugInstance {
        // Em space points upward, while the render target has downward y.
        m: [metrics.device_size, 0.0, 0.0, -metrics.device_size],
        t: [x as f32, (16 + metrics.baseline_px) as f32],
        bbox: handle.bbox,
        // The two 16-bit coordinates retain the packer's physical rows.
        glyph_loc: puzzle_slug::encoding::encode_glyph_loc(handle.glyph_loc),
        band_info: handle.band_info,
        color: 0xffff_ffff,
        paint: 0,
    })
}
