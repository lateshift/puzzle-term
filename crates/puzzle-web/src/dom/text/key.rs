//! Binding ownership before textarea insertion and terminal key encoding.
//!
//! Printable keys must still visit the shared matcher: physical bindings can
//! override text. The host's disposition keeps native insertion and paste in
//! the browser, while consumed keys cancel the default action.
//!
//! Keyboard lock is the only condition that makes browser chrome chords
//! eligible. Ordinary pages leave them untouched; validation reports any user
//! binding that cannot reach this handler outside fullscreen lock.
//!
//! Copy must call writeText during the key callback. Waiting for a frame would
//! lose the browser user activation even though the eventual promise is async.
//! Rejection is reported without logging private clipboard contents.
use crate::dom::{DomHost, KeyDisposition, KeyboardState, keyboard};
use wasm_bindgen_futures::JsFuture;
/// Rich MIME construction shares the user-activation clipboard boundary.
mod clipboard;
use web_sys::KeyboardEvent;

/// Bindings run before native text ownership, but never during composition.
pub(crate) fn dispatch<H: DomHost>(state: &mut KeyboardState<H>, key: &KeyboardEvent) {
    if state.composition.active() {
        return;
    }
    state.composition.new_key();
    let mut policy = state.host.key_policy();
    // A host may also represent an embedded browser with its own shortcut policy.
    policy.locked |= state.locked;
    let Some(mapped) = state.keyboard.map(key, policy) else {
        return;
    };
    if !policy.locked
        && keyboard::reserved(mapped.event.key, mapped.event.modifiers, policy.platform)
    {
        // A page cannot steal browser chrome commands without keyboard lock.
        return;
    }
    match state.host.key(mapped) {
        KeyDisposition::Consumed => key.prevent_default(),
        KeyDisposition::Text | KeyDisposition::Paste => {}
        // The pair carries one accepted selection into both clipboard MIME types.
        KeyDisposition::CopyHtml { html, text } => {
            if let Some(window) = web_sys::window() {
                // Construct and invoke write synchronously, before awaiting its promise.
                match clipboard::item(&html, &text) {
                    Ok(item) => clipboard::observe(
                        window
                            .navigator()
                            .clipboard()
                            .write(&js_sys::Array::of1(&item)),
                    ),
                    Err(_) => {
                        // If rich construction is unavailable, retain ordinary paste text.
                        // Never pass the HTML source string to writeText.
                        clipboard::observe(window.navigator().clipboard().write_text(&text));
                    }
                }
                key.prevent_default();
            }
        }
        KeyDisposition::Copy(text) => {
            if let Some(window) = web_sys::window() {
                // Calling synchronously retains this keydown's user activation.
                let promise = window.navigator().clipboard().write_text(&text);
                wasm_bindgen_futures::spawn_local(async move {
                    if JsFuture::from(promise).await.is_err() {
                        tracing::warn!(
                            event = "web.clipboard_write_failed",
                            "Clipboard write rejected"
                        );
                    }
                });
                key.prevent_default();
            }
        }
    }
}
