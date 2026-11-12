//! One-period Slug underline waves shared by cells of equal geometry (§10.5).
//!
//! A terminal underline belongs to a device-pixel cell, not to the font's
//! outline. Each period starts and ends on the same centre line and tangent,
//! so neighbouring cells meet without a visible break. `kurbo::stroke`
//! expands the centre curve into a fill before the existing Slug preparer
//! converts it to quadratics. The cached atlas key depends on pixel width,
//! height and thickness; color stays on the cheap instance, not the shape.
use super::{CachedRow, colors};
use kurbo::{Affine, BezPath, Stroke, StrokeOpts, stroke};
use puzzle_frame::SlugInstance;
use puzzle_slug::{ShapeKey, atlas::FillRule};
use puzzle_text::{glyph::slug::SlugBackend, metrics::CellMetrics};

/// Adds a row-relative curly underline instance for one terminal cell.
pub(super) fn add(
    backend: &mut SlugBackend,
    cache: &mut CachedRow,
    column: usize,
    ink: [u8; 3],
    metrics: CellMetrics,
) -> Result<(), String> {
    let width = metrics.width_px;
    let height = metrics.height_px;
    let thickness = metrics.underline_px;
    // The wave's shape changes with all three metrics, unlike a font glyph
    // whose em-space outline normally ignores the current pixel size.
    let key = wave_key(width, height, thickness);
    // SlugBackend assigns font glyphs from the low half of the key space;
    // the high bit keeps procedural underline shapes disjoint from them.
    let handle = match backend.atlas().get(key) {
        Some(handle) => handle,
        None => backend
            .atlas_mut()
            .prepare(key, &path(width, height, thickness), FillRule::NonZero, 0)
            .map_err(|error| format!("curly underline shape: {error}"))?,
    };
    let scale = height as f32;
    // Device-cell height acts as one em for this procedural shape, allowing
    // the same Slug vertex shader to render glyphs and decorations.
    cache.waves.push(SlugInstance {
        // The path's em y axis points up; this transform restores device
        // pixels below the row's top edge during frame assembly.
        m: [scale, 0.0, 0.0, -scale],
        t: [column as f32 * width as f32, 0.0],
        // The packed band header and bounds refer to the shared atlas
        // shape. Only t and color differ between neighbouring cell periods.
        bbox: handle.bbox,
        glyph_loc: puzzle_slug::encoding::encode_glyph_loc(handle.glyph_loc),
        band_info: handle.band_info,
        color: colors::rgba(ink),
        paint: 0,
    });
    Ok(())
}

/// Reserves a disjoint procedural namespace in the Slug atlas.
fn wave_key(width: u32, height: u32, thickness: u32) -> ShapeKey {
    // The frame builder rejects a surface above 8192 pixels, so each
    // geometry component fits the 16-bit field used in this shape key.
    // This direct encoding also avoids hashing or allocation when a cached
    // wave is requested during a changed-row rebuild.
    ShapeKey(
        (1_u64 << 63) | (u64::from(width) << 32) | (u64::from(height) << 16) | u64::from(thickness),
    )
}

/// Strokes a closed-tangent quarter-wave chain in local device pixels.
fn path(width: u32, height: u32, thickness: u32) -> BezPath {
    let w = f64::from(width);
    let h = f64::from(height);
    let t = f64::from(thickness);
    // Both ends meet at the centre line. The first and last control
    // vectors match, so each cell contributes exactly one smooth period.
    let center = h - 2.0 * t - 1.0;
    let amplitude = t.max(1.0);
    // Four quadratic arcs approximate one sine-like period. Their control
    // points preserve matching endpoint slopes at adjoining cell edges.
    let mut centerline = BezPath::new();
    centerline.move_to((0.0, center));
    centerline.quad_to(
        (w * 0.125, center - amplitude),
        (w * 0.25, center - amplitude),
    );
    centerline.quad_to((w * 0.375, center - amplitude), (w * 0.5, center));
    centerline.quad_to(
        (w * 0.625, center + amplitude),
        (w * 0.75, center + amplitude),
    );
    centerline.quad_to((w * 0.875, center + amplitude), (w, center));
    // kurbo owns the stroke expansion. Converting the filled path to em
    // space keeps Slug's ordinary size-independent outline math unchanged.
    // A small device-pixel tolerance keeps the fill smooth at 1× while
    // producing few enough quadratics for the per-cell Slug band lists.
    let mut filled = stroke(
        centerline.elements().iter().copied(),
        &Stroke::new(t),
        &StrokeOpts::default(),
        0.01,
    );
    filled.apply_affine(Affine::scale_non_uniform(1.0 / h, -1.0 / h));
    filled
}
