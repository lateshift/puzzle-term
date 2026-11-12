//! Public-API painters for the M8 scrollbar, pill and desktop query (M9.4).
//!
//! Providers read frozen model data, draw ordinary Top primitives and return
//! typed pointer actions. They own only presentation state, never a viewport,
//! query engine or duplicate result set. Session supplies time and deadlines.
use crate::{
    anchor::Anchor,
    draw::{DrawList, Primitive, RectSpec, TextSpec},
    geometry::Rect,
    view::ViewCtx,
};
use puzzle_frame::ZSlot;
/// Shared logical bounds for fixed-corner and caret-relative floating chrome.
mod placement;
use placement::panel;
/// OSC command gutters, durations and explicit output copy.
pub mod blocks;
/// Effective bindings and exact leader continuations.
pub mod help;
/// Inline candidate and suggestion text use the public cursor anchor.
pub mod inline;
/// Grouped hyperlinks and a finite delayed URL tooltip.
pub mod links;
/// New-output state and click-to-follow presentation.
pub mod pill;
/// Shared Unicode query-field rendering for both Session input modes.
mod query_field;
/// Scrollbar hover, finite fade and drag presentation.
pub mod scrollbar;
/// Session-owned query text, IME candidate, count and navigation buttons.
pub mod search;
/// Finite bell scheduling and OSC progress presentation.
pub mod status;
/// Fixed surface coordinates already include their own padding.
fn push(out: &mut DrawList, primitive: Primitive) {
    // A zero viewport anchor leaves the sampled surface coordinates unchanged.
    out.push(Anchor::Viewport(Rect::default()), ZSlot::Top, primitive);
}
/// Panels use the common rectangle pipeline's rounded SDF.
fn rect(out: &mut DrawList, bounds: Rect, color: u32, radius: f32) {
    // Rounded corners are analytic in the existing rectangle shader.
    // Stroke zero denotes a filled panel rather than an outline.
    push(
        out,
        Primitive::Rect(RectSpec {
            rect: bounds,
            color,
            radius,
            stroke: 0.0,
        }),
    );
}
/// Labels share grid faces and natural fallback, with no private font owner.
fn text(out: &mut DrawList, label: String, point: [f32; 2], color: u32) {
    // Free text uses a natural advance instead of terminal cell spacing.
    // The transform starts at the line top; the shared compiler adds its baseline.
    push(
        out,
        Primitive::Text(TextSpec {
            text: label,
            size: 12.0,
            face: None,
            style: 0,
            color,
            transform: kurbo::Affine::translate((f64::from(point[0]), f64::from(point[1]))),
        }),
    );
}
/// Fade only changes straight-alpha ink, preserving each theme token's alpha.
fn alpha(color: u32, opacity: f32) -> u32 {
    let mut bytes = color.to_le_bytes();
    // RGB remains straight; the shared GPU shader premultiplies after coverage.
    bytes[3] = (f32::from(bytes[3]) * opacity.clamp(0.0, 1.0)).round() as u8;
    u32::from_le_bytes(bytes)
}
/// Opt-in presentation statistics without a self-sustaining refresh loop.
pub mod hud;

/// Cross-platform harness notification presentation and click reports.
pub mod toast;
