//! Overlay rectangle compilation, slot-local clip stacks and compact batches.
//!
//! Logical commands cross into device pixels once, here. Empty scissors draw
//! nothing, and nested pushes intersect their parent, so neither an extension
//! nor an offscreen anchor can submit an invalid scissor to the GPU.
//!
//! Scissor rectangles use integer device pixels, unlike logical draw lists.
//! Left/top edges round down and right/bottom edges round up so fractional
//! coverage is not lost. Clamping both edges handles panels entering from
//! negative coordinates and clips larger than the physical target.
//!
//! A clip push intersects its parent. An unresolvable push creates an empty
//! clip rather than exposing the child content. Pop must balance that push
//! even when its own anchor is hidden; it restores the previous rectangle.
//! Each provider/slot starts with a fresh stack, keeping scopes independent.
//!
//! Grid slots inherit the terminal grid scissor supplied by frame assembly.
//! Top inherits no scissor and can paint scrollbar/search UI into padding.
//! Empty intersections are rejected before producing a batch, since wgpu
//! forbids zero-size scissor rectangles on a draw.
//!
//! Rectangles with invalid/nonfinite dimensions are ignored at this seam.
//! Their geometry is untrusted extension input, just like a JS resize.
//! Rounded corners and stroke widths scale with DPR together with bounds;
//! colors remain straight RGBA8 for the renderer's premultiplied blend.
//!
//! Consecutive rectangles with the same slot, pipeline and scissor extend
//! one instance range. A clip change starts a batch because reordering it
//! would change painter semantics. Sorting only by slot preserves ties.
//! Seven fixed slots keep insertion ordering small without sort's scratch.
//!
//! Instance arrays need not be physically shuffled: batches reference their
//! original ranges. This preserves submitted text/cursor arrays and retains
//! their warmed allocation. GPU execution follows the batch order alone.
//!
//! Shape preparation shares this slot/clip compilation with rectangles.
//! A shape may add several guarded chunks, each with its own owner scissor.
//! Gradient rows use the shared text context and transfer uploads afterward.
//! Free text uses this same command boundary; regions remain logical for routing.
//! They submit no GPU instance unless the provider also paints a background.
//! Compiler dispatch separates resolving anchors from preparing drawable ink.
//! Clip commands always balance even when a tracked anchor is currently hidden.
//! The atlas reports malformed shapes locally without suppressing other panels.
//! Free-text source requests use the same shared context as ordinary grid ink.
//! Shape and text preparation return uploads to the final collection phase.
use puzzle_frame::{Batch, PipelineKind, RectInstance, RectU32, RenderFrame};
use puzzle_overlay::{
    draw::{Draw, Primitive, RectSpec},
    geometry::{Rect, TextGeometry},
};

