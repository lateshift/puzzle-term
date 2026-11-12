//! Fixed row layout for Slug paint records (design §9.5).
//!
//! Header tags are negative, distinguishing records from the positive RGBA
//! legacy binding probe. Stops and clip handles never cross a texture row.
//!
//! Texel 0 stores type, extension, stop count and extra clip count. Texel 1
//! stores the inverse matrix's linear terms, and texel 2 its translation.
//! Texels 3–4 store gradient geometry; texel 5 stores a constant solid fill.
//! Texel 6 identifies the solid foreground sentinel. Stops start at texel 8,
//! with an offset/sentinel texel and a premultiplied RGBA texel per stop.
//! The sixteen-stop maximum ends at texel 39. Extra clip records begin at
//! texel 40, each storing bbox and packed glyph/band metadata in two texels.
//! The remaining reserved texels keep this row layout extensible without
//! moving records or changing the 56-byte Slug instance contract.
//! Zero-paint solids never read this texture; row zero remains for the M3
//! legacy positive-RGBA binding probe. New records start at a later row.
use super::{Brush, Extend, Paint};
/// Encodes one brush with at most sixteen stops and sixteen outer clips.
pub(super) fn encode(paint: &Paint) -> Result<[[f32; 4]; 256], String> {
    // Validate before allocating a row address or issuing a GPU write.
    validate(paint)?;
    // Reserved texels are deterministic zeros, including unoccupied stop slots.
    let mut row = [[0.0; 4]; 256];
    // Negative exact integers distinguish brush records from legacy colors.
    let kind = match paint.brush {
        Brush::Solid(_) => -1.0,
        Brush::Linear { .. } => -2.0,
        Brush::Radial { .. } => -3.0,
        Brush::Sweep { .. } => -4.0,
    };
    // These exact small integers are shared with the shader extension switch.
    let extend = match paint.extend {
        Extend::Pad => 0.0,
        Extend::Repeat => 1.0,
        Extend::Reflect => 2.0,
    };
    // Counts are bounded before converting them to exactly representable f32 values.
    row[0] = [
        kind,
        extend,
        paint.stops.len() as f32,
        paint.clips.len() as f32,
    ];
    // Affine storage is column-major, matching the shader root-to-local map.
    row[1].copy_from_slice(&paint.inverse[..4]);
    // Translation has its own slot so the shader can use the linear terms directly.
    row[2][..2].copy_from_slice(&paint.inverse[4..]);
    // Geometry has separate slots so every brush shares the same stop layout.
    match paint.brush {
        Brush::Solid(color) => row[5] = color,
        Brush::Linear { p0, p1 } => row[3] = [p0[0], p0[1], p1[0], p1[1]],
        Brush::Radial { c0, c1, radii } => {
            row[3] = [c0[0], c0[1], c1[0], c1[1]];
            row[4][..2].copy_from_slice(&radii);
        }
        Brush::Sweep { center, angles } => {
            row[3][..2].copy_from_slice(&center);
            row[4][..2].copy_from_slice(&angles);
        }
    }
    // Sentinel identity survives palette resolution for selection/cursor tint.
    row[6][0] = f32::from(paint.foreground);
    // Two fixed texels per stop make a bounded shader search independent of length.
    for (i, stop) in paint.stops.iter().enumerate() {
        row[8 + i * 2][0] = stop.offset;
        row[8 + i * 2][1] = f32::from(stop.foreground);
        row[9 + i * 2] = stop.color;
    }
    for (i, clip) in paint.clips.iter().enumerate() {
        // Band flags are bitcast, rather than numerically rounded to f32.
        row[40 + i * 2] = clip.bbox;
        row[41 + i * 2] = [
            f32::from(clip.glyph_loc[0]),
            f32::from(clip.glyph_loc[1]),
            f32::from_bits(clip.band_info),
            0.0,
        ];
    }
    // Clip band flags deliberately use bit patterns; validate numeric brush slots only.
    if row[..40].iter().flatten().any(|v| !v.is_finite()) {
        return Err("nonfinite paint geometry".into());
    }
    Ok(row)
}

/// Rejects unrepresentable records before they can consume atlas addresses.
fn validate(paint: &Paint) -> Result<(), String> {
    // Exceeding a representable record is an error rather than dropped color stops.
    if paint.stops.len() > 16 || paint.clips.len() > 16 {
        return Err("paint exceeds sixteen stops or enclosing clips".into());
    }
    if paint
        .inverse
        .iter()
        .chain(
            paint
                .stops
                .iter()
                .flat_map(|s| s.color.iter().chain([&s.offset])),
        )
        .any(|v| !v.is_finite())
    {
        return Err("nonfinite paint transform or stop".into());
    }
    Ok(())
}
