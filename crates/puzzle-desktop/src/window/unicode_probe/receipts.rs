//! One probe owns its receipt files and the lifetime of its input timer.
//!
//! Earlier probes deleted a process-global key.txt at launch. A later window
//! could erase a failed run, while a detached timer from a closed window could
//! post keys to a subsequent host. Atomic directory reservation separates runs;
//! disconnecting the timer's cancellation channel ends its remaining schedule.
//! Files survive drop so a failed exact count remains diagnosable.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

/// Probe-local evidence and cancellation authority, never terminal ownership.
pub(super) struct Receipts {
    /// The child receives this exact path as argv, including its private directory.
    pub(super) path: PathBuf,
    /// Dropping the sender wakes a timer even while its next sample is far away.
    cancel: Option<mpsc::Sender<()>>,
}

impl Receipts {
    /// Reserves a directory atomically; concurrent probes cannot share a journal.
    ///
    /// A process ID alone is insufficient because a process can open two probes
    /// and the OS can later reuse an ID. Existing directories are never deleted.
    /// Filesystem failures reject setup rather than falling back to shared paths.
    pub(super) fn new(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
        // create_dir is the reservation: checking existence first would race
        // another probe before either one creates its child receipt file.
        for ordinal in 0u64.. {
            let directory = root.join(format!("{}-{ordinal}", std::process::id()));
            match fs::create_dir(&directory) {
                Ok(()) => {
                    // The child creates key.txt; owning the directory is enough
                    // to guarantee its path cannot belong to a later probe.
                    return Ok(Self {
                        path: directory.join("key.txt"),
                        cancel: None,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("receipt directory identities exhausted".into())
    }

    /// Installs one timer; replacing it cancels any previous schedule first.
    ///
    /// A disconnected channel wakes recv_timeout without waiting for the next
    /// scheduled key. This authority belongs to the probe, not a detached host.
    pub(super) fn timer(&mut self) -> mpsc::Receiver<()> {
        let (cancel, receiver) = mpsc::channel();
        self.cancel = Some(cancel);
        receiver
    }

    /// Keeps the parent's complete timeline even if the child journal is missing.
    pub(super) fn save_issued(&self, sent: &[u128]) -> Result<(), String> {
        // This runs at observation completion, outside all timed input deliveries.
        let bytes = serde_json::to_vec(sent).map_err(|error| error.to_string())?;
        fs::write(self.path.with_extension("issued.json"), bytes).map_err(|error| error.to_string())
    }

    /// Reports an exact-count failure with both histories and its retained path.
    pub(super) fn pair(
        &self,
        sent: &[u128],
        received: &[u128],
        expected: usize,
    ) -> Result<Vec<f64>, String> {
        // Count each side independently: zipping before validation would silently
        // discard an unpaired key and let five receipts satisfy an exact-six gate.
        if sent.len() != expected || received.len() != expected {
            return Err(format!(
                "input receipt count: expected {expected}, sent {}, received {}; sent={sent:?} received={received:?}; evidence {}",
                sent.len(),
                received.len(),
                self.path.display()
            ));
        }
        // Timestamps are nanoseconds on the common parent/child wall clock.
        // A receipt before its send is an error, never a zero-latency success.
        sent.iter()
            .zip(received)
            .map(|(issued, received)| {
                received
                    .checked_sub(*issued)
                    .map(|ns| ns as f64 / 1e6)
                    .ok_or_else(|| {
                        format!(
                            "child receipt preceded key; evidence {}",
                            self.path.display()
                        )
                    })
            })
            .collect()
    }
}

/// A disconnected owner is cancellation, rather than permission to inject later.
pub(super) fn wait(timer: &mpsc::Receiver<()>, delay: Duration) -> bool {
    matches!(
        timer.recv_timeout(delay),
        Err(mpsc::RecvTimeoutError::Timeout)
    )
}

#[cfg(test)]
/// Independent runs, exact counts and cancellation have deterministic witnesses.
mod tests;