/// Compiles one retained command using a reused stack and the slot's grid clip.
pub(super) fn command(
    context: &mut crate::frame_builder::context::TextContext,
    frame: &mut RenderFrame,
    draw: &Draw,
    geometry: &TextGeometry,
    stack: &mut Vec<RectU32>,
    base: Option<RectU32>,
) {
    // Pop has no geometry and must balance even if a preceding anchor is hidden.
    if matches!(draw.primitive, Primitive::ClipPop) {
        stack.pop();
        return;
    }
    // The sampled anchor belongs to this frame, not the now-running parser.
    let origin = draw.resolved.origin(geometry);
    // An empty explicit clip remains empty rather than falling back to the grid.
    let clip = stack.last().copied().or(base);
    if let Primitive::ClipPush(rect) = draw.primitive {
        let next = origin.map_or(
            RectU32 {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            |origin| scissor(rect, origin, geometry.dpr, frame.target_size),
        );
        stack.push(clip.map_or(next, |parent| intersect(parent, next)));
        return;
    }
    let Some(origin) = origin else {
        // A tracked anchor outside the captured view never submits stale coordinates.
        return;
    };
    if clip.is_some_and(|rect| rect.width == 0 || rect.height == 0) {
        // Preserve balanced clip stacks, but omit every descendant draw of an empty clip.
        return;
    }
    primitive(context, frame, draw, origin, clip, geometry.dpr);
}

/// Converts a logical panel into physical object coordinates and extends its batch.
fn append_rect(
    frame: &mut RenderFrame,
    spec: &RectSpec,
    origin: [f32; 2],
    slot: puzzle_frame::ZSlot,
    clip: Option<RectU32>,
    dpr: f32,
) {
    // Invalid dimensions must not leak NaNs into the rounded-rectangle SDF shader.
    // A nonfinite anchor offset is equally invalid, even for a finite local box.
    if !spec
        .rect
        .size
        .iter()
        .all(|value| value.is_finite() && *value > 0.0)
        || !origin
            .iter()
            .chain(&spec.rect.origin)
            .all(|value| value.is_finite())
    {
        return;
    }
    // Existing grid and cursor rectangles stay at their original array indices.
    let index = frame.rects.len() as u32;
    frame.rects.push(RectInstance {
        rect: [
            (spec.rect.origin[0] + origin[0]) * dpr,
            (spec.rect.origin[1] + origin[1]) * dpr,
            spec.rect.size[0] * dpr,
            spec.rect.size[1] * dpr,
        ],
        color: spec.color,
        corner_radius: spec.radius.max(0.0) * dpr,
        stroke_width: spec.stroke.max(0.0) * dpr,
        flags: 0,
    });
    // Consecutive compatible panels become one instanced draw, never one per badge.
    if let Some(last) = frame.batches.last_mut()
        && last.slot == slot
        && last.pipeline == PipelineKind::Rect
        && last.scissor == clip
        && last.instances.end == index
    {
        last.instances.end += 1;
    } else {
        frame.batches.push(Batch {
            slot,
            pipeline: PipelineKind::Rect,
            instances: index..index + 1,
            scissor: clip,
        });
    }
}

/// Orders the seven fixed slots stably without sort's optional heap scratch space.
pub(super) fn order_batches(batches: &mut [Batch]) {
    for index in 1..batches.len() {
        let mut current = index;
        // Moving only strictly lower slots preserves submission order on ties.
        while current > 0 && batches[current - 1].slot > batches[current].slot {
            batches.swap(current - 1, current);
            current -= 1;
        }
    }
}

/// Clips both edges independently, including rectangles entering from negative px.
fn scissor(rect: Rect, origin: [f32; 2], dpr: f32, target: [u32; 2]) -> RectU32 {
    let edge = |axis: usize, end: bool| {
        let value =
            (rect.origin[axis] + origin[axis] + if end { rect.size[axis].max(0.0) } else { 0.0 })
                * dpr;
        // Conservative integer coverage matches the renderer's fractional SDF edges.
        if end { value.ceil() } else { value.floor() }.clamp(0.0, target[axis] as f32) as u32
    };
    // Saturating differences reject inverted or wholly off-target input clips.
    let (x, y) = (edge(0, false), edge(1, false));
    RectU32 {
        x,
        y,
        width: edge(0, true).saturating_sub(x),
        height: edge(1, true).saturating_sub(y),
    }
}

/// Intersects physical scissors without overflowing untrusted rectangle endpoints.
pub(super) fn intersect(parent: RectU32, next: RectU32) -> RectU32 {
    let x = parent.x.max(next.x);
    let y = parent.y.max(next.y);
    RectU32 {
        x,
        y,
        width: parent
            .x
            .saturating_add(parent.width)
            .min(next.x.saturating_add(next.width))
            .saturating_sub(x),
        height: parent
            .y
            .saturating_add(parent.height)
            .min(next.y.saturating_add(next.height))
            .saturating_sub(y),
    }
}

/// Sends a resolved drawable to its store without changing the clip stack.
fn primitive(
    context: &mut crate::frame_builder::context::TextContext,
    frame: &mut RenderFrame,
    draw: &Draw,
    origin: [f32; 2],
    clip: Option<RectU32>,
    dpr: f32,
) {
    if let Primitive::Rect(spec) = &draw.primitive {
        // Rectangles need no atlas preparation and can extend the current panel batch.
        append_rect(frame, spec, origin, draw.slot, clip, dpr);
    }
    if let Primitive::Shape(shape, transform, paint) = &draw.primitive
        && let Err(error) = super::shapes::append(
            &mut context.backend,
            frame,
            shape,
            *transform,
            paint,
            origin,
            draw.slot,
            clip,
            dpr,
        )
    {
        // Untrusted extension geometry fails locally; other providers still draw.
        tracing::warn!(event = "overlay.shape_rejected", %error, "invalid overlay shape skipped");
    }
    if let Primitive::Text(spec) = &draw.primitive
        && let Err(error) = super::text::append(context, frame, spec, origin, draw.slot, clip, dpr)
    {
        tracing::warn!(event = "overlay.text_rejected", %error, "invalid overlay text skipped");
    }
    if let Primitive::Raster(source, rect) = &draw.primitive {
        // The public reserved primitive uses logical coordinates like other overlays.
        // Apply the sampled anchor before DPR; source UVs remain original-image pixels.
        // The clip stack already intersects the grid and all parent provider clips.
        // append_image validates untrusted crop/geometry and ignores hidden sources.
        // No glyph preparation or shape-cache insertion happens for this primitive.
        frame.append_image(
            source,
            [
                (origin[0] + rect.origin[0]) * dpr,
                (origin[1] + rect.origin[1]) * dpr,
                rect.size[0] * dpr,
                rect.size[1] * dpr,
            ],
            draw.slot,
            clip,
        );
    }
    // Regions stay in the retained list for the common platform-neutral pointer router.
}
