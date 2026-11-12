//! One clipped, measured UTF-8 field painter for Session's built-in query modes.
//! The shared source owns editing; this helper only places caret and candidate ink.
//!
//! Both fields use natural font advances rather than terminal-cell counts. A
//! candidate is inserted at the UTF-8 caret between committed prefix and suffix;
//! it contributes visible width but never becomes committed query data here.
//! The nested clip is restored before the caller draws its controls or rows.
//! Horizontal translation keeps the insertion endpoint inside the field without
//! changing the source string, selection, or the application's grid cursor.
//! Measurement and painting share size, face and style, including fallback.
//! A combining mark therefore cannot shift the caret by an invented cell width.
//! The one-device-pixel caret is static: neither this helper nor either view owns
//! a timer. Provisional composition receives its own semantic underline instead.
//! Geometry is logical; DPR is applied only for hairline thickness at this seam.
//! The borrowed input can come only from a sealed frame source during building.
//! No rendering callback can insert text or ask Session to schedule a frame.
use super::*;
/// Borrowed committed and provisional query data; no independent editable state.
pub(super) struct Field<'a> {
    /// Visible field purpose.
    pub label: &'a str,
    /// Committed scalar sequence.
    pub text: &'a str,
    /// Provisional IME ink at the insertion boundary.
    pub preedit: &'a str,
    /// UTF-8 byte boundary in text.
    pub cursor: usize,
}
/// Measures the insertion prefix with the same natural layout used to draw it.
pub(super) fn width(ctx: &ViewCtx<'_>, label: &str) -> f32 {
    // Match the emitted TextSpec so fallback and shaping cannot disagree.
    let spec = TextSpec {
        text: label.into(),
        size: 12.0,
        face: None,
        style: 0,
        color: 0xffffffff,
        transform: kurbo::Affine::IDENTITY,
    };
    // Pointer-only callbacks have no font borrow; frame builds always supply one.
    ctx.text
        .and_then(|layout| layout.measure(&spec).ok())
        .map_or(0.0, |m| m.width)
}

/// A horizontally scrolled, clipped line keeps the UTF-8 insertion point visible.
pub(super) fn draw(
    ctx: &ViewCtx<'_>,
    out: &mut DrawList,
    bounds: Rect,
    color: u32,
    state: Field<'_>,
) {
    let field = Rect {
        origin: [bounds.origin[0] + 10.0, bounds.origin[1] + 7.0],
        size: [(bounds.size[0] - 20.0).max(0.0), 22.0],
    };
    // Defensive boundary validation keeps a malformed external snapshot harmless.
    let cursor = state.cursor.min(state.text.len());
    let prefix = state.text.get(..cursor).unwrap_or("");
    let suffix = state.text.get(cursor..).unwrap_or(state.text);
    let before = format!("{}: {prefix}", state.label);
    // Width includes the field label and preceding committed text.
    let advance = width(ctx, &before);
    let candidate = width(ctx, state.preedit);
    // Scroll by natural width, preserving narrow/combining glyph spacing.
    let shift = (advance + candidate + 2.0 - field.size[0]).max(0.0);
    push(out, Primitive::ClipPush(field));
    text(
        out,
        format!("{before}{}{suffix}", state.preedit),
        [field.origin[0] - shift, field.origin[1]],
        color,
    );
    // The solid caret needs no timer; terminal blink remains a separate owner.
    rect(
        out,
        Rect {
            origin: [
                field.origin[0] + advance + candidate - shift,
                field.origin[1],
            ],
            size: [1.0 / ctx.geometry.dpr, 16.0],
        },
        color,
        0.0,
    );
    if !state.preedit.is_empty() {
        // Composition is only provisional ink; committed text alone changes the query.
        rect(
            out,
            Rect {
                origin: [field.origin[0] + advance - shift, field.origin[1] + 17.0],
                size: [candidate, 1.0 / ctx.geometry.dpr],
            },
            ctx.theme.color("preeditUnderline", color),
            0.0,
        );
    }
    push(out, Primitive::ClipPop);
}
