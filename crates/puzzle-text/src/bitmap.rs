//! Embedded sbix/CBDT emoji use the existing read-fonts tables via skrifa.
//!
//! This neutral module selects the nearest non-smaller strike, or the largest
//! available one. Only the desktop host knows PNG: it supplies a decoder and
//! receives recoverable errors for corrupt input. No codec enters the wasm.
//! Bitmap advances never enter layout; fitting uses the immutable cell span.
//! Stored bearings are converted to y-up strike pixels before cell fitting.
//! Sbix anchors the image bottom, whereas CBDT anchors its top. Skrifa already
//! resolves sbix duplicate glyph records and validates the table offsets.
use crate::{
    FontData,
    glyph::{BitmapKind, GlyphPlacement, RasterGlyph},
};
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    bitmap::{BitmapData, BitmapFormat, Origin},
};

/// Area integration prevents transparent RGB from bleeding into smaller emoji.
mod scale;
pub use scale::area_resize;

/// The decoder owns the returned samples, so no borrowed codec buffer escapes.
/// Width and height describe the embedded PNG, independently of its strike ppem.
/// The neutral side verifies the byte count even if a platform decoder forgets to.
/// Bounded, straight-alpha samples returned by a platform codec.
pub struct DecodedBitmap {
    /// Source columns before the cell fit.
    pub width: u32,
    /// Source rows before the cell fit.
    pub height: u32,
    /// Straight RGBA8 samples; transparent RGB is ignored during filtering.
    pub pixels: Vec<u8>,
}

/// PNG is a desktop-only implementation behind this Send-ready seam (R9).
pub trait PngDecoder: Send {
    /// Decodes bounded embedded bytes, rejecting invalid input without a panic.
    fn decode(&mut self, bytes: &[u8]) -> Result<DecodedBitmap, String>;
}

/// Strike geometry remains separate from samples: a font can position an image
/// partly below its baseline or include extra image padding around visible ink.
/// The fitter preserves those bearings while transforming the entire image box.
/// One decoded strike with a bottom-left origin in y-up strike pixels.
pub struct BitmapImage {
    /// Unfiltered source samples from sbix or CBDT.
    pub image: DecodedBitmap,
    /// Selected strike's pixels per em; never zero or nonfinite.
    pub ppem: f32,
    /// Source image's bottom-left offset from its glyph baseline.
    pub origin: [f32; 2],
}

/// Selects the smallest strike at least as large as requested, otherwise largest.
/// Strike directories need not be sorted and can contain duplicate sizes.
/// A tie keeps the earlier valid record. Requests above every strike use the
/// largest source, allowing enlargement without selecting a tiny arbitrary strike.
/// Invalid strike sizes are skipped rather than entering a divide by zero.
pub fn strike_index(sizes: &[f32], requested: f32) -> Option<usize> {
    if !requested.is_finite() || requested <= 0.0 {
        return None;
    }
    let valid = sizes
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, s)| s.is_finite() && *s > 0.0);
    // Prefer a larger source so downsampling retains details at fractional ppem.
    valid
        .clone()
        .filter(|(_, s)| *s >= requested)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .or_else(|| valid.max_by(|a, b| a.1.total_cmp(&b.1)))
        .map(|(i, _)| i)
}

/// Reads a bitmap glyph from an untrusted collection face through skrifa.
/// The face index selects a member of a TTC without copying its table bytes.
/// This function does not modify font-choice caches or terminal placements.
/// Unsupported bitmap formats return a report; missing glyph records return None.
/// Every allocation is bounded before the external decoder is invoked.
/// Absence means another source may draw it; malformed PNG is an error report.
pub fn load(
    data: &FontData,
    glyph: u32,
    size: f32,
    decoder: &mut dyn PngDecoder,
) -> Result<Option<BitmapImage>, String> {
    // Parsing borrows the retained collection; tables never escape this call.
    load_bytes(data.bytes(), data.index(), glyph, size, decoder)
}

