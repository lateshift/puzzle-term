//! SIGCHLD readiness while retaining the original portable-pty child handle.
//!
//! A cloned Unix killer sends only SIGHUP: unlike Child::kill it has no kill
//! fallback when a shell traps HUP. Keeping the original handle here lets
//! disconnect cleanup escalate and reap even that shell. No wait thread owns
//! the handle, and an idle child creates no periodic exit checks (P2).
use portable_pty::Child;
use tokio::signal::unix::{Signal, SignalKind, signal};

/// Registers before spawn: even an immediately exiting child has a wake stored.
pub(crate) fn watch() -> Result<Signal, String> {
    signal(SignalKind::child()).map_err(|e| format!("watch child exit: {e}"))
}

/// Retained child and readiness notifications, separate from FIFO PTY output.
pub(crate) struct ChildOwner {
    /// Original kill implementation provides SIGHUP grace and forced termination.
    child: Box<dyn Child + Send + Sync>,
    /// Every child signal is a reason to check status, never a clock tick.
    changed: Signal,
}

/// Child status cannot overtake PTY EOF in the connection coordinator.
impl ChildOwner {
    /// Takes an already registered watcher with the newly spawned child.
    pub(crate) fn start(
        child: Box<dyn Child + Send + Sync>,
        changed: Signal,
    ) -> Result<Self, String> {
        Ok(Self { child, changed })
    }

    /// Reaps when ready; cancellation leaves both the handle and signal intact.
    pub(crate) async fn exited(&mut self) -> Result<i32, String> {
        loop {
            // Check before awaiting: exit can precede PTY EOF, and SIGCHLD
            // can coalesce across sessions. The handle identifies our child.
            if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
                return Ok(status.exit_code() as i32);
            }
            // recv is cancellation-safe. A socket close may interrupt it
            // without losing ownership or skipping disconnect cleanup.
            self.changed
                .recv()
                .await
                .ok_or("child signal stream closed")?;
        }
    }
}

/// Cleanup is moved to spawn_blocking before the session admission is released.
impl Drop for ChildOwner {
    /// Signals only a live child and always reaps, including ignored/trapped HUP.
    fn drop(&mut self) {
        // Unlike clone_killer, this original method escalates after its grace
        // period; a hostile shell cannot hold a bridge session slot forever.
        if self.child.try_wait().ok().flatten().is_none()
            && let Err(error) = self.child.kill()
        {
            tracing::warn!(event = "bridge.hangup_failed", %error);
        }
        let _ = self.child.wait();
    }
}
