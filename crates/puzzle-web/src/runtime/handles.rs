//! Disposable subscriptions and socket generations never retain their terminal.
use super::*;

/// Removing a subscription does not retain the terminal it came from.
#[wasm_bindgen]
pub struct Disposable {
    /// Weak owner also makes disposal harmless after terminal disposal.
    pub(super) owner: std::rc::Weak<RefCell<Runtime>>,
    /// Registration identity applies only to the resource it originally created.
    pub(super) id: u64,
}
/// Idempotent subscription removal; old handles cannot remove replacements.
#[wasm_bindgen]
impl Disposable {
    /// Stops this callback or attached transport, without closing the terminal.
    pub fn dispose(&self) {
        // If the terminal was released, the weak handle becomes an idempotent no-op.
        if let Some(owner) = self.owner.upgrade() {
            let transport = {
                let mut r = owner.borrow_mut();
                // Each event subscriber has a separate identity from its shared bus observer.
                r.events.retain(|(id, _, _)| *id != self.id);
                // Removing one stdin observer must leave the other transport consumers intact.
                r.data.retain(|(id, _)| *id != self.id);
                // Old handles cannot close a host installed after their registration.
                if r.transport_id == self.id {
                    r.transport.take()
                } else {
                    None
                }
            };
            // A foreign cleanup can throw; expose it through the existing error event.
            if let Err(error) = output::close_transport(transport) {
                events::emit(&owner, "error", error);
            }
        }
    }
}

/// One connection handle cannot disconnect a newer replacement connection.
#[wasm_bindgen]
pub struct Connection {
    /// Weak identity avoids retaining a disposed terminal through the host UI.
    pub(super) owner: std::rc::Weak<RefCell<Runtime>>,
    /// The same generation is checked by incoming socket callbacks.
    pub(super) id: u64,
}
/// Disconnecting tears down event handlers and asks the bridge to clean its child.
#[wasm_bindgen]
impl Connection {
    /// Closes only this handle's socket, with no effect on a later reconnect.
    pub fn disconnect(&self) {
        // If the terminal was released, the weak handle becomes an idempotent no-op.
        if let Some(owner) = self.owner.upgrade() {
            // Reconnect creates another generation, so a stale handle cannot close it.
            let mut r = owner.borrow_mut();
            if r.socket.as_ref().is_some_and(|s| s.id == self.id) {
                r.socket = None;
            }
        }
    }
}
