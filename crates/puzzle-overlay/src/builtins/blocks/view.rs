//! Command block ink, source flash and source-bounded interactive copy placement.
//!
//! All positions are logical surface points from the sealed publication. The
//! source rectangle stays fixed while a button may move around host occlusion.
//! The flash is below text: it never changes cell content or the selection model.
//! Button text and its hit region are emitted together, or omitted together.
//! Timing comes from the model's receipt; drawing has no scheduling capability.
use super::*;
use crate::view::OverlayView;
use std::fmt::Write as _;

/// Rebuildable drawing has no clock, clipboard or scheduling capability.
#[derive(Default)]
pub struct BlocksView {
    /// Recycle labels between flash frames; animation never allocates strings.
    labels: Vec<String>,
}
/// Label buffers are rebuildable render-side caches, never shared with Session.
impl BlocksView {
    /// Retained label capacity follows the actual visible command working set.
    fn label_buffer(&mut self) -> String {
        // An unused view starts empty. Initial labels may allocate, then future
        // clock-driven redraws reuse their existing UTF-8 capacity.
        let mut label = self.labels.pop().unwrap_or_default();
        label.clear();
        label
    }
}
/// This painter can be recreated from a sealed source without losing interaction.
impl OverlayView for BlocksView {
    /// Recover text allocations only after this view's old list is no longer drawn.
    fn recycle(&mut self, out: &mut DrawList) {
        // This callback runs before the registry clears the old list, after
        // the frame compiler has finished consuming it. Published model states
        // never own these strings, so recycling cannot alter an in-flight view.
        for draw in &mut out.draws {
            if let Primitive::Text(text) = &mut draw.primitive {
                // Taking the buffer leaves a valid empty TextSpec until clear().
                // The cache grows only if the visible badge count itself grows.
                self.labels.push(std::mem::take(&mut text.text));
            }
        }
    }
    /// Gutter ink and target bounds share precisely the frame's neutral row placements.
    fn build(&mut self, state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        // The unit of interaction is an immutable command ID, not a row number.
        let Some(state) = state.as_any().downcast_ref::<State>() else {
            return;
        };
        for block in &ctx.models.blocks {
            let Some(bounds) = block_bounds(ctx, block) else {
                continue;
            };
            // A passive row observer loses to text/UI regions and consumes no click.
            out.push(
                Anchor::Viewport(Rect::default()),
                ZSlot::BelowText,
                Primitive::Region(bounds, RegionId(block.id), CursorIcon::Default),
            );
            // An explicit nonzero completion is the only failure-color source.
            let token = if block.exit.is_some_and(|exit| exit != 0) {
                "zoneError"
            } else {
                "zoneOk"
            };
            // The thin bar occupies padding immediately to the left of grid text.
            let gutter = Rect {
                origin: [bounds.origin[0] - 4.0, bounds.origin[1]],
                size: [2.0, bounds.size[1]],
            };
            // Top lets the gutter use padding without expanding the grid scissor.
            rect(out, gutter, ctx.theme.color(token, 0xff88cc88), 0.0);
            // A receipt belongs to one command, even if the pointer now hovers
            // a different block. A new command cannot inherit a stale success.
            let feedback = state.feedback.filter(|receipt| receipt.id == block.id);
            flash(ctx, out, bounds, feedback);
            // Only completed durations over the specified second get annotation ink.
            if let Some(duration) = block.duration.filter(|seconds| *seconds > 1.0) {
                let mut value = self.label_buffer();
                // String formatting is infallible and retains its warmed capacity.
                let _ = write!(value, "{duration:.1}s");
                label(ctx, out, value, bounds, false, block.id, "badgeForeground");
            }
            // A hover button replaces the duration paper in the same corner.
            // Keep the result visible when the mouse leaves. Hover alone is
            // transient; explicit feedback persists until its model deadline.
            if state.hover == Some(block.id) || feedback.is_some() {
                let mut value = self.label_buffer();
                value.push_str(feedback.map_or("Copy output", CopyFeedback::label));
                label(
                    ctx,
                    out,
                    value,
                    bounds,
                    true,
                    block.id,
                    feedback.map_or("badgeForeground", CopyFeedback::token),
                );
            }
        }
    }
}
/// Badges use the shared text layouter; the copy region covers only the visible paper.
fn label(
    ctx: &ViewCtx<'_>,
    out: &mut DrawList,
    value: String,
    block: Rect,
    copy: bool,
    id: u64,
    token: &str,
) {
    // Measure the owned TextSpec that will be drawn. The generic query-field
    // convenience helper clones a borrowed string; a per-frame fade must reuse
    // the recovered label allocation instead (P1). Matching colour and style
    // also lets measurement and painting use the very same natural-layout cache.
    let mut spec = TextSpec {
        text: value,
        size: 12.0,
        face: None,
        style: 0,
        color: ctx.theme.color(token, 0xffffffff),
        transform: kurbo::Affine::IDENTITY,
    };
    let natural = ctx
        .text
        .and_then(|text| text.measure(&spec).ok())
        .map_or(0.0, |metrics| metrics.width);
    // Success/warning changes the label width, so placement must be recomputed
    // from the same host exclusions as the ordinary Copy output affordance.
    let width = (natural + 12.0).min(block.size[0]);
    let Some(bounds) = badge_bounds(ctx, block, width, copy) else {
        return;
    };
    // Semantic paper and text are resolved in this publication's theme.
    rect(
        out,
        bounds,
        ctx.theme.color("badgeBackground", 0xff202020),
        3.0,
    );
    // Paper is clipped separately from grid ink because Top includes padding.
    push(out, Primitive::ClipPush(bounds));
    // Only translation changes after measurement; size and font remain identical.
    // This is a logical surface offset; the compiler alone multiplies it by DPR.
    spec.transform = kurbo::Affine::translate((
        f64::from(bounds.origin[0] + 6.0),
        f64::from(bounds.origin[1] + 3.0),
    ));
    push(out, Primitive::Text(spec));
    // Later overlays must inherit their own clip rather than the badge's bounds.
    push(out, Primitive::ClipPop);
    // Duration labels are inert; only the copy label creates a pointer target.
    if copy {
        push(
            out,
            Primitive::Region(bounds, RegionId(id | COPY), CursorIcon::Pointer),
        );
    }
}

