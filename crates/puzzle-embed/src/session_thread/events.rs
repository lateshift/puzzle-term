//! Host results wake the native event loop once per outstanding delivery (D-50).
//!
//! The queue carries values; the latch contains no terminal or drawing state.
//! Clearing before a drain ensures an event arriving during that drain can post
//! a further wake. Clearing after an empty poll could lose that race forever.
//! Finite harnesses use the same inbox without installing a native notification.
use super::*;
use std::sync::atomic::AtomicBool;

/// Only the host owns the result receiver; Session is not reachable through it.
pub struct Events {
    /// Each value is fully owned before its native wake is posted.
    receiver: Receiver<HostEvent>,
    /// True means a native delivery is outstanding, not that VT is locked.
    queued: Arc<AtomicBool>,
}

/// Sending authority can notify without borrowing any native window.
pub(super) struct Signal {
    /// The receiver clears this before consuming its queued values.
    queued: Arc<AtomicBool>,
    /// A rare host capability posts a native wake; it never invokes Session.
    notify: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Events {
    /// Creates the inbox and its paired coalescing notification before worker spawn.
    pub(super) fn pair(
        receiver: Receiver<HostEvent>,
        notify: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> (Self, Signal) {
        let queued = Arc::new(AtomicBool::new(false));
        (
            Self {
                receiver,
                queued: queued.clone(),
            },
            Signal { queued, notify },
        )
    }

    /// Acknowledge before polling so concurrent arrivals can retain their own wake.
    pub fn begin_drain(&self) {
        self.queued.store(false, Ordering::Release);
    }

    /// Moving an event cannot block behind parsing or presentation.
    pub fn try_recv(&self) -> Result<HostEvent, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }
}

impl Signal {
    /// Called after results are queued, never before their policy values are visible.
    pub(super) fn delivered(&self) {
        if let Some(notify) = &self.notify
            && !self.queued.swap(true, Ordering::AcqRel)
        {
            // Multiple facts in one core turn need one toolkit callback, not one each.
            notify();
        }
    }
}
