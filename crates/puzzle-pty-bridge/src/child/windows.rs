//! Blocking child wait with an async completion, never try_wait polling.
//!
//! portable-pty 0.9 WinChild::wait blocks on WaitForSingleObject, and its
//! WinChildKiller uses TerminateProcess on a cloned handle (src/win/mod.rs).
//! Unlike the Unix cloned killer, it needs no HUP grace or signal escalation.
//! The safe killer can interrupt wait from another thread. The
//! child handle remains owned even if starting that wait thread fails, so a
//! spawn failure still signals and reaps it before admission is released.
//! Normal socket cleanup moves this owner to spawn_blocking: joining the
//! native wait cannot stall another session's async socket reads. Runtime
//! correctness on ConPTY remains an M13 check, not a macOS test conclusion.
use portable_pty::{Child, ChildKiller};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// Child lifetime and its one-shot status, separate from FIFO PTY output.
pub(crate) struct ChildOwner {
    /// The wait worker has exclusive child access; the killer needs no lock.
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    /// Terminates the Windows process handle independently of its blocking wait.
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// Joining ensures a session's admission slot outlives actual reaping.
    worker: Option<std::thread::JoinHandle<()>>,
    /// Async status becomes eligible only after the output channel delivers EOF.
    exited: oneshot::Receiver<Result<i32, String>>,
}

/// Every child starts exactly one blocking wait, including silent shells.
impl ChildOwner {
    /// Waits on the native process-handle worker without periodic timers.
    pub(crate) async fn exited(&mut self) -> Result<i32, String> {
        (&mut self.exited)
            .await
            .map_err(|_| "child wait worker stopped")?
    }

    /// Captures a safe killer before moving the child's wait into its worker.
    pub(crate) fn start(child: Box<dyn Child + Send + Sync>, _watch: ()) -> Result<Self, String> {
        let killer = child.clone_killer();
        let child = Arc::new(Mutex::new(child));
        let (done, exited) = oneshot::channel();
        let mut owner = Self {
            child: child.clone(),
            killer,
            worker: None,
            exited,
        };
        // Retaining the Arc in owner makes thread-creation failure recoverable:
        // the closure's handle can drop without orphaning the live child.
        owner.worker = Some(
            std::thread::Builder::new()
                .name("bridge-exit".into())
                .spawn(move || {
                    let status = child
                        .lock()
                        .map_err(|_| "child lock poisoned".to_string())
                        .and_then(|mut child| {
                            child
                                .wait()
                                .map(|s| s.exit_code() as i32)
                                .map_err(|e| e.to_string())
                        });
                    let _ = done.send(status);
                })
                .map_err(|e| e.to_string())?,
        );
        Ok(owner)
    }
}

/// Cleanup never waits for a final browser ACK or an input queue to drain.
impl Drop for ChildOwner {
    /// Signals only a still-running child, then joins its actual wait.
    fn drop(&mut self) {
        // If wait owns the mutex, its cloned killer provides the independent
        // disconnect path. If it finished, avoid signaling a reaped PID.
        let running = match self.child.try_lock() {
            Ok(mut child) => child.try_wait().ok().flatten().is_none(),
            Err(_) => true,
        };
        if running && let Err(error) = self.killer.kill() {
            tracing::warn!(event = "bridge.hangup_failed", %error);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        } else if let Ok(mut child) = self.child.lock() {
            // Only the failed thread-start path has no worker to reap for us.
            let _ = child.wait();
        }
    }
}

/// Windows wait owns the ConPTY process handle; M13 validates this host path.
pub(crate) fn watch() -> Result<(), String> {
    Ok(())
}
