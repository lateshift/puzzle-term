//! Background-tab activity is one output edge per explicit host acknowledgement.
//!
//! This latch belongs to Session, so parser slices and native/web transports
//! share identical semantics. Visibility is a host decision, not inferred from
//! focus: a visible split can be unfocused and still have its output seen.
//! No timer guesses when a burst ends. An empty feed, repaint or input gesture
//! cannot manufacture output, and draining an event does not acknowledge it.
use super::{Session, host_events::HostEvent};

impl Session {
    /// Mark previous output seen; only fresh subsequently admitted bytes re-arm it.
    pub fn activity_seen(&mut self) {
        // The owner handles this between slices through the normal control lane.
        self.host_activity_pending = false;
    }

    /// Called once per accepted source slice, never once per scalar or rendered cell.
    pub(super) fn observe_output_activity(&mut self, accepted: usize) {
        if accepted == 0 || self.host_activity_pending || self.host_fact_hook.is_none() {
            return;
        }
        // One pending event covers arbitrarily many chunks until a host says seen.
        // Bare parser users retain no extra event or allocation (P8).
        self.host_activity_pending = true;
        self.host_events.push_back(HostEvent::Activity);
    }
}
