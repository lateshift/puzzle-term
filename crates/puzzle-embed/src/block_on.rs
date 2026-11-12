//! A std-only future executor for native GPU initialization (SPEC M0 dependency gate).
//!
//! wgpu initialization is async, but a native window can wait for the adapter
//! without adding a runtime crate. A wake unparks the thread that polls it.
//! `thread::park` remembers an unpark that arrives before the park call, so
//! completion cannot be lost between polling and sleeping.
use std::{
    future::Future,
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
};
/// Wakes the thread parked by the executor when wgpu becomes ready.
struct ThreadWake(thread::Thread);
/// Converts a future wake into a native thread unpark signal.
impl Wake for ThreadWake {
    /// Schedules another poll by unparking the blocked thread.
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    /// Schedules another poll without consuming the wake handle.
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
/// Drives a single future to completion without an async runtime.
/// The pinned future remains in one stack location while wgpu retains wake handles.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        // Poll again only after the adapter request wakes this thread.
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park(),
        }
    }
}
