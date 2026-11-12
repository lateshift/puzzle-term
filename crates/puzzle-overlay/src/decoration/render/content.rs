//! Natural annotation positioning and SVG fitting on visible source rectangles.
//!
//! Text measurement uses the public layouter. SVG parsing belongs to Session's
//! mutation boundary, so scroll/hover builds only apply an affine to retained paths.
//!
//! Free-text origins denote the line's top-left before its final affine map.
//! The natural layout already includes the face's baseline; adding that
//! baseline again would move After/Before labels below their source row.
//! Width comes from shaping, not character count, when fonts are available.
//!
//! Only Before needs width to determine its horizontal origin. Above uses
//! the measured line allocation height; Below uses the source row height.
//! EndOfLine follows the last written nonblank cell rather than the grid's
//! right edge, so an annotation can be seen without enlarging the grid.
//!
//! Gutter labels intentionally right-align in padding. The declaration's
//! explicit slot controls clipping, just as it does for external providers.
//! Icon labels are ordinary Unicode text and therefore use loaded fallback
//! faces rather than a private icon renderer or a system-font dependency.
//!
//! SVG fit removes the authored minimum and then uniformly scales the path.
//! Uniform scaling preserves aspect ratio and centers leftover whitespace.
//! Custom affines remain relative to the source's first visible segment;
//! they can rotate or shear the same immutable path without parsing it again.
use super::*;
use crate::{draw::TextSpec, text::TextMetrics};
use kurbo::Shape;
/// Dispatches the three non-cell-aligned declarative content families.
/// Each uses the definition's explicit painter slot and frozen source segments.
/// The first and last visible rectangles determine Before/After endpoints.
/// Measurement and drawing use the same TextSpec so fallback and style agree.
/// Glyph content never writes into the VT buffer or changes copied text.
pub(super) fn paint(sample: &Sample, rects: &[Rect], ctx: &ViewCtx<'_>, out: &mut DrawList) {
    match &sample.declaration.kind {
        DecorationKind::InlineText {
            text: label,
            style,
            placement,
        } => {
            let mut spec = text(label, *style);
            let metrics = measure(&spec, ctx);
            let first = rects[0];
            let last = rects[rects.len() - 1];
            let point = position(first, last, metrics, *placement, ctx);
            spec.transform = Affine::translate((f64::from(point[0]), f64::from(point[1])));
            push(out, sample.declaration.slot, Primitive::Text(spec));
        }
        DecorationKind::GutterBadge { text: label, color } => {
            let first = rects[0];
            // The device-padding gutter can shrink in logical pixels at higher DPR.
            let size = (ctx.geometry.cell[1] * 0.65)
                .min(ctx.geometry.inset * 0.65)
                .clamp(1.0, 200.0);
            let mut spec = text(
                label,
                TextStyle {
                    size,
                    face: None,
                    style: 0,
                    color: *color,
                },
            );
            let metrics = measure(&spec, ctx);
            // Right-align in the logical left gutter; the explicit Top slot allows padding ink.
            let x = (ctx.geometry.inset - metrics.width - 1.0).max(0.0);
            spec.transform = Affine::translate((f64::from(x), f64::from(first.origin[1])));
            push(out, sample.declaration.slot, Primitive::Text(spec));
        }
        DecorationKind::Shape { fit, paint, .. } => shape(sample, rects, *fit, paint, out, ctx),
        _ => {}
    }
}
/// Builds a natural line; its final top-left placement is supplied separately.
/// Font size is logical, so the shared layouter applies the host DPR once.
/// None selects the primary face and ordinary whole-cluster fallback chain.
/// The affine starts as identity because measurement precedes final translation.
/// Color is straight RGBA8, matching every public text primitive.
pub(super) fn text(label: &str, style: TextStyle) -> TextSpec {
    TextSpec {
        text: label.into(),
        size: style.size,
        face: style.face,
        style: style.style,
        color: style.color,
        transform: Affine::IDENTITY,
    }
}
/// Actual loaded-font metrics win; synthetic pointer-only contexts use a finite estimate.
/// Measurement failures never make a tooltip panic while fonts are unavailable.
/// The estimate only sizes presentation; it does not choose a glyph backend.
/// Frames with a real layouter use exact shaped width and allocated line height.
/// Whitespace has advance even when its ink rectangle is empty.
pub(super) fn measure(spec: &TextSpec, ctx: &ViewCtx<'_>) -> TextMetrics {
    ctx.text
        .and_then(|t| t.measure(spec).ok())
        .unwrap_or(TextMetrics {
            width: spec.text.chars().count() as f32 * spec.size * 0.6,
            height: spec.size * 1.3,
            baseline: spec.size,
            ink: Rect::default(),
        })
}
/// Maps the five placements without changing the source's grid cell advance.
/// Before subtracts natural text width from the first source cell's x origin.
/// Above subtracts annotation height; Below adds the source's row height.
/// After uses the last source segment's right edge rather than glyph ink bounds.
/// EndOfLine queries only a line already present in the frozen visible buffer.
fn position(
    first: Rect,
    last: Rect,
    metrics: TextMetrics,
    placement: Placement,
    ctx: &ViewCtx<'_>,
) -> [f32; 2] {
    match placement {
        Placement::After => [last.origin[0] + last.size[0], last.origin[1]],
        Placement::Before => [first.origin[0] - metrics.width, first.origin[1]],
        Placement::Above => [first.origin[0], first.origin[1] - metrics.height],
        Placement::Below => [first.origin[0], first.origin[1] + first.size[1]],
        Placement::EndOfLine => [end_of_line(last, ctx), last.origin[1]],
    }
}
/// Aspect-preserving centering keeps SVG glyphs whole rather than stretching them.
/// Both scale ratios use positive finite source extents with an epsilon floor.
/// The smaller ratio fits the entire authored rectangle inside the target.
/// Leftover space is distributed equally along each logical target axis.
/// Authored SVG minima are removed in source coordinates before scaling.
fn fit_matrix(source: kurbo::Rect, target: Rect) -> Affine {
    let scale = (f64::from(target.size[0]) / source.width().max(f64::EPSILON))
        .min(f64::from(target.size[1]) / source.height().max(f64::EPSILON));
    let origin = [
        f64::from(target.origin[0]) + (f64::from(target.size[0]) - source.width() * scale) * 0.5,
        f64::from(target.origin[1]) + (f64::from(target.size[1]) - source.height() * scale) * 0.5,
    ];
    // Remove the SVG's authored minimum after applying its fitted uniform scale.
    Affine::translate((origin[0], origin[1]))
        * Affine::scale(scale)
        * Affine::translate((-source.x0, -source.y0))
}

