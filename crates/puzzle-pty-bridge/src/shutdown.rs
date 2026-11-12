//! Host-only listener shutdown, using a stored notification rather than polling.
//!
//! notify_one retains a permit even if shutdown races the accept loop's
//! first await. No network message can stop the listener or acquire this handle.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;

/// Cloneable native host signal; stopping admission leaves active cleanup intact.
#[derive(Clone, Default)]
pub struct Shutdown {
    /// The atomic lets a stop before serve return without registering a wait.
    stopped: Arc<AtomicBool>,
    /// One listener consumes the retained wake permit; there are no ticks.
    notify: Arc<Notify>,
}

/// The predicate and retained Notify permit cover both pre- and post-await stops.
impl Shutdown {
    /// Stops the owning listener without an authenticated control endpoint.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.notify.notify_one();
    }
    /// Waits once for shutdown; no finite timeout runs while the listener is idle.
    pub(crate) async fn cancelled(&self) {
        if !self.stopped.load(Ordering::Acquire) {
            self.notify.notified().await;
        }
    }
}
