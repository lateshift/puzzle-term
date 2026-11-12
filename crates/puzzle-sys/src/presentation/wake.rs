//! Nonblocking GPU notification with a cold close barrier for host callback data.
//!
//! App free must return after every possible foreign wake invocation. A boolean
//! alone is insufficient: completion could pass its check just before close.
//! Two checks around an active-call count close that race without a callback wait.
//! Only close receives completion tokens; GPU callbacks use nonblocking try_send.
use super::Wake;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};

/// Owned callback remains inert after close, even if native transactions retain it.
pub(super) struct Gate {
    /// User wake only schedules work; it must never wait for a host tick.
    wake: Wake,
    /// Sticky rejection is ordered with activity admission below.
    closed: AtomicBool,
    /// Close waits only for already admitted callback invocations, never GPU work.
    active: AtomicUsize,
    /// One coalesced token is sufficient to recheck the active-call count.
    finished: mpsc::SyncSender<()>,
    /// Only the main-thread close path receives; callbacks never acquire this mutex.
    receive: Mutex<mpsc::Receiver<()>>,
}
impl Gate {
    /// Channels and callback storage are allocated once with the native attachment.
    pub(super) fn new(wake: Wake) -> Self {
        let (finished, receive) = mpsc::sync_channel(1);
        Self {
            wake,
            closed: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            finished,
            receive: Mutex::new(receive),
        }
    }
    /// Metal completion takes no waiting path and never calls a closed host.
    pub(super) fn notify(&self) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        self.active.fetch_add(1, Ordering::SeqCst);
        let _active = Active(self);
        // With sequential consistency, either close observes this activity or
        // this second check observes close. There is no unchecked invocation gap.
        if !self.closed.load(Ordering::SeqCst) {
            (self.wake)();
        }
    }
    /// Cold native teardown is a lifetime barrier, not a rendering timeout.
    pub(super) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let receive = self.receive.lock().unwrap_or_else(|e| e.into_inner());
        while self.active.load(Ordering::SeqCst) != 0 {
            // A last returning call always leaves a token (possibly an older
            // coalesced one). Recheck the count instead of counting notifications.
            if receive.recv().is_err() {
                break;
            }
        }
    }
}
/// Activity is retired even if a Rust host callback unwinds during a test.
struct Active<'a>(&'a Gate);
impl Drop for Active<'_> {
    /// The returning callback never blocks waiting for its close-barrier consumer.
    fn drop(&mut self) {
        if self.0.active.fetch_sub(1, Ordering::SeqCst) == 1 {
            let _ = self.0.finished.try_send(());
        }
    }
}
