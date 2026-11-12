//! Textarea listeners separate committed text from key identity (§12.2).
//!
//! The browser owns plain text and paste defaults until a binding consumes a
//! chord. All other paths submit neutral commands immediately; they do not
//! wait for the next frame. Weak callback ownership avoids an element cycle.
//!
//! A printable keydown still crosses the binding matcher: a user may bind a
//! physical key even when it would normally insert a character. If no binding
//! consumes it, the browser supplies committed text through input, not keydown.
//! Dead keys therefore keep their native accent/composition behavior.
//!
//! Provisional composition input can arrive with isComposing already cleared.
//! The transaction state, rather than that single flag, keeps candidates out
//! of stdin until compositionend. One trailing input echo is suppressed.
//! Blur cancels the transaction and clears held Option-side state, because the
//! corresponding key release may be delivered to a different page or window.
//!
//! A paste listener uses only clipboardData from the user action. Preventing
//! the default insertion avoids a second input event carrying the same bytes.
//! This path requires neither clipboard read permission nor an async prompt.
use super::{DomHost, KeyboardState, listener::Listener};
use puzzle_term::messages::{Command, PreeditState};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    ClipboardEvent, CompositionEvent, Event, HtmlTextAreaElement, InputEvent, KeyboardEvent,
};

/// Candidate-window placement and composition event dispatch.
mod ime;

/// Binding decisions while the keydown still has user activation.
mod key;

/// Installs text/focus callbacks on the single editable target.
pub(super) fn install<H: DomHost + 'static>(
    state: &Rc<RefCell<KeyboardState<H>>>,
    textarea: &HtmlTextAreaElement,
) -> Result<Vec<Listener>, JsValue> {
    let mut listeners = Vec::new();
    // Bubbling textarea callbacks cover mobile input without document shortcuts.
    for kind in [
        "keydown",
        "keyup",
        "compositionstart",
        "compositionupdate",
        "compositionend",
        "input",
        "paste",
        "focus",
        "blur",
    ] {
        let weak = Rc::downgrade(state);
        // Holding the area permits clearing input even if the page retains it.
        let area = textarea.clone();
        listeners.push(Listener::new(textarea.as_ref(), kind, move |event| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            let Ok(mut state) = state.try_borrow_mut() else {
                return;
            };
            route(&mut state, &area, &event);
        })?);
    }
    Ok(listeners)
}

/// DOM event families share one transaction state and one command lane.
fn route<H: DomHost>(state: &mut KeyboardState<H>, area: &HtmlTextAreaElement, event: &Event) {
    // Specific event interfaces precede generic focus/blur events.
    if let Some(key) = event.dyn_ref::<KeyboardEvent>() {
        key::dispatch(state, key);
    } else if let Some(composition) = event.dyn_ref::<CompositionEvent>() {
        ime::dispatch(state, area, composition);
    } else if let Some(input) = event.dyn_ref::<InputEvent>() {
        // input.data can be null for replacement/autofill; textarea.value is final.
        let text = input.data().unwrap_or_else(|| area.value());
        let commands = state.composition.input(text, input.is_composing());
        // Composition produces preedit only until its own end commits once.
        for command in commands {
            state.host.command(command);
        }
        if !state.composition.active() {
            // A retained value would be repeated by later null-data insertions.
            area.set_value("");
        }
    } else if let Some(paste) = event.dyn_ref::<ClipboardEvent>() {
        if let Some(data) = paste
            .clipboard_data()
            .and_then(|data| data.get_data("text/plain").ok())
        {
            // Prevent the browser's subsequent insertion, so a paste is typed once.
            paste.prevent_default();
            state.host.command(Command::Paste(data));
            area.set_value("");
        }
    } else if event.type_() == "focus" || event.type_() == "blur" {
        let focused = event.type_() == "focus";
        if !focused {
            // Focus can leave mid-composition; cancellation is never shell input.
            state.keyboard.reset();
            state.composition = super::composition::Composition::default();
            state
                .host
                .command(Command::Preedit(PreeditState::default()));
            area.set_value("");
        }
        state.host.command(Command::Focus(focused));
    }
}
