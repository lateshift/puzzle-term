//! Device callbacks publish loss without borrowing a surface or a Session.
//!
//! Replacement remains serialized on the render owner; callbacks only mark a
//! loss fact and post a nonblocking wake to its host notification endpoint.
use super::host::HostView;
use puzzle_render::Renderer;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Registers a notification without moving GPU ownership onto wgpu's callback thread.
pub fn watch(renderer: &Renderer, host: HostView) -> Arc<AtomicBool> {
    // A callback thread only publishes the loss fact; it cannot mutate the owner.
    let lost = Arc::new(AtomicBool::new(false));
    // Keep the notification alive even while the actor replaces its old device.
    let notified = Arc::clone(&lost);
    renderer
        .device()
        .set_device_lost_callback(move |reason, message| {
            tracing::warn!(event = "desktop.device_lost", ?reason, %message);
            // Release pairs with the actor’s Acquire before any recovery work begins.
            notified.store(true, Ordering::Release);
            // Sleeping actors recover without requiring another PTY byte.
            host.request_redraw();
        });
    lost
}
