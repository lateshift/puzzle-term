//! Joined native pipeline lifetime, independent of any host toolkit (M17.embed).
//!
//! Cancellation requests Session/master and writer closure before signalling the
//! tty process group. Only teardown joins. The Session joins first, releasing
//! backpressure; the reader then discards through EOF or its bounded close deadline,
//! drops its master, and reaps. HUP → TERM → KILL runs while reads continue; the
//! final writer join follows so a blocked write has a killed peer and closed master.
//! Rendering is cancelled separately by the App before it releases native views.
//!
//! Host byte sources retain the same cancellation and Session join barrier but
//! have no child or OS IO workers. Taking their producer seals output even if
//! the embedding host still holds a cloned Surface. The owner is woken first,
//! so EOF retention cannot make shutdown depend on those clones being dropped.
use crate::{pty::WriterWorker, session_thread::Handle};
use std::thread::JoinHandle;

/// One session's owned workers, retained from PTY startup through App shutdown.
pub struct Pipeline {
    /// Host output has no blocking OS IO; cancellation closes the fixed-pool producer.
    feed: Option<crate::app::transport::Feed>,
    /// Input priority and held EOF are cancelled without dropping host senders.
    session: Option<Handle>,
    /// Session drops its consumer before the drain is joined.
    owner: Option<JoinHandle<Result<(), String>>>,
    /// The only blocking PTY reader also reaps the child after EOF.
    drain: Option<crate::window::live::Drain>,
    /// Writer queue order remains unchanged during normal operation.
    writer: Option<WriterWorker>,
}

impl Pipeline {
    /// Immediately owns both IO workers, including while Session startup can fail.
    ///
    /// Before attachment, the caller releases its pool consumer on failure. Once
    /// attached, the Session owns that consumer and cancellation releases it here.
    pub fn new(
        _killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
        writer: WriterWorker,
        drain: crate::window::live::Drain,
    ) -> Self {
        Self {
            feed: None,
            session: None,
            owner: None,
            drain: Some(drain),
            writer: Some(writer),
        }
    }

    /// A host source owns the same Session join barrier without child IO workers.
    pub(crate) fn host(feed: crate::app::transport::Feed) -> Self {
        Self {
            feed: Some(feed),
            session: None,
            owner: None,
            drain: None,
            writer: None,
        }
    }

    /// Surface admission clones lifecycle state, never a Session or pool receiver.
    pub(crate) fn host_feed(&self) -> Option<crate::app::transport::Feed> {
        self.feed.clone()
    }

    /// Transfers the sole Session join handle before a surface is registered.
    pub fn attach(&mut self, session: Handle, owner: JoinHandle<Result<(), String>>) {
        // Startup attaches exactly once. Keep any earlier ownership rather than
        // silently detaching a worker if an embedding caller violates that order.
        if self.owner.is_some() {
            session.shutdown();
            if let Err(error) = join_owner(owner) {
                tracing::error!(event = "desktop.session_stopped", %error);
            }
            return;
        }
        // Install both capabilities before rendering or host callbacks can fail.
        self.session = Some(session);
        self.owner = Some(owner);
    }

    /// Wakes every blocking operation before App waits for any worker or GPU turn.
    pub fn cancel(&mut self) {
        if let Some(session) = &self.session {
            session.shutdown();
        }
        if let Some(feed) = &self.feed {
            feed.cancel();
        }
        if let Some(writer) = &self.writer {
            writer.cancel();
        }
        // Close requests precede HUP. The last master is deliberately retained
        // by the drain until EOF or its documented cancellation deadline.
        if let Some(drain) = &self.drain {
            drain.cancel();
        }
    }

    /// Fixtures observe process ownership without exposing PIDs in the host API.
    #[cfg(test)]
    pub(crate) fn drain_pid(&self) -> u32 {
        self.drain.as_ref().map_or(0, |drain| drain.process_id())
    }

    /// Joins all workers; one error cannot skip the remaining join barriers.
    fn join(&mut self) {
        self.cancel();
        if let Some(owner) = self.owner.take()
            && let Err(error) = join_owner(owner)
        {
            tracing::error!(event = "desktop.session_stopped", %error);
        }
        // Session has dropped its master/consumer. Request writer closure before
        // the drain escalates signals; keep reading/discarding while it closes.
        if let Some(drain) = self.drain.take()
            && drain.join().is_err()
        {
            tracing::error!(event = "desktop.drain_panicked");
        }
        // Kill and the ordered sentinel cover blocked writes and idle receives.
        if let Some(writer) = self.writer.take()
            && let Err(error) = writer.join()
        {
            tracing::error!(event = "desktop.writer_stopped", %error);
        }
    }
}

/// Neither startup failure nor a held final screen may detach native workers.
impl Drop for Pipeline {
    /// Host cancellation precedes these cold join barriers (D-50, M16.7).
    fn drop(&mut self) {
        // Taking each handle makes repeated cancellation harmless on error paths.
        self.join();
    }
}

/// A Session error and a panic remain distinguishable without skipping cleanup.
fn join_owner(owner: JoinHandle<Result<(), String>>) -> Result<(), String> {
    owner
        .join()
        .map_err(|_| "Session thread panicked".to_owned())?
}

#[cfg(test)]
/// Real child/flood/held-screen teardown protects the embedding lifetime contract.
pub(crate) mod tests;
