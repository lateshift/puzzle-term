//! Display subscriptions last until output is represented and requests are answered.
//!
//! Consuming a pacer opportunity only sends a request. It is not proof that the
//! owner replied, nor that parsed output reached a drawable. Keeping these clocks
//! separate prevents stop/start on every NothingNew or late publication (D-53).
//! This render-owned value contains no Session, lock, timer or callback capability.

/// Outstanding work is distinct from one already-consumed display opportunity.
#[derive(Default)]
pub(super) struct FrameDemand {
    /// Byte offset resolved by submission or an explicitly unchanged owner answer.
    covered: u64,
    /// A dirty publication still needs a successful submission, even after NothingNew.
    unpresented: bool,
    /// Newer requests supersede older ones on the Session's priority inbox.
    request: Option<u64>,
}

impl FrameDemand {
    /// Sending a request keeps its source alive even if its wait expires.
    pub(super) fn requested(&mut self, id: u64) {
        self.request = Some(id);
    }

    /// An older queued reply cannot acknowledge a newer outstanding request.
    pub(super) fn answered(&mut self, id: u64, published: bool, parsed: u64) {
        if self.request.is_some_and(|request| id >= request) {
            self.request = None;
        }
        if published {
            self.unpresented = true;
        } else {
            // Replies and nonvisual controls consume bytes without requiring a frame.
            // They acknowledge source progress, never another publication's dirty ink.
            self.covered = self.covered.max(parsed);
        }
    }

    /// Only actual submission advances the output represented on screen.
    pub(super) fn presented(&mut self, parsed: u64) {
        self.covered = self.covered.max(parsed);
        self.unpresented = false;
    }

    /// Channel residence and parsed-but-unpresented bytes both mean non-idle work.
    pub(super) fn pending(&self, parsed: u64, queued_bytes: usize) -> bool {
        self.request.is_some() || self.unpresented || parsed > self.covered || queued_bytes != 0
    }
}

impl super::LiveWindow {
    /// Observation uses existing scalar counters; it never queries the Session owner.
    #[cfg(target_os = "macos")]
    pub(in crate::window) fn output_pending(&self) -> bool {
        self.frame_demand
            .pending(self.session.parsed_total(), self.session.pending_bytes())
    }
}

#[cfg(test)]
mod tests;