/// Reads borrowed platform bytes without copying a large mapped collection.
/// The platform owns the mapping for this call; decoded samples are independent.
/// No discovery-library or foreign read-fonts type crosses this byte-only seam.
pub fn load_bytes(
    bytes: &[u8],
    index: u32,
    glyph: u32,
    size: f32,
    decoder: &mut dyn PngDecoder,
) -> Result<Option<BitmapImage>, String> {
    let font = FontRef::from_index(bytes, index).map_err(|e| e.to_string())?;
    let strikes = font.bitmap_strikes();
    // EBDT monochrome fonts are outside this color-emoji task.
    if !matches!(
        strikes.format(),
        Some(BitmapFormat::Sbix | BitmapFormat::Cbdt)
    ) {
        return Ok(None);
    }
    // Bound a malicious strike directory before collecting its device sizes.
    if strikes.len() > 256 {
        return Err("bitmap strike allowance exceeded".into());
    }
    // Missing strike records contribute zero and are ignored by strike_index.
    let sizes: Vec<_> = (0..strikes.len())
        .map(|i| strikes.get(i).map_or(0.0, |s| s.ppem()))
        .collect();
    // Request ppem is already in device pixels, so DPR is not applied again.
    let index = strike_index(&sizes, size).ok_or("no usable bitmap strike")?;
    let strike = strikes.get(index).ok_or("invalid bitmap strike")?;
    // A cmap entry alone does not guarantee this strike contains an image.
    let Some(bitmap) = strike.get(GlyphId::new(glyph)) else {
        return Ok(None);
    };
    // This seam decodes PNG only; non-PNG color data must try another font.
    let BitmapData::Png(bytes) = bitmap.data else {
        return Err("color strike is not PNG".into());
    };
    // Codec errors remain local to this candidate and are handled by first().
    let image = decoder.decode(bytes)?;
    // The font and codec must agree; contradictory dimensions are corrupt input.
    if image.width != bitmap.width || image.height != bitmap.height {
        return Err("bitmap font and PNG dimensions disagree".into());
    }
    validate(&image)?;
    metrics(font, bitmap, image, sizes[index]).map(Some)
}

/// Converts table bearings into one bottom-left strike-pixel origin.
/// Font-unit bearings are scaled once; CBDT’s top origin is shifted by image height.
fn metrics(
    font: FontRef<'_>,
    bitmap: skrifa::bitmap::BitmapGlyph<'_>,
    image: DecodedBitmap,
    ppem: f32,
) -> Result<BitmapImage, String> {
    // Unscaled default metrics provide the font-unit denominator, not device size.
    let upem = font
        .metrics(
            skrifa::instance::Size::unscaled(),
            skrifa::instance::LocationRef::default(),
        )
        .units_per_em;
    if upem == 0 {
        return Err("bitmap font has zero units per em".into());
    }
    // Outer bearings use font units; inner bearings use this strike's pixels.
    let x = bitmap.bearing_x * ppem / f32::from(upem) + bitmap.inner_bearing_x;
    let mut y = bitmap.bearing_y * ppem / f32::from(upem) + bitmap.inner_bearing_y;
    // Converting a top-left anchor gives the same y-up bounds used by sbix.
    if bitmap.placement_origin == Origin::TopLeft {
        y -= image.height as f32;
    }
    Ok(BitmapImage {
        image,
        ppem,
        origin: [x, y],
    })
}

