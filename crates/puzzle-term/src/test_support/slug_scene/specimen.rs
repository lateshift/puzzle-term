//! Real-font M3 specimen scenes spanning size, DPR and affine transforms.
//!
//! The full line uses the approved bundled face. D-27 light autohinting fits
//! its untransformed 1× grid specimens; DPR 2 and affine effects retain
//! original em-space curves. The platform supplies bytes; this neutral
//! module prepares outlines, and repeated characters share atlas handles.
//!
//! The line's union of transformed bboxes sizes the offscreen image. A
//! generous black margin leaves dynamic dilation and anti-aliasing visible
//! without clipping. The large 96 px DPR 2 line may exceed 4096 pixels in
//! width, so the target follows WebGPU's 8192-pixel default limit.
//!
//! Layout is intentionally narrow in M3: it maps each character to an
//! outline and its OpenType advance, without the HarfBuzz shaping
//! that M4 will add. This still exercises lowercase forms, counters, stems,
//! punctuation and repeated outlines through the real Slug atlas. The
//! renderer sees a single batch of instances, not one draw per character.
//! A space advances the pen but has no outline or instance; its empty area
//! remains an observable gap in the reference image.
//!
//! The matrix maps em coordinates, with y pointing up, into device pixels,
//! with y pointing down. Its columns are the transformed screen vectors for
//! one em right and one em up. The per-character pen advances along the
//! first column. A quarter turn exchanges the ray axes; a shear gives a
//! synthetic italic slant; zoom changes scale without repreparing curves.
//! Every baseline translation is rounded to device pixels before the
//! common margin is added, matching the grid origin rule in design §8.2.
//! The matrix itself stays fractional so the shader's inverse Jacobian and
//! dynamic dilation retain the true affine transform.
//!
//! Stable scenario labels encode size, DPR and treatment. Parsing rejects
//! values outside the agreed matrix rather than making an arbitrary huge
//! target from script input. The same labels name golden PNGs and oracle
//! diagnostics, so a failed case points to a reproducible configuration.
use super::{FontData, SlugScene, SlugSceneKind, SpecimenTreatment};
use puzzle_frame::slug::SlugInstance;
use puzzle_slug::{ShapeKey, SlugAtlas, atlas::FillRule};
use puzzle_text::probe::{HintedProbe, ProbeFont, ProbeHintMode};

/// The exact probe text required for M3 reference and oracle coverage.
pub(super) const LINE: &str = "Hamburgefonstiv 0Oo1lI {}[] -> => != ==";

/// Parses a stable scenario label into its typed size and transform.
pub(super) fn parse_name(value: &str) -> Option<SlugSceneKind> {
    // Prefix parsing keeps the scenario language small while preserving a
    // typed treatment enum in all geometry and oracle code after this edge.
    let tail = value.strip_prefix("specimen-")?;
    let (treatment, tail) = if let Some(rest) = tail.strip_prefix("rot15-") {
        (SpecimenTreatment::Rotate15, rest)
    } else if let Some(rest) = tail.strip_prefix("rot90-") {
        (SpecimenTreatment::Rotate90, rest)
    } else if let Some(rest) = tail.strip_prefix("shear12-") {
        (SpecimenTreatment::Shear12, rest)
    } else if let Some(rest) = tail.strip_prefix("zoom4-") {
        (SpecimenTreatment::Zoom4, rest)
    } else {
        (SpecimenTreatment::Normal, tail)
    };
    let (size, dpr) = tail.split_once('-')?;
    // The suffix is an explicit DPR, not part of the font size. Using the
    // whole suffix prevents a typo such as `2px` from selecting another case.
    let size_px = size.parse::<u16>().ok()?;
    let dpr = dpr.strip_suffix('x')?.parse::<u8>().ok()?;
    if ![6, 8, 10, 12, 14, 16, 24, 48, 96].contains(&size_px)
        || ![1, 2].contains(&dpr)
        || (treatment != SpecimenTreatment::Normal && size_px != 24)
    {
        return None;
    }
    Some(SlugSceneKind::Specimen {
        size_px,
        dpr,
        treatment,
    })
}

