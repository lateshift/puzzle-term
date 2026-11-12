//! One joined App decoder with one active job and one bounded handoff slot.
//! A full queue retains bytes on their Session owner. Releasing the slot wakes
//! waiting owners through priority controls, without polling or dropping data.
use super::*;
use crate::session_thread::DecodeEndpoint;
use std::{
    sync::{
        Mutex,
        mpsc::{self, SyncSender, TrySendError},
    },
    thread,
};

/// Complete source ownership and a weak route back to its still-live Session.
struct Job {
    /// Compressed/encoded bytes are already bounded by that Session's allowance.
    request: DecodeRequest,
    /// A worker must never keep a closed surface alive to deliver obsolete pixels.
    reply: DecodeEndpoint,
}

/// Cloneable submission capability; the receiving worker belongs to App alone.
#[derive(Clone)]
pub struct Sender {
    /// Capacity one represents a single handoff in addition to the running decode.
    queue: SyncSender<Job>,
    /// Waiting Sessions hold their own bytes; only weak reply routes accumulate here.
    waiters: Arc<Mutex<Vec<DecodeEndpoint>>>,
}

/// Joined lifetime is a cold App resource, never a detached background thread.
pub(crate) struct Worker {
    /// Removing the final sender ends an idle recv after Sessions have joined.
    sender: Option<Sender>,
    /// Every accepted job completes or is discarded before App destruction returns.
    thread: Option<thread::JoinHandle<()>>,
}

/// Worker creation is an App lifetime operation, never a per-surface decode spawn.
impl Worker {
    /// Starts a sleeping decoder; it does no work until an image transmission arrives.
    pub(crate) fn start() -> Result<Self, String> {
        // One running job plus one handoff bounds native work independently of
        // the number of Sessions. Each waiting Session retains its own request.
        let (queue, receiver) = mpsc::sync_channel::<Job>(1);
        let waiters = Arc::new(Mutex::new(Vec::<DecodeEndpoint>::new()));
        let wake = Arc::clone(&waiters);
        let thread = thread::Builder::new()
            .name("puzzle-image-decode".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    // Locking across try_send/registration prevents a lost availability wake.
                    if let Ok(mut waiters) = wake.lock() {
                        for waiter in waiters.drain(..) {
                            waiter.ready();
                        }
                    }
                    // The completion carries immutable pixels or an error. The
                    // worker cannot mutate Session, publish a frame, or call UI.
                    job.reply.complete(decode(job.request));
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            sender: Some(Sender { queue, waiters }),
            thread: Some(thread),
        })
    }

    /// A Session borrows the App-owned worker through a cheap sending value.
    pub(crate) fn sender(&self) -> Option<Sender> {
        // Only sending authority is cloned. The JoinHandle and receiver never
        // leave App, so hosts cannot detach or run another decoding loop.
        self.sender.clone()
    }
}

/// Only admission and readiness share this cold, short mutex-protected registry.
impl Sender {
    /// Returns a full-queue source to its original owner and registers one readiness wake.
    pub(crate) fn submit(
        &self,
        request: DecodeRequest,
        reply: DecodeEndpoint,
    ) -> Result<Option<DecodeRequest>, String> {
        // Registration and try_send share the availability lock with recv's
        // readiness drain, preventing a queue release from being lost between them.
        let mut waiters = self
            .waiters
            .lock()
            .map_err(|_| "image decode waiters poisoned")?;
        match self.queue.try_send(Job {
            request,
            reply: reply.clone(),
        }) {
            // An accepted source has exactly one owner in the queue/worker;
            // the Session keeps only its small ticket and original control data.
            Ok(()) => Ok(None),
            Err(TrySendError::Full(job)) => {
                // Store only a weak route, not another copy of a potentially large
                // image. The owner retries once after this readiness notification.
                waiters.push(reply);
                Ok(Some(job.request))
            }
            Err(TrySendError::Disconnected(_)) => Err("image decode worker disconnected".into()),
        }
    }
}

/// Teardown is synchronous: no background decoder outlives its App-owned GPU/UI.
impl Drop for Worker {
    /// App drops Session pipelines first, so no producer can retain this channel indefinitely.
    fn drop(&mut self) {
        // App teardown has already joined all Session owners, dropping their
        // Sender clones. Removing this last producer wakes an idle receiver.
        self.sender.take();
        if let Some(worker) = self.thread.take()
            && worker.join().is_err()
        {
            tracing::error!(event = "images.decode_worker_panicked");
        }
    }
}
