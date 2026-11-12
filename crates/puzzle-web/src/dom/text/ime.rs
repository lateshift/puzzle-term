//! Native candidate-window placement and composition transaction dispatch.
//!
//! The browser may update the provisional value between frames. Sampling the
//! host's current cursor here anchors the OS candidate popup without rendering
//! or treating the candidate string as terminal input.
//!
//! End clears the UI state before committing text, including cancellation.
//! The platform's UTF-16 selection offset is converted by Composition into
//! a checked UTF-8 caret; missing offsets remain absent rather than guessed.
//! CSS placement divides device-pixel geometry by DPR exactly once.
use crate::dom::{DomHost, KeyboardState};
use web_sys::{CompositionEvent, HtmlTextAreaElement};

/// Preedit is UI state; compositionend clears it before exactly one Text.
pub(crate) fn dispatch<H: DomHost>(
    state: &mut KeyboardState<H>,
    area: &HtmlTextAreaElement,
    event: &CompositionEvent,
) {
    let text = event.data().unwrap_or_default();
    // Missing composition data represents cancellation, not a malformed key.
    let commands = match event.type_().as_str() {
        "compositionstart" => vec![state.composition.start()],
        "compositionupdate" => vec![
            state
                .composition
                .update(text, area.selection_start().ok().flatten()),
        ],
        "compositionend" => state.composition.end(text),
        _ => Vec::new(),
    };
    for command in commands {
        state.host.command(command);
    }
    if !state.composition.active() {
        area.set_value("");
    }
    // The runtime calls anchor after layout too; this path covers a composition
    // starting between frames while the cell cursor is already up to date.
    let geometry = state.host.geometry();
    let dpr = geometry.dpr.max(f64::EPSILON);
    let _ = area
        .style()
        .set_property("left", &format!("{}px", geometry.cursor[0] / dpr));
    let _ = area
        .style()
        .set_property("top", &format!("{}px", geometry.cursor[1] / dpr));
}
