//! RAII event listeners, including non-passive wheel ownership (§12.2).
//!
//! JS keeps a callback alive while registered. Rust must therefore remove
//! the exact callback before releasing its closure, including failed setup.
//! Removing the DOM node alone is insufficient: event targets can remain held
//! by a page, and a callback must never invoke a freed wasm trampoline.
//! Each registration uses the bubbling phase, so removeEventListener's default
//! capture=false matches it exactly; passive does not affect removal identity.
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{AddEventListenerOptions, Event, EventTarget};

/// One registration and its closure, dropped together on terminal disposal.
pub(super) struct Listener {
    /// The same DOM target is needed for removeEventListener.
    target: EventTarget,
    /// Event names are static implementation choices, never external text.
    kind: &'static str,
    /// Rust owns this callback until it has been removed from the target.
    callback: Closure<dyn FnMut(Event)>,
}

/// No forgotten closure remains after a canvas is removed from the document.
impl Listener {
    /// Installs a bubbling listener; passive=false allows consumed wheel defaults.
    pub(super) fn new(
        target: &EventTarget,
        kind: &'static str,
        callback: impl FnMut(Event) + 'static,
    ) -> Result<Self, JsValue> {
        let callback = Closure::wrap(Box::new(callback) as Box<dyn FnMut(Event)>);
        let options = AddEventListenerOptions::new();
        // Explicit non-passivity matters for Chrome's wheel/touch defaults.
        options.set_passive(false);
        target.add_event_listener_with_callback_and_add_event_listener_options(
            kind,
            callback.as_ref().unchecked_ref(),
            &options,
        )?;
        Ok(Self {
            target: target.clone(),
            kind,
            callback,
        })
    }
}

/// Cleanup cannot fail disposal, but an unexpected DOM rejection is observable.
impl Drop for Listener {
    /// Detaches before Closure's destructor releases its wasm trampoline.
    fn drop(&mut self) {
        if self
            .target
            .remove_event_listener_with_callback(self.kind, self.callback.as_ref().unchecked_ref())
            .is_err()
        {
            tracing::warn!(
                event = "web.listener_remove_failed",
                "DOM listener removal failed"
            );
        }
    }
}
