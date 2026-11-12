//! Native-target release acknowledgement for asynchronous surface close.
//!
//! C hosts cannot give wgpu an owning Arc of their toolkit view. Closing the
//! endpoint only queues actor removal; an in-flight frame may still use the
//! target. This wrapper drops the entire driver before posting its completion.
//! The host keeps its view alive until that event, or until App drop has joined
//! the actor. No callback ever runs on the render worker, including teardown.
//!
//! Two completion mechanisms serve different host lifetimes. The receiver is
//! a cold synchronous barrier for toolkits that destroy a layer as soon as their
//! close callback returns (D-56). The queued event serves asynchronous owners,
//! including the harness, without forcing a render wait on ordinary window input.
//! Both follow destruction of the inner driver, so neither merely acknowledges
//! receipt of a removal command while a drawable still borrows the native view.
//! The actor never waits for the receiver. An unbounded standard channel owns
//! the one completion value and also wakes a waiting receiver if teardown fails.
//! Wakeup remains the host's nonblocking event-loop notification, not UI work.
use super::{Posted, SurfaceId};
use crate::render_thread::{Driver, resources::Resources, surface::Pending};
use std::{
    sync::{Arc, mpsc},
    time::Instant,
};

/// Actor-owned barrier between invalidating an endpoint and releasing its view.
pub(super) struct ReleasedDriver {
    /// Option permits explicit destruction before the completion notification.
    inner: Option<Box<dyn Driver>>,
    /// Stable host identity remains useful after the input capability closes.
    id: SurfaceId,
    /// Owned completion crosses to App::tick without touching native UI.
    posts: mpsc::Sender<Posted>,
    /// The only worker-callable host entry point wakes an idle event loop.
    wake: Arc<dyn Fn() + Send + Sync>,
    /// Cold native teardown can wait for driver destruction without running App::tick.
    released: mpsc::Sender<()>,
}
impl ReleasedDriver {
    /// Transfers the existing driver without changing its scheduling or resources.
    pub(super) fn wrap(
        id: SurfaceId,
        inner: Box<dyn Driver>,
        posts: mpsc::Sender<Posted>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> (Box<dyn Driver>, mpsc::Receiver<()>) {
        let (released, completion) = mpsc::channel();
        (
            Box::new(Self {
                inner: Some(inner),
                id,
                posts,
                wake,
                released,
            }),
            completion,
        )
    }
}
impl Driver for ReleasedDriver {
    /// Lifecycle wrapping preserves the in-flight display-crossing cancellation gate.
    fn attach_updates(&mut self, updates: std::sync::Weak<std::sync::Mutex<Pending>>) {
        if let Some(inner) = &mut self.inner {
            inner.attach_updates(updates);
        }
    }
    /// Ordinary drivers retain their existing timer path unchanged.
    fn advance(&mut self, pending: Pending, now: Instant) -> Option<Instant> {
        self.inner.as_mut()?.advance(pending, now)
    }
    /// Shared font/GPU ownership stays with the process render actor.
    fn advance_shared(
        &mut self,
        pending: Pending,
        now: Instant,
        resources: &mut Resources,
    ) -> Option<Instant> {
        self.inner.as_mut()?.advance_shared(pending, now, resources)
    }
    /// Wrapping lifecycle must not disconnect the native display-link seam.
    #[cfg(target_os = "macos")]
    fn refresh_display(&self, pending: &Pending) -> Option<u32> {
        self.inner.as_ref()?.refresh_display(pending)
    }
}
impl Drop for ReleasedDriver {
    /// Field drop order alone would send too early; destroy the target explicitly.
    fn drop(&mut self) {
        drop(self.inner.take());
        // D-56: completion means the native surface itself has already been destroyed.
        let _ = self.released.send(());
        let _ = self.posts.send(Posted::Released(self.id));
        (self.wake)();
    }
}