/// The shared fit chooses a uniform scale and centers the emoji in its cell inset.
/// Rounded dimensions become the exact atlas quad size; layout advances remain
/// immutable. Filtering integrates all source samples into this destination.
/// Returned offsets are relative to the glyph baseline, with screen y downward.
/// Fits color samples with premultiplied area integration, keeping D-31 cells.
pub fn fitted(
    source: BitmapImage,
    placement: &GlyphPlacement,
    baseline: f32,
) -> Result<RasterGlyph, String> {
    validate(&source.image)?;
    if !source.ppem.is_finite()
        || source.ppem <= 0.0
        || !placement.size_px.is_finite()
        || placement.size_px <= 0.0
    {
        return Err("invalid bitmap device size".into());
    }
    // Strike pixels convert to device pixels before fitting the terminal cell.
    let ratio = placement.size_px / source.ppem;
    let [x, y] = source.origin;
    let top = y + source.image.height as f32;
    // Bounds are y-up device pixels; fitting centers emoji inside the row inset.
    let bounds = [
        x * ratio,
        y * ratio,
        (x + source.image.width as f32) * ratio,
        top * ratio,
    ];
    let fit = crate::fitting::fit(placement, bounds, true, baseline);
    // Fold the uniform cell fit into the original strike-to-device scale.
    let ratio = ratio * fit.scale;
    let width = (source.image.width as f32 * ratio).round().max(1.0) as u32;
    let height = (source.image.height as f32 * ratio).round().max(1.0) as u32;
    // Leave the raster shelf's border allowance available after fitting.
    if width > 2046 || height > 2046 || !source.origin.iter().all(|x| x.is_finite()) {
        return Err("fitted bitmap exceeds raster page".into());
    }
    // The GPU samples the resized image directly, avoiding nearest-neighbor reduction.
    let pixels = area_resize(&source.image, width, height)?;
    Ok(RasterGlyph {
        // Only ink moves; the terminal retains its original cell origin and advance.
        offset_px: [
            (x * ratio + fit.offset[0]).round() as i32,
            (-top * ratio + fit.offset[1]).round() as i32,
        ],
        width,
        height,
        pixels,
        kind: BitmapKind::Color,
    })
}

/// Dimensions are capped at one atlas page before multiplication or indexing.
/// Checked multiplication prevents malformed headers from wrapping the byte count.
/// An exact length prevents short source slices and silently ignored trailing data.
/// Bounds every byte product before the source can reach a filter or atlas.
pub(super) fn validate(image: &DecodedBitmap) -> Result<(), String> {
    let bytes = image
        .width
        .checked_mul(image.height)
        .and_then(|n| n.checked_mul(4));
    if image.width == 0
        || image.height == 0
        || image.width > 2048
        || image.height > 2048
        || bytes.map(|n| n as usize) != Some(image.pixels.len())
    {
        return Err("invalid color bitmap dimensions or byte count".into());
    }
    Ok(())
}

/// Tries ordered bitmap candidates; a broken font is warned once and skipped.
/// Candidates are already ordered by the platform’s family/fallback policy.
/// Unsupported glyphs and decoding failures do not poison later candidates.
/// Warnings identify retained font bytes plus their TTC face index, not each glyph.
/// The bounded warning set avoids per-glyph log floods from one broken font.
/// The caller retains warning identities across requests, but not decoded images.
pub fn first(
    candidates: &[(FontData, u32)],
    placement: &GlyphPlacement,
    baseline: f32,
    decoder: &mut dyn PngDecoder,
    warned: &mut std::collections::HashSet<(usize, u32)>,
) -> Option<RasterGlyph> {
    for (data, glyph) in candidates {
        // All bytes are parsed by our aligned skrifa, even when discovery uses another version.
        let result = load(data, *glyph, placement.size_px, decoder).and_then(|image| {
            image
                .map(|image| fitted(image, placement, baseline))
                .transpose()
        });
        match result {
            Ok(Some(bitmap)) => return Some(bitmap),
            Ok(None) => {}
            Err(error) => {
                let identity = (data.bytes().as_ptr() as usize, data.index());
                // Retain a bounded per-font warning identity across cold requests.
                if warned.len() < 256 && warned.insert(identity) {
                    tracing::warn!(event = "font.bitmap_failed", %error, "trying next bitmap source");
                }
            }
        }
    }
    None
}

#[cfg(test)]
/// Strike choice, table parsing and alpha-correct filter contracts.
mod tests;