/// Tint exactly this command's visible output, with glyphs painted above it.
fn flash(ctx: &ViewCtx<'_>, out: &mut DrawList, bounds: Rect, feedback: Option<CopyFeedback>) {
    if let Some(receipt) = feedback {
        // Expiry is checked again against this snapshot's clock; a late
        // renderer cannot resurrect ink that the model already retired.
        let opacity = receipt.flash(ctx.time, ctx.reduced_motion);
        if opacity > 0.0 {
            // BelowText tints the copied source without hiding any glyphs.
            out.push(
                Anchor::Viewport(Rect::default()),
                ZSlot::BelowText,
                Primitive::Rect(RectSpec {
                    rect: bounds,
                    radius: 0.0,
                    stroke: 0.0,
                    color: alpha(ctx.theme.color("selectionBackground", 0x60407090), opacity),
                }),
            );
        }
    }
}

/// A control's paper and hit target always share the same unobscured rectangle.
fn badge_bounds(ctx: &ViewCtx<'_>, block: Rect, width: f32, copy: bool) -> Option<Rect> {
    // The right edge stays aligned with the command's visible output rectangle.
    let bounds = Rect {
        origin: [block.origin[0] + block.size[0] - width, block.origin[1]],
        // One-row command output may be shorter than the preferred badge.
        // Its control still fits its source instead of becoming unclickable.
        size: [width, 22.0_f32.min(block.size[1])],
    };
    // Copy is movable UI even though its output source is fixed. Clip available
    // rows at the grid: fractional edge rows must not put a whole button outside it.
    // Duration labels are inert annotations, so they retain source coordinates.
    // Reposition only the interactive label: changing the block's own bounds
    // would also displace its status bar and passive output-hover observer.
    // A fully obscured output can omit the control, but its source observer
    // still permits hover again when native UI moves out of the way.
    if copy {
        let top = block.origin[1].max(ctx.geometry.grid.origin[1]);
        let bottom = (block.origin[1] + block.size[1])
            .min(ctx.geometry.grid.origin[1] + ctx.geometry.grid.size[1]);
        let area = Rect {
            origin: [block.origin[0], top],
            size: [block.size[0], bottom - top],
        };
        ctx.place_control(bounds, area)
    } else {
        Some(bounds)
    }
}

/// Resolves a frozen interval to the actual visible row placements without allocating.
fn block_bounds(ctx: &ViewCtx<'_>, block: &crate::models::blocks::Block) -> Option<Rect> {
    // Clip against actual line geometry, including partially scrolled edge rows.
    let rows = ctx
        .geometry
        .lines
        .iter()
        .filter(|row| row.id >= block.first && row.id <= block.last);
    // The source interval may be larger than the viewport. Walking placements
    // clips it to rows actually shown, including fractional top/bottom rows.
    // Row identities select the source; their current Y positions select ink.
    // Accumulate row extents without allocating a second geometry list.
    let mut first = None;
    // The final visible baseline contributes one complete cell height.
    let mut last = None;
    for row in rows {
        // Encounter order is the displayed row order, not command completion order.
        if first.is_none() {
            first = Some(row.y);
        }
        last = Some(row.y);
    }
    // Empty/evicted ranges have no visible observer either; a future command
    // must establish its own hover instead of inheriting this block's target.
    // A scrolled-out block emits neither ink nor an invisible hit target.
    let (Some(first), Some(last)) = (first, last) else {
        return None;
    };
    // Use placement-derived cell height rather than font ascent or ink bounds.
    // The right edge stays aligned with the command's visible output rectangle.
    Some(Rect {
        origin: [ctx.geometry.grid.origin[0], first],
        size: [
            ctx.geometry.grid.size[0],
            last - first + ctx.geometry.cell_size()[1],
        ],
    })
}
