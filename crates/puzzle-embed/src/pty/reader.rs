//! Interruptible Unix master reads with bounded discard on explicit host shutdown.
//!
//! portable-pty exposes its master FD safely but erases the cloned reader's FD.
//! filedescriptor duplicates that still-owned descriptor through its safe API.
//! A shutdown pipe interrupts poll, including an
//! idle slave inherited by a descendant; normal reads have no timer at all.
//! Only the drain reads the master, so readiness cannot be stolen between poll
//! and read. The writer has a different duplicate and does not consume output.
use std::io::Read;

/// Shared cancellation is a shutdown-only wake, never a Session reference.
#[derive(Clone, Default)]
pub(super) struct Cancel {
    /// Unix uses a pipe EOF to wake a blocking poll without a blocking send.
    #[cfg(unix)]
    inner: Option<std::sync::Arc<Wake>>,
}
impl Cancel {
    /// Starts the drain deadline and wakes an idle read exactly once.
    pub(super) fn stop(&self) {
        #[cfg(unix)]
        if let Some(inner) = &self.inner
            && !inner
                .cancelled
                .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            let _ = inner.pipe.shutdown(std::net::Shutdown::Write);
        }
    }
}

/// The writer requests closure before the drain starts process escalation.
#[cfg(unix)]
struct Wake {
    /// Atomic observation keeps ordinary short reads free of locks.
    cancelled: std::sync::atomic::AtomicBool,
    /// Shutdown is persistent, so an idle reader cannot lose a cancellation wake.
    pipe: std::os::unix::net::UnixStream,
}

/// Closing the last master cannot depend on every descendant closing its slave.
#[cfg(unix)]
struct Reader {
    /// This duplicate is destroyed before child reaping, even without kernel EOF.
    master: filedescriptor::FileDescriptor,
    /// Poll observes pipe EOF after host cancellation.
    pipe: std::os::unix::net::UnixStream,
    /// No per-read allocation or lifecycle mutex is introduced.
    wake: std::sync::Arc<Wake>,
    /// Relative deadline begins at the first cancelled read, even after pool blocking.
    deadline: Option<std::time::Instant>,
}

/// Discard lasts at most one second after cancellation reaches the drain. This
/// covers the two 250-ms HUP/TERM grace windows plus KILL and kernel-close work;
/// it is a teardown bound, not a throughput or frame-policy constant.
#[cfg(unix)]
const DRAIN_GRACE: std::time::Duration = std::time::Duration::from_secs(1);

/// The borrow keeps the portable-pty owner alive for the safe duplication call.
#[cfg(unix)]
struct BorrowedMaster<'a>(
    /// Owner whose lifetime encloses the descriptor borrow.
    &'a dyn portable_pty::MasterPty,
    /// Validated native descriptor exposed by that owner.
    std::os::fd::RawFd,
);
#[cfg(unix)]
impl std::os::fd::AsRawFd for BorrowedMaster<'_> {
    /// Returns the descriptor without taking ownership or constructing a raw File.
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        // Keep the owning borrow explicit; duplication cannot outlive this value.
        let _owner = self.0;
        self.1
    }
}

/// Acquires an independently owned read descriptor while MasterPty is still live.
pub(super) fn pair(
    master: &dyn portable_pty::MasterPty,
) -> Result<(Box<dyn Read + Send>, Cancel), String> {
    #[cfg(unix)]
    {
        // Portable-pty retains ownership across duplication. Missing descriptors
        // are startup errors rather than an attempt to poll an unrelated handle.
        let fd = master
            .as_raw_fd()
            .ok_or("PTY master has no Unix descriptor")?;
        let file = filedescriptor::FileDescriptor::dup(&BorrowedMaster(master, fd))
            .map_err(|e| format!("duplicate PTY master: {e}"))?;
        // A socket-pair shutdown provides persistent readiness, with no allocation
        // or signal handler when a different worker cancels a blocked read.
        let (read, write) = std::os::unix::net::UnixStream::pair().map_err(|e| e.to_string())?;
        let wake = std::sync::Arc::new(Wake {
            cancelled: std::sync::atomic::AtomicBool::new(false),
            pipe: write,
        });
        let reader = Reader {
            master: file,
            pipe: read,
            wake: wake.clone(),
            deadline: None,
        };
        // The last read descriptor belongs to the drain, never to a host callback.
        Ok((Box::new(reader), Cancel { inner: Some(wake) }))
    }
    #[cfg(not(unix))]
    {
        master
            .try_clone_reader()
            .map(|reader| (reader, Cancel::default()))
            .map_err(|e| e.to_string())
    }
}

#[cfg(unix)]
impl Read for Reader {
    /// Waits indefinitely during normal operation, but never indefinitely on close.
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        use std::{sync::atomic::Ordering, time::Instant};
        // The poll descriptors borrow owned objects, so neither can be closed by
        // cancellation while the kernel is observing their readiness.
        loop {
            // Cancellation may have arrived while the fixed pool was exhausted.
            // Begin the discard allowance when this worker resumes consumption.
            if self.wake.cancelled.load(Ordering::Acquire) && self.deadline.is_none() {
                self.deadline = Some(Instant::now() + DRAIN_GRACE);
            }
            let timeout = if let Some(end) = self.deadline {
                // Every read shares one deadline; active output cannot extend it.
                let remaining = end.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Ok(0);
                }
                Some(Timespec::try_from(remaining).map_err(std::io::Error::other)?)
            } else {
                None
            };
            // After cancellation the persistent pipe HUP is excluded to avoid a
            // busy loop; the deadline still bounds a completely silent descendant.
            let mut fds = [
                PollFd::new(&self.master, PollFlags::IN),
                PollFd::new(&self.pipe, PollFlags::IN),
            ];
            // Ordinary operation sleeps until output or cancellation, not a timer.
            let watched = if self.deadline.is_some() {
                &mut fds[..1]
            } else {
                &mut fds[..]
            };
            match poll(watched, timeout.as_ref()) {
                // Unrelated process signals do not count as EOF or consume grace.
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(error.into()),
                Ok(0) => return Ok(0), // Explicit shutdown deadline only.
                Ok(_) => {}
            }
            if !fds[0].revents().is_empty() {
                // Read also handles master HUP/ERR: Unix PTYs finish with EOF/EIO.
                return self.master.read(bytes);
            }
            // The cancellation pipe woke first; install the bounded drain clock.
        }
    }
}

#[cfg(all(test, unix))]
/// Real silent-descriptor cancellation must not require a slave's EOF.
mod tests;