/// Builds one full line at a requested device size and affine treatment.
pub(super) fn build(
    size_px: u16,
    dpr: u8,
    treatment: SpecimenTreatment,
    font_data: Option<&FontData>,
) -> Result<SlugScene, String> {
    // A caller cannot substitute a system face or a collection member and
    // still compare against references made from this vendored TTF.
    let data = font_data.ok_or("specimen scene needs font bytes")?;
    if data.index() != 0 || size_px == 0 || ![1, 2].contains(&dpr) {
        return Err("invalid M3 specimen face, size or DPR".into());
    }
    let font = ProbeFont::new(data.bytes()).map_err(|error| error.to_string())?;
    // The normal monospaced specimen has rounded grid origins. D-27 fits
    // those 1× outlines per ppem; rotations, shear, zoom and Retina retain
    // the size-independent source shapes for affine fidelity.
    let hinted = if treatment == SpecimenTreatment::Normal && dpr == 1 {
        Some(
            font.hinted(f32::from(size_px), ProbeHintMode::AutoLight)
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    let m = matrix(f32::from(size_px) * f32::from(dpr), treatment);
    // The transformed paths remain size independent. The hinted branch
    // differs only in input curves; the Slug packer and shader are shared.
    let (atlas, mut instances, handles) = prepare_line(&font, hinted.as_ref(), m)?;
    let (width, height) = place_in_target(&mut instances)?;
    // Dirty texels remain in the CPU mirror until the renderer uploads
    // them; handles retain the same packing for the oracle comparison.
    Ok(SlugScene {
        atlas,
        instances,
        width,
        height,
        handles,
    })
}

/// Maps em x-right/y-up coordinates into transformed device pixels.
fn matrix(size: f32, treatment: SpecimenTreatment) -> [f32; 4] {
    // Zoom is a view enlargement of the same 24 px source line; retaining
    // its em geometry tests the point of Slug's size-independent atlas.
    let scale = if treatment == SpecimenTreatment::Zoom4 {
        size * 4.0
    } else {
        size
    };
    match treatment {
        SpecimenTreatment::Normal | SpecimenTreatment::Zoom4 => [scale, 0.0, 0.0, -scale],
        SpecimenTreatment::Rotate15 => {
            // Screen-clockwise rotation of [scale,0;0,-scale] produces
            // columns [s*cos,s*sin] and [s*sin,-s*cos].
            let angle = 15.0_f32.to_radians();
            [
                scale * angle.cos(),
                scale * angle.sin(),
                scale * angle.sin(),
                -scale * angle.cos(),
            ]
        }
        SpecimenTreatment::Rotate90 => [0.0, scale, scale, 0.0],
        // Slant comes from the y-up em coordinate: upper outline points
        // shift right while the baseline remains at its integer origin.
        SpecimenTreatment::Shear12 => [scale, 0.0, scale * 12.0_f32.to_radians().tan(), -scale],
    }
}

/// Prepares each distinct outline and emits one instance per ink glyph.
fn prepare_line(
    font: &ProbeFont<'_>,
    hinted: Option<&HintedProbe<'_>>,
    m: [f32; 4],
) -> Result<
    (
        SlugAtlas,
        Vec<SlugInstance>,
        Vec<puzzle_slug::atlas::ShapeHandle>,
    ),
    String,
> {
    let mut atlas = SlugAtlas::default();
    let mut instances = Vec::new();
    let mut handles = Vec::new();
    let mut pen = 0.0_f32;
    // Metrics are queried even for spaces, which have width but no contour.
    // Repeated letters reuse ShapeKey by scalar while receiving new origins.
    for ch in LINE.chars() {
        let advance = font.advance(ch).map_err(|error| error.to_string())?;
        if ch != ' ' {
            let path = match hinted {
                Some(hinter) => hinter.glyph(ch),
                None => font.glyph(ch),
            }
            .map_err(|error| error.to_string())?
            .0;
            let handle = atlas
                .prepare(ShapeKey(ch as u64), &path, FillRule::NonZero, 0)
                .map_err(|error| error.to_string())?;
            // Packed band coordinates come from the same handle consumed by
            // the CPU oracle, avoiding an independently prepared comparison.
            // Round the transformed baseline origin; the per-glyph matrix
            // still preserves exact em curves and the requested affine map.
            let t = [(m[0] * pen).round(), (m[1] * pen).round()];
            instances.push(SlugInstance {
                m,
                t,
                bbox: handle.bbox,
                glyph_loc: puzzle_slug::encoding::encode_glyph_loc(handle.glyph_loc),
                band_info: handle.band_info,
                color: 0xffff_ffff,
                paint: 0,
            });
            handles.push(handle);
        }
        pen += advance;
    }
    Ok((atlas, instances, handles))
}

/// Offsets every whole-pixel origin so the transformed union has a margin.
fn place_in_target(instances: &mut [SlugInstance]) -> Result<(u32, u32), String> {
    // Sweep the four bbox corners because rotation and shear can move any
    // one of them to an axis extremum; em min/max alone are insufficient.
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    for instance in instances.iter() {
        for x in [instance.bbox[0], instance.bbox[2]] {
            for y in [instance.bbox[1], instance.bbox[3]] {
                let px = instance.m[0] * x + instance.m[2] * y + instance.t[0];
                let py = instance.m[1] * x + instance.m[3] * y + instance.t[1];
                low[0] = low[0].min(px);
                low[1] = low[1].min(py);
                high[0] = high[0].max(px);
                high[1] = high[1].max(py);
            }
        }
    }
    if low
        .iter()
        .chain(high.iter())
        .any(|value| !value.is_finite())
    {
        // Bad font coordinates must fail before an unbounded image size is
        // converted to integer dimensions and sent to the GPU device.
        return Err("specimen has invalid transformed bounds".into());
    }
    // The extra two pixels protect the half-pixel Slug dilation even when
    // the transformed bbox lands exactly on an integer boundary.
    let shift = [(24.0 - low[0]).ceil(), (24.0 - low[1]).ceil()];
    let width = (high[0] + shift[0] + 24.0).ceil() as u32;
    let height = (high[1] + shift[1] + 24.0).ceil() as u32;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err(format!(
            "specimen target {width}x{height} exceeds WebGPU limits"
        ));
    }
    for instance in instances {
        // The common integer shift preserves whole-pixel origins and the
        // relative affine placement of every glyph in the line.
        instance.t[0] += shift[0];
        instance.t[1] += shift[1];
    }
    Ok((width, height))
}
