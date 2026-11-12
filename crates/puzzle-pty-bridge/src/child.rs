//! Event-driven child status with platform-specific native lifetime ownership.
//!
//! Unix keeps the original child handle for portable-pty's SIGHUP escalation.
//! Windows waits on the process handle in a blocking worker, since SIGCHLD
//! does not exist there; its cloned killer directly terminates that handle.
/// Unix SIGCHLD readiness, registered before spawning to avoid a lost exit.
#[cfg(unix)]
mod unix;
/// ConPTY process-handle wait; runtime validation remains an M13 review.
#[cfg(windows)]
mod windows;
#[cfg(unix)]
pub(crate) use unix::{ChildOwner, watch};
#[cfg(windows)]
pub(crate) use windows::{ChildOwner, watch};
