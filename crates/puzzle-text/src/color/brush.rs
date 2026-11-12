//! Skrifa brushes converted to renderer-neutral em geometry and palette colors.
//!
//! Foreground index 0xffff is the only reference to terminal text color.
//! CPAL colors remain sRGB and premultiply before stop interpolation (COLR).
//! Skrifa has already normalized each gradient's color line and its geometry:
//! re-normalizing stop offsets here would change reflected/repeated intervals.
//! Linear gradients arrive as two effective points, accounting for the third
//! OpenType point's projection. Radial circles may have negative radii after
//! normalization; the sampler rejects the negative-radius portion of the cone.
//! Sweep angles stay in degrees, including spans outside the visible 360° turn.
//! The affine inverse maps root em samples back to brush-local em coordinates.
//! Keeping both spaces in em units permits one record to serve every DPR.
//!
//! A brush color and its clipping geometry are independent. In a PaintGlyph
//! wrapping a PaintTransform, the clip is captured before that transform;
//! only sampling coordinates are inverted here. In a PaintTransform wrapping
//! a PaintGlyph, both move together because the painter captured the earlier
//! parent transform on the path. This distinction keeps rotated gradients
//! inside the intended glyph instead of rotating their clipping outline.
//!
//! Palette alpha and paint alpha are separate quantities. Their product is
//! the color-line alpha. Multiplying RGB by that product before interpolation
//! prevents transparent stops from creating dark fringes or hue changes.
//! The shader unpremultiplies only to meet the preexisting Slug paint seam;
//! its final blend output immediately premultiplies by coverage and alpha.
//! No gamma or linear-light conversion belongs to the color-line operation.
//! Foreground flags preserve the sentinel's identity after CPU resolution:
//! palette colors stay authored while cursor/selection can retint foreground.
//! Unknown palette indices fail rather than borrowing the final CPAL entry.
use kurbo::Affine;
use puzzle_slug::paint::{Brush as PaintBrush, Extend, Paint, Stop};
use skrifa::{
    FontRef, MetadataProvider,
    color::{Brush, ColorStop, Extend as FontExtend},
    raw::types::Point,
};
/// Converts one brush, rejecting unsupported stop counts before table upload.
pub(super) fn convert(
    font: &FontRef<'_>,
    brush: Brush<'_>,
    foreground: u32,
    transform: Affine,
    upem: f64,
) -> Result<(Paint, bool), String> {
    // An inverse map exists only for a finite, nonsingular brush transform.
    if !transform.determinant().is_finite() || transform.determinant().abs() < 1e-12 {
        return Err("singular COLR brush transform".into());
    }
    // Default empty buffers are populated only after the brush is validated.
    // Enclosing clips are attached later, when paths have atlas handles.
    let mut paint = Paint {
        brush: PaintBrush::Solid([0.0; 4]),
        foreground: false,
        // Inversion changes the sampling basis, never the terminal cell origin.
        inverse: transform.inverse().as_coeffs().map(|v| v as f32),
        extend: Extend::Pad,
        stops: Vec::new(),
        clips: Vec::new(),
    };
    // Solid brushes have no color line and must not accidentally inherit stops.
    if let Brush::Solid {
        palette_index,
        alpha,
    } = brush
    {
        // A solid foreground sentinel also participates in cursor tinting.
        paint.foreground = palette_index == 0xffff;
        paint.brush = PaintBrush::Solid(color(font, palette_index, alpha, foreground)?);
        return Ok((paint, palette_index == 0xffff));
    }
    let (geometry, stops, extend) = geometry(brush, upem)?;
    // Keep the geometry paired with the same normalized stop array.
    paint.brush = geometry;
    // This is the fixed shader record's capacity, not a truncation policy.
    if stops.len() > 16 {
        return Err("COLR gradient exceeds sixteen stops".into());
    }
    paint.extend = match extend {
        // Extension is relative to the first/last stop, not the glyph bounds.
        FontExtend::Pad => Extend::Pad,
        FontExtend::Repeat => Extend::Repeat,
        FontExtend::Reflect => Extend::Reflect,
        _ => return Err("unknown COLR extend mode".into()),
    };
    // A mixed gradient may contain both palette and foreground stops.
    let mut uses_foreground = false;
    for stop in stops {
        // Preserve a sentinel flag alongside the resolved CPU color: selection
        // and cursor tint can change foreground stops without touching CPAL.
        let is_foreground = stop.palette_index == 0xffff;
        uses_foreground |= is_foreground;
        paint.stops.push(Stop {
            offset: stop.offset,
            foreground: is_foreground,
            color: color(font, stop.palette_index, stop.alpha, foreground)?,
        });
    }
    // Report whether recoloring can affect any part of this layer.
    Ok((paint, uses_foreground))
}
/// Keeps skrifa's normalized geometry while changing font units to em units.
fn geometry(brush: Brush<'_>, upem: f64) -> Result<(PaintBrush, &[ColorStop], FontExtend), String> {
    match brush {
        Brush::LinearGradient {
            p0,
            p1,
            color_stops,
            extend,
        } => {
            // The effective vector is preserved; no font advance enters it.
            Ok((
                PaintBrush::Linear {
                    p0: point(p0, upem),
                    p1: point(p1, upem),
                },
                color_stops,
                extend,
            ))
        }
        Brush::RadialGradient {
            c0,
            r0,
            c1,
            r1,
            color_stops,
            extend,
        } => {
            // Centers and radii need identical scaling to preserve the cone.
            Ok((
                PaintBrush::Radial {
                    c0: point(c0, upem),
                    c1: point(c1, upem),
                    radii: [r0 / upem as f32, r1 / upem as f32],
                },
                color_stops,
                extend,
            ))
        }
        Brush::SweepGradient {
            c0,
            start_angle,
            end_angle,
            color_stops,
            extend,
        } => {
            // Angles are unitless and therefore unaffected by UPEM scaling.
            Ok((
                PaintBrush::Sweep {
                    center: point(c0, upem),
                    angles: [start_angle, end_angle],
                },
                color_stops,
                extend,
            ))
        }
        Brush::Solid { .. } => Err("solid brush has no gradient geometry".into()),
    }
}
/// Normalizes a font-unit point, retaining the font's y-up orientation.
fn point(p: Point<f32>, upem: f64) -> [f32; 2] {
    [p.x / upem as f32, p.y / upem as f32]
}
/// Resolves a CPAL entry and combines its alpha with the paint's alpha.
fn color(font: &FontRef<'_>, index: u16, alpha: f32, foreground: u32) -> Result<[f32; 4], String> {
    let rgba = if index == 0xffff {
        // Packed terminal colors use RGBA byte order independently of the CPU.
        foreground.to_le_bytes()
    } else {
        // COLR references CPAL palette zero unless a later explicit palette
        // selection API exists. Missing indices reject the whole cold glyph.
        let palettes = font.color_palettes();
        let palette = palettes.get(0).ok_or("COLR font has no palette")?;
        let c = palette
            .colors()
            .get(usize::from(index))
            .ok_or("COLR palette index is outside CPAL")?;
        // CPAL stores BGRA, but typed accessors remove that storage detail.
        [c.red(), c.green(), c.blue(), c.alpha()]
    };
    // Paint alpha multiplies CPAL alpha before premultiplied interpolation.
    let a = (f32::from(rgba[3]) / 255.0 * alpha).clamp(0.0, 1.0);
    Ok([
        f32::from(rgba[0]) / 255.0 * a,
        f32::from(rgba[1]) / 255.0 * a,
        f32::from(rgba[2]) / 255.0 * a,
        a,
    ])
}
