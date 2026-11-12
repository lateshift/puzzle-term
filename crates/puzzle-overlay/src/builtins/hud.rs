//! Debug statistics are an opt-in observer, displayed through the public API.
//!
//! The Session installs/removes the observer when the HUD is shown/hidden. This
//! painter sees only the previous submitted frame's copied counters. It requests
//! no animation: statistics never generate more frames merely to measure them.
//!
//! Native coverage moves the paper and clip together, preserving label containment.
use super::*;
use crate::{
    model::{ModelCtx, OverlayModel, SharedState, ViewState, state_pool::StatePool},
    provider::EventKinds,
    view::OverlayView,
};
/// Visibility is Session-owned; this model only versions frozen source changes.
#[derive(Default)]
pub struct Hud {
    /// Unit payloads need no per-frame allocation after publication warming.
    states: StatePool<()>,
}
impl OverlayModel for Hud {
    /// Stable name for resource and callback diagnostics.
    fn name(&self) -> &'static str {
        "performance-hud"
    }
    /// Statistics updates and appearance changes invalidate the retained draw list.
    fn wants(&self) -> EventKinds {
        EventKinds(EventKinds::STATUS.0 | EventKinds::GEOMETRY.0 | EventKinds::THEME.0)
    }
    /// The owner already supplied visibility and completed-frame measurements.
    fn update(&mut self, _: &ModelCtx<'_>) {}
    /// Unit is enough because counters live in the frozen source payload.
    fn view_state(&mut self) -> SharedState {
        self.states.publish(&())
    }
}
/// No clock, observer registration or wake method is accessible from drawing.
pub struct HudView;
impl OverlayView for HudView {
    /// A hidden HUD emits nothing; it cannot request ongoing measurement frames.
    fn build(&mut self, _: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        // Source counters belong to the sealed publication, never to a live Session.
        let hud = &ctx.models.hud;
        // Retain an empty list when hidden; even panel/text preparation is unnecessary.
        if !hud.shown {
            return;
        }
        // The shared panel helper anchors to the logical viewport and clamps small windows.
        let Some(bounds) = panel(ctx, 370.0, 66.0) else {
            return;
        };
        // Semantic tooltip tokens keep the debug panel legible in both appearances.
        rect(
            out,
            bounds,
            ctx.theme.color("tooltipBackground", 0xff202020),
            4.0,
        );
        // Clip long numbers instead of letting debug labels obscure neighboring content.
        push(out, Primitive::ClipPush(bounds));
        // Formatting occurs only on dirty view rebuilds; an idle HUD never reaches here.
        let labels = [
            // Build time excludes drawable acquisition and GPU execution.
            format!(
                "CPU {:.3} ms | parse {:.1} MiB/s",
                hud.cpu_ms,
                // Binary MiB matches the native Unicode throughput diagnostics.
                hud.parse_bytes_per_second / 1_048_576.0
            ),
            // Show all three pipeline populations, including rasterized grid glyphs.
            format!(
                "instances rect {} / vector {} / raster {}",
                hud.instances[0], hud.instances[1], hud.instances[2]
            ),
            // Capacity includes both atlas kinds; uploads describe this submission only.
            format!(
                "glyphs {:.1} MiB | uploads {} B",
                hud.glyph_bytes as f64 / 1_048_576.0,
                hud.upload_bytes
            ),
        ];
        // The three baselines share the normal free-text layouter and Slug renderer.
        for (index, label) in labels.into_iter().enumerate() {
            // Coordinates and line spacing are logical pixels; DPR scaling happens later.
            text(
                out,
                label,
                [
                    bounds.origin[0] + 8.0,
                    bounds.origin[1] + 5.0 + index as f32 * 18.0,
                ],
                // Use the paired foreground token, not terminal palette assumptions.
                ctx.theme.color("tooltipForeground", 0xffffffff),
            );
        }
        // Restore the parent scissor so subsequent Top-slot overlays remain independent.
        push(out, Primitive::ClipPop);
    }
}