/// SVG sources are immutable; dirty builds only choose their logical fit matrix.
/// Cell uses one cell-sized target; Range uses the visible segments' union.
/// Custom preserves caller rotation/shear relative to the first visible source.
/// Sharing the parsed ShapeRef lets the compiler reuse its prepared Slug chunks.
/// The primitive carries authored paint independently of the geometry cache.
fn shape(
    sample: &Sample,
    rects: &[Rect],
    fit: ShapeFit,
    paint: &Paint,
    out: &mut DrawList,
    ctx: &ViewCtx<'_>,
) {
    let Some(shape) = &sample.shape else {
        return;
    };
    let Some(bounds) = union(rects) else {
        return;
    };
    let source = shape.0.bounding_box();
    let origin = rects[0].origin.map(f64::from);
    let matrix = match fit {
        ShapeFit::Custom(matrix) => Affine::translate((origin[0], origin[1])) * matrix,
        ShapeFit::Cell => fit_matrix(
            source,
            Rect {
                size: ctx.geometry.cell,
                ..rects[0]
            },
        ),
        ShapeFit::Range => fit_matrix(source, bounds),
    };
    push(
        out,
        sample.declaration.slot,
        Primitive::Shape(shape.clone(), matrix, paint.clone()),
    );
}

/// Finds the last written nonblank cell only in the already captured visible line.
fn end_of_line(last: Rect, ctx: &ViewCtx<'_>) -> f32 {
    let column = ctx
        .geometry
        .lines
        .iter()
        .find(|line| line.y == last.origin[1])
        .and_then(|line| ctx.buffer.line(line.id))
        .and_then(|line| {
            line.cells().iter().rposition(|cell| {
                (cell.ch != 0 && cell.ch != 32)
                    || cell.flags.contains(puzzle_vt::CellFlags::WIDE_SPACER)
            })
        })
        .map_or(0, |last| last + 1);
    ctx.geometry.inset + column as f32 * ctx.geometry.cell[0]
}
