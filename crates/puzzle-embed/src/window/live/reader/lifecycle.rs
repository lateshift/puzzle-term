//! A native child's PID stays owned until EOF and reaping, including cancellation.
//!
//! The reader keeps consuming the master after its Session disappears. A cold
//! Pipeline join closes Session/master and requests writer closure first, then
//! escalates HUP → TERM → KILL while that reader continues to EOF. No worker is
//! detached. A short lifecycle gate serializes signalling with try_wait so a
//! reaped/reused PID can never be signalled. It contains no Session or hot data.
//!
//! EOF is a tty lifetime boundary, not a child-state notification: inherited
//! slaves can outlive the shell. The cancellable reader bounds that lifetime.
//! Conversely, a child can close stdout and remain alive; after EOF the cold
//! reaper must stay interruptible by host escalation rather than call wait4.
//! Completion therefore follows both descriptor destruction and child reaping.
//! Keeping that distinction here avoids detached signal/watchdog threads and
//! allows the App's ordinary joined shutdown to cover startup failures too.
use std::{
    io,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

/// Grace is for a child to react to a shutdown signal, never a performance budget.
const SIGNAL_GRACE: Duration = Duration::from_millis(250);
/// After EOF only, observe termination without holding the signal gate in wait4.
const REAP_POLL: Duration = Duration::from_millis(10);

/// Shared process identity is invalidated under the same gate that sends signals.
pub(super) struct State {
    /// Native wait ownership survives until its reader has closed at EOF/EIO.
    child: Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>,
    /// Completion acknowledges reaping, not just a shutdown request.
    done: mpsc::Sender<()>,
}
impl State {
    /// Installs the process before its first blocking read can run.
    pub(super) fn new(
        child: Box<dyn portable_pty::Child + Send + Sync>,
    ) -> (Arc<Self>, mpsc::Receiver<()>) {
        // Completion belongs to the drain; neither UI tick nor render progress
        // is needed to tell the cold joiner that process ownership has ended.
        let (done, completion) = mpsc::channel();
        (
            Arc::new(Self {
                child: Mutex::new(Some(child)),
                done,
            }),
            completion,
        )
    }

    /// Called only AFTER the final master read and descriptor destruction.
    pub(super) fn reap(&self) -> io::Result<portable_pty::ExitStatus> {
        loop {
            {
                // try_wait is nonblocking. Release this gate before any sleep so
                // the host can signal a child which closed its tty before exiting.
                let mut guard = self.child.lock().unwrap_or_else(|error| error.into_inner());
                let child = guard
                    .as_mut()
                    .ok_or_else(|| io::Error::other("child already reaped"))?;
                match child.try_wait() {
                    Ok(Some(status)) => {
                        // This atomic ownership transition closes the PID-reuse race.
                        guard.take();
                        return Ok(status);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        // A failed wait cannot preserve a trustworthy signalling
                        // capability. Retire it before reporting unknown exit status.
                        guard.take();
                        return Err(error);
                    }
                }
            }
            // Only native EOF reaches this cold loop. Blocking wait4 would prevent
            // a concurrent shutdown from escalating a child that closed stdout early.
            thread::sleep(REAP_POLL);
        }
    }

    /// No host callback participates in completion or its bounded grace waits.
    pub(super) fn complete(&self) {
        let _ = self.done.send(());
    }

    /// A still-owned PID cannot be recycled while the signal gate is held.
    fn signal(&self, stage: u8) {
        // Reaping invalidates this same option before the OS can reuse its PID.
        let mut guard = self.child.lock().unwrap_or_else(|error| error.into_inner());
        let Some(child) = guard.as_mut() else { return };
        #[cfg(unix)]
        if let Some(pid) = child
            .process_id()
            .and_then(|pid| i32::try_from(pid).ok())
            .and_then(rustix::process::Pid::from_raw)
        {
            use rustix::process::{Signal, kill_process, kill_process_group};
            // Each escalation targets the full inherited-tty group, including
            // background jobs after their noninteractive shell already exited.
            let signal = match stage {
                0 => Signal::HUP,
                1 => Signal::TERM,
                _ => Signal::KILL,
            };
            // portable-pty starts a controlling-tty session/group. Signal that group
            // so a shell's foreground writer does not retain the slave after close.
            if kill_process_group(pid, signal).is_err()
                && let Err(error) = kill_process(pid, signal)
            {
                tracing::debug!(event="desktop.shutdown_signal_failed", %error, stage);
            }
        }
        #[cfg(not(unix))]
        {
            // ConPTY keeps portable-pty's existing native process killer; Unix
            // descriptor polling and POSIX group identities never enter this path.
            let _ = stage;
            let _ = child.clone_killer().kill();
        }
    }
}

/// Reader and child share one join barrier; ordinary cancellation never detaches it.
pub struct Drain {
    /// All reads finish before this worker reaps its process.
    worker: JoinHandle<()>,
    /// Signals may run while the reader drains, but never after PID reaping.
    state: Arc<State>,
    /// Signal grace sleeps end early when reaping actually completes.
    completion: mpsc::Receiver<()>,
}
impl Drain {
    /// Binds the worker and process gate before returning startup ownership.
    pub(super) fn new(
        worker: JoinHandle<()>,
        state: Arc<State>,
        completion: mpsc::Receiver<()>,
    ) -> Self {
        Self {
            worker,
            state,
            completion,
        }
    }

    /// Fixtures assert that no child or process-group descendant survives teardown.
    #[cfg(test)]
    pub(crate) fn process_id(&self) -> u32 {
        self.state
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|child| child.process_id())
            .unwrap_or(0)
    }

    /// HUP is nonblocking and the drain keeps reading even after Session cancellation.
    pub(crate) fn cancel(&self) {
        self.state.signal(0);
    }

    /// Bounded grace for HUP and TERM; KILL is followed by the mandatory worker join.
    pub(crate) fn join(self) -> thread::Result<()> {
        for stage in 0..=2 {
            // Completion can win between signals. Never send a later signal after
            // the worker has relinquished child identity and exited.
            if self.worker.is_finished() {
                break;
            }
            self.state.signal(stage);
            // A completion message ends the grace early; timeout only escalates,
            // never claims that worker teardown or child reaping succeeded.
            if self.completion.recv_timeout(SIGNAL_GRACE).is_ok() {
                break;
            }
        }
        // Never replace this with a detached worker or a successful timeout return:
        // native ownership and child reaping are part of surface/app free.
        self.worker.join()
    }
}
