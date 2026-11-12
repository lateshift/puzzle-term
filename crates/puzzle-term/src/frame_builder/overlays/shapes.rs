//! Compiles immutable overlay paths through the shared Slug/paint stores.
//!
//! Linear transforms are baked once into guarded physical-space chunks; per-draw
//! translations remain cheap instance data. Pixel scissors share the same rounded
//! boundary on both sides, so overlap preserves antialiasing without double blend.
//!
//! Shape unit space is not font em space. Providers author logical paths, and
//! the cache bakes their linear map and DPR into physical quadratic coordinates.
//! The remaining instance matrix is identity; its translation moves that variant
//! without recutting contours or reallocating paint rows on each frame.
//! Separating translation lets animated panels reuse their expensive geometry.
//!
//! Each chunk has a guarded outline and an exclusive owner rectangle. Coverage
//! near a cut reads the guarded outline, while a scissor admits only owned pixels.
//! Using the same ceiling rule at both edges partitions the target exactly, even
//! when the provider places the shape at a fractional pixel translation.
//! Independently flooring left edges would blend translucent seams twice.
//! Independently rounding right edges inward could instead leave a visible gap.
//!
//! The gradient inverse is mapped into the same physical root coordinates as
//! the outline. Per-instance translation is removed by Slug's interpolation,
//! so gradient colors remain attached to the source path across chunk boundaries.
//! Solid colors bypass the paint table as they do for ordinary glyph ink.
//! All uploads remain in the shared context until frame collection (§10.4).
use kurbo::Affine;
use puzzle_frame::{Batch, PipelineKind, RectU32, RenderFrame, SlugInstance, ZSlot};
use puzzle_overlay::draw::{Paint, ShapeRef};
use puzzle_text::glyph::slug::SlugBackend;

/// Prepares a path variant and appends chunk instances in the provider's painter order.
#[allow(
    clippy::too_many_arguments,
    reason = "draw state crosses logical provider and physical painter seams"
)]
pub(super) fn append(
    backend: &mut SlugBackend,
    frame: &mut RenderFrame,
    shape: &ShapeRef,
    transform: Affine,
    paint: &Paint,
    origin: [f32; 2],
    slot: ZSlot,
    clip: Option<RectU32>,
    dpr: f32,
) -> Result<(), String> {
    // A singular linear map cannot supply Slug's inverse Jacobian for coverage.
    let c = transform.as_coeffs();
    if !c.iter().all(|v| v.is_finite()) || transform.determinant().abs() < 1e-12 {
        return Err("shape transform is nonfinite or singular".into());
    }
    let linear = Affine::scale(f64::from(dpr)) * Affine::new([c[0], c[1], c[2], c[3], 0.0, 0.0]);
    // Anchors and commands are logical; DPR applies once at this seam.
    let translation = [
        (c[4] as f32 + origin[0]) * dpr,
        (c[5] as f32 + origin[1]) * dpr,
    ];
    let gradient = match &paint.gradient {
        // Prepare the brush first to avoid borrowing the backend across its chunk slice.
        Some(brush) => backend.prepare_overlay_paint(brush, linear)?,
        None => 0,
    };
    for chunk in backend.prepare_overlay(&shape.0, linear)? {
        // Inherited provider clips further restrict the chunk's exclusive ownership.
        let owner = ownership(chunk.owner, translation, frame.target_size);
        let scissor = clip.map_or(owner, |clip| super::paint::intersect(clip, owner));
        if scissor.width == 0 || scissor.height == 0 {
            // Clipping can hide a prepared sibling without changing retained geometry.
            continue;
        }
        let index = frame.slugs.len() as u32;
        let handle = chunk.shape;
        // Packed addresses come from the current atlas generation, never a provider.
        frame.slugs.push(SlugInstance {
            m: [1.0, 0.0, 0.0, 1.0],
            t: translation,
            bbox: handle.bbox,
            glyph_loc: puzzle_slug::encoding::encode_glyph_loc(handle.glyph_loc),
            band_info: handle.band_info,
            color: paint.color,
            paint: gradient,
        });
        batch(frame, slot, Some(scissor), index..index + 1);
    }
    Ok(())
}

/// Adjacent chunks use identical integer boundaries, including fractional translations.
fn ownership(rect: kurbo::Rect, translation: [f32; 2], target: [u32; 2]) -> RectU32 {
    // Clamp both endpoints before subtraction; wholly offscreen owners become empty.
    let edge = |value: f64, axis: usize| {
        (value + f64::from(translation[axis]))
            .ceil()
            .clamp(0.0, f64::from(target[axis])) as u32
    };
    let (x, y) = (edge(rect.x0, 0), edge(rect.y0, 1));
    // Saturating differences avoid unsigned wrap for invalid/reversed clip bounds.
    RectU32 {
        x,
        y,
        width: edge(rect.x1, 0).saturating_sub(x),
        height: edge(rect.y1, 1).saturating_sub(y),
    }
}

/// Compatible consecutive vector instances extend an existing draw range.
pub(super) fn batch(
    frame: &mut RenderFrame,
    slot: ZSlot,
    clip: Option<RectU32>,
    instances: std::ops::Range<u32>,
) {
    // An empty range contributes neither a batch nor a draw call.
    if instances.is_empty() {
        return;
    }
    if let Some(last) = frame.batches.last_mut()
        && last.slot == slot
        && last.pipeline == PipelineKind::Slug
        && last.scissor == clip
        && last.instances.end == instances.start
    {
        // Only adjacent compatible ranges coalesce; painter order stays intact.
        last.instances.end = instances.end;
    } else {
        // A scissor or slot change is a real state boundary inside the same pass.
        frame.batches.push(Batch {
            slot,
            pipeline: PipelineKind::Slug,
            instances,
            scissor: clip,
        });
    }
}
