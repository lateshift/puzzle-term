//! Fractional application wheel steps, shared by desktop and web (§7.4).
//!
//! Trackpads provide pixels while protocols accept whole rows. Retain the
//! remainder between events, but never carry it across a screen, destination
//! or cell-height change: half a mouse report must not become half a key.
//! Horizontal reports also carry axis identity so their fractions cannot
//! contribute a later vertical step. Both axes use device-cell height as
//! the existing protocol step unit; local history accepts vertical input only.
//! The destination query is shared with browser default ownership: it reads
//! modes without mutating this ledger or allocating encoded output bytes.
//! Shift is an unconditional local-wheel override even if shiftSelects is off.
//! Mode 1007 affects vertical alternate-screen input only; sideways input
//! remains a mouse report or returns to the embedding page. The terminal's
//! live mode snapshot determines the destination after interceptors run.
use crate::{
    viewport::ScrollInput,
    vt::{
        Terminal,
        input::{InputModes, Modifiers, MouseEncoding, MouseEvent, MouseModes, MouseTracking},
        terminal::ActiveScreen,
    },
};
/// Destination identity prevents gesture fragments crossing application modes.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Delivery {
    /// Reports retain tracking, encoding and axis so fractions cannot cross axes.
    Mouse(MouseTracking, MouseEncoding, bool),
    /// Arrow form is selected by the current DECCKM state.
    Keys(bool),
}
/// One bounded fractional row ledger; local scrolling uses the viewport instead.
#[derive(Default)]
pub(super) struct Steps {
    /// Mode/screen/geometry identity attached to the retained fraction.
    context: Option<(u64, Delivery, u32)>,
    /// Signed row fraction; positive goes down toward newer output.
    remainder: f64,
}
/// Input conversion allocates nothing and never reads a platform clock.
impl Steps {
    /// Returns signed whole steps, keeping sub-row pixel input for the next event.
    pub(super) fn take(
        &mut self,
        input: ScrollInput,
        height: u32,
        epoch: u64,
        route: Delivery,
    ) -> i32 {
        // Reflow/DPR and mode switches cannot reinterpret an old physical fraction.
        let context = (epoch, route, height.max(1));
        if self.context != Some(context) {
            self.clear();
            self.context = Some(context);
        }
        // Convert pixels once in the same device space as the visible text cells.
        let rows = match input {
            ScrollInput::Pixels(pixels) => pixels / f64::from(height.max(1)),
            ScrollInput::Rows(rows) => rows,
        };
        // Broken driver values cannot manufacture an unbounded protocol-report loop.
        if !rows.is_finite() {
            return 0;
        }
        self.remainder = (self.remainder + rows).clamp(-64.0, 64.0);
        // Truncation preserves both direction and the unconsumed sub-row remainder.
        let steps = self.remainder.trunc() as i32;
        self.remainder -= f64::from(steps);
        steps
    }
    /// Local Shift gestures cannot later contribute fractional application input.
    pub(super) fn clear(&mut self) {
        self.context = None;
        self.remainder = 0.0;
    }
}

/// Chooses the live destination once for dispatch and browser-default ownership.
/// Shift always returns wheel input to local history, independently of selection.
pub(super) fn delivery(term: &Terminal, pointer: MouseEvent) -> Option<Delivery> {
    let mouse = MouseModes::from_terminal(term);
    if pointer.modifiers.has(Modifiers::SHIFT) {
        None
    } else if mouse.tracking != MouseTracking::None {
        // Application reporting takes priority over alternate-screen arrows.
        Some(Delivery::Mouse(
            mouse.tracking,
            mouse.encoding,
            pointer.button >= 66,
        ))
    } else if pointer.button < 66
        && term.active == ActiveScreen::Alternate
        && term.modes.get(true, 1007) == Some(true)
    {
        Some(Delivery::Keys(
            InputModes::from_terminal(term).application_cursor,
        ))
    } else {
        None
    }
}
