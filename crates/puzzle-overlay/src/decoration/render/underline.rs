//! Pixel-sized rules and cached seamless Slug waves for declarative underlines.
//!
//! The source clip spans the entire expanded stroke height, not just its centerline.
//! This keeps the crest and trough visible while excluding neighboring source columns.
//! At fractional device scales the last repeated period can extend past the range.
//! Its scissor enforces the same inclusive source endpoint as the straight rules.
//!
//! Thickness is one device pixel in logical geometry. Curly contours repeat
//! one period per cell and share a single cached path, just like grid underlines.
//!
//! Each source row is independent: a wrapped range yields one rule per
//! visible segment. The row's logical height places the rule below ordinary
//! ink while fractional scrolling moves it with the same source rectangle.
//! One physical pixel becomes 1/DPR logical pixels, including on Retina.
//!
//! Single and double rules use sharp rectangles. Dotted patterns use a
//! rounded rectangle exactly one pixel wide; dashed patterns use a fraction
//! of the cell width. The final straight segment is clipped to its source
//! width instead of painting beyond the inclusive range endpoint.
//!
//! A curly underline is an expanded quadratic centerline, rendered through
//! Slug rather than a second tessellator. The shape cache retains one period
//! for a given cell width and thickness. Animated dirty builds clone only
//! its shared path handle instead of repeating stroke expansion.
//!
//! The period starts and ends at the same height with matching tangents.
//! Butt caps join consecutive periods without overlapping rounded endpoints.
//! Stroke expansion supplies a closed fill outline before the compiler
//! splits any large transformed shape into its guarded device chunks.
//!
//! Cached geometry depends on width and thickness, not color or position.
//! The key includes both floating-point bit patterns in private provider storage.
//! Changing DPR can therefore choose another outline, while theme changes
//! only change the primitive's paint. No GPU address enters this provider.
use super::*;
use kurbo::{BezPath, Cap, Stroke, StrokeOpts, stroke};
use puzzle_slug::ShapeKey;
/// Appends every SGR underline style beneath the source ink without changing cells.
/// None deliberately emits no draw command and no hit-test geometry of its own.
/// The two solid variants differ only by a second parallel one-pixel rule.
/// Patterned styles repeat in logical coordinates derived from device-sized thickness.
/// All variants retain the declaration's z-slot and its source-row displacement.
pub(super) fn paint(
    rects: &[Rect],
    style: UnderlineStyle,
    color: u32,
    slot: ZSlot,
    ctx: &ViewCtx<'_>,
    out: &mut DrawList,
    cache: &crate::shapes::ShapeCache,
) {
    if style == UnderlineStyle::None {
        return;
    }
    let thickness = 1.0 / ctx.geometry.dpr.max(f32::EPSILON);
    for rect in rects {
        let y = rect.origin[1] + rect.size[1] - thickness * 2.0;
        match style {
            UnderlineStyle::Single | UnderlineStyle::Double => {
                rule(
                    out,
                    slot,
                    [rect.origin[0], y],
                    [rect.size[0], thickness],
                    color,
                    0.0,
                );
                if style == UnderlineStyle::Double {
                    rule(
                        out,
                        slot,
                        [rect.origin[0], y - thickness * 2.0],
                        [rect.size[0], thickness],
                        color,
                        0.0,
                    );
                }
            }
            UnderlineStyle::Dotted | UnderlineStyle::Dashed => {
                let width = if style == UnderlineStyle::Dotted {
                    thickness
                } else {
                    ctx.geometry.cell[0] * 0.6
                };
                let step = if style == UnderlineStyle::Dotted {
                    thickness * 3.0
                } else {
                    ctx.geometry.cell[0]
                };
                let mut x = 0.0;
                while x < rect.size[0] {
                    rule(
                        out,
                        slot,
                        [rect.origin[0] + x, y],
                        [width.min(rect.size[0] - x), thickness],
                        color,
                        if style == UnderlineStyle::Dotted {
                            thickness * 0.5
                        } else {
                            0.0
                        },
                    );
                    x += step;
                }
            }
            UnderlineStyle::Curly => wave((*rect, y), (thickness, color), slot, ctx, out, cache),
            UnderlineStyle::None => {}
        }
    }
}
/// Small straight patterns share the renderer's rectangle SDF and painter order.
/// Rounded dots request a half-thickness radius rather than a separate shape cache.
/// Sharp rules and dashes use radius zero, so whole-pixel edges remain exact.
/// Inward rectangle strokes are absent here because these patterns are filled ink.
/// Source widths are clipped by the caller before a final segment is appended.
fn rule(
    out: &mut DrawList,
    slot: ZSlot,
    origin: [f32; 2],
    size: [f32; 2],
    color: u32,
    radius: f32,
) {
    rectangle(out, slot, Rect { origin, size }, color, radius, 0.0);
}
/// Prepares one quadratic period and instances it across the visible source interval.
/// Repeated instances share both prepared contours and color paint semantics.
/// The cache key includes size-dependent geometry and excludes translation/color.
/// Cold stroke expansion happens only inside the cache's missing-key closure.
/// A failed preparation skips that underline without corrupting the other declarations.
/// Consecutive endpoints use identical height and tangent direction.
/// A source scissor suppresses any final floating-point overshoot of the repeated period.
/// The shape is a filled expanded stroke, so only the existing Slug pipeline is needed.
/// The final clip pop restores the provider's enclosing slot scissor.
fn wave(
    source: (Rect, f32),
    ink: (f32, u32),
    slot: ZSlot,
    ctx: &ViewCtx<'_>,
    out: &mut DrawList,
    cache: &crate::shapes::ShapeCache,
) {
    let (rect, y) = source;
    let (thickness, color) = ink;
    let width = ctx.geometry.cell[0].max(thickness);
    // Private storage avoids collisions with arbitrary extension-supplied shape keys.
    let key = ShapeKey((u64::from(width.to_bits()) << 32) | u64::from(thickness.to_bits()));
    let result = cache.get_or_prepare(key, || {
        let w = f64::from(width);
        let h = f64::from(thickness);
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.quad_to((w * 0.25, -h * 2.0), (w * 0.5, 0.0));
        path.quad_to((w * 0.75, h * 2.0), (w, 0.0));
        // Butt endpoints meet adjacent periods without introducing a round-cap bulge.
        stroke(
            path.iter(),
            &Stroke::new(h).with_caps(Cap::Butt),
            &StrokeOpts::default(),
            0.001,
        )
    });
    if let Ok(shape) = result {
        // Fractional DPR can accumulate tiny period errors; clip to the exact source width.
        push(
            out,
            slot,
            Primitive::ClipPush(Rect {
                origin: [rect.origin[0], y - thickness * 2.5],
                size: [rect.size[0], thickness * 5.0],
            }),
        );
        let mut x = 0.0;
        while x < rect.size[0] {
            push(
                out,
                slot,
                Primitive::Shape(
                    shape.clone(),
                    Affine::translate((f64::from(rect.origin[0] + x), f64::from(y))),
                    Paint {
                        color,
                        gradient: None,
                    },
                ),
            );
            x += width;
        }
        push(out, slot, Primitive::ClipPop);
    }
}
