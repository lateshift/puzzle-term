//! Diagnostic closures execute on the Session owner, never in shipping UI code.
//!
//! This module is compiled only for tests and the existing measurement feature.
//! A caller supplies its diagnostic deadline; no production frame or input path
//! invokes a closure, borrows VT, or waits for a reply through this capability.
use super::{Control, Handle};
use puzzle_term::Session;
use std::{sync::mpsc, time::Instant};

/// Erased diagnostic requests retain their private typed reply sender by value.
pub(super) type Request = Box<dyn FnOnce(&mut Session) + Send>;

impl Handle {
    /// Runs an explicitly enabled diagnostic query within the caller's own wait bound.
    pub fn probe<T: Send + 'static>(
        &self,
        deadline: Instant,
        operation: impl FnOnce(&mut Session) -> T + Send + 'static,
    ) -> Result<T, String> {
        // One request owns one return path; the receiver is never shared by a lock.
        let (sender, receiver) = mpsc::channel();
        self.post(Control::Probe(Box::new(move |session| {
            // An expired observer may drop its receiver while its queued query finishes.
            // That cancellation has no effect on terminal output or future input.
            let _ = sender.send(operation(session));
        })))?;
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| error.to_string())
    }
}
