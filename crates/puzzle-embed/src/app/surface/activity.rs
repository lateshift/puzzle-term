//! Explicit host acknowledgement of output uses the Session's control lane.
//!
//! No host-side quiet timer, focus heuristic or render-thread query is needed.
//! Hosts acknowledge visible panes and each activity callback while still visible;
//! hidden panes retain their unread indication until the host makes them visible.
use super::Surface;
use crate::ApiError;
use puzzle_term::SessionMessage;

impl Surface {
    /// Acknowledge prior output; the next fresh output emits HostEvent::Activity.
    ///
    /// Merely receiving the event does not acknowledge it. This queued operation
    /// never waits for Session, terminal rendering or child process execution.
    pub fn activity_seen(&self) -> Result<(), ApiError> {
        self.send(SessionMessage::ActivitySeen)
    }
}
