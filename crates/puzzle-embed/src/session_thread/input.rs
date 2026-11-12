//! Native raw callbacks translate on the exclusive Session owner (D-50).
//!
//! A host queues owned facts without exposing the adapter as a second API. The
//! same ordered stream owns composition, modifiers, key routing and PTY writes.
//! Diagnostic clocks retain the actual host arrival rather than starting at mapping.
//!
//! The host supplies facts, not an already translated key. Keeping translation
//! here gives every Surface clone the same modifier and composition history.
//! Keybindings and the search query mode also belong to this owner, so an IME
//! commit cannot race a focus change made by an overlay pointer message.
//!
//! Configuration uses the same control queue as callbacks. A policy update
//! therefore takes effect before the next queued key, without a second host-side
//! adapter that could retain the previous Option-key setting.
//!
//! Recording observes this ordered stream before translation and then records
//! the selected command. Neither operation runs unless a recorder was supplied;
//! ordinary hosts do not acquire a recorder or a diagnostics dependency.
//!
//! Focus is carried in raw recordings for deterministic replay. Unlike modifier
//! bookkeeping it also changes terminal state, so it becomes a Platform command
//! when the adapter itself has no text/key command to emit.
use super::*;
use crate::input::RawInput;
use endpoint::{Control, HostMessage};
use puzzle_term::SessionMessage;

/// Native payload and optional observer clock, never a callback into the host.
pub(super) struct InputMessage {
    /// Layout, physical identity, text and modifier facts copied by the native host.
    pub(super) value: RawInput,
    /// Public trace timestamps survive conversion on the sole Session owner.
    timing: Option<crate::InputTiming>,
    /// Channel residence begins on the host before queueing.
    #[cfg(feature = "lock-metrics")]
    sent: Instant,
    /// Explicit probes include event-loop delivery delay before queueing.
    #[cfg(feature = "lock-metrics")]
    arrival: Instant,
}
impl Handle {
    /// Sends a raw callback; translation cannot wait on rendering or native UI.
    pub(crate) fn input(
        &self,
        input: RawInput,
        #[cfg(feature = "lock-metrics")] arrival: Instant,
    ) -> Result<(), String> {
        // post also wakes a sleeping owner; a key never waits behind the data backlog.
        self.post(Control::Input(InputMessage {
            value: input,
            timing: crate::InputTiming::capture().map(|mut timing| { timing.sent = Instant::now(); timing }),
            #[cfg(feature = "lock-metrics")]
            // Start residence timing before mpsc sends, retaining queue/preemption cost.
            sent: Instant::now(),
            #[cfg(feature = "lock-metrics")]
            arrival,
        }))
    }
    /// A caller may timestamp before converting its native input structure.
    /// Keep that API boundary and host residence intact; only the send boundary
    /// is replaced here. Reading another thread's CPU clock would be invalid,
    /// so owner CPU measurement starts later, on actual owner receipt.
    pub(crate) fn input_timed(
        &self,
        value: RawInput,
        mut timing: crate::InputTiming,
    ) -> Result<(), String> {
        timing.sent = Instant::now();
        self.post(Control::Input(InputMessage {
            value,
            timing: Some(timing),
            #[cfg(feature = "lock-metrics")]
            sent: timing.sent,
            #[cfg(feature = "lock-metrics")]
            arrival: timing.api,
        }))
    }
    /// Timed public inputs preserve the API-entry stamp across marshalling.
    /// Typed keys and committed text use the same control FIFO as raw callbacks;
    /// adding clocks must not create a second input route or bypass query focus.
    pub(crate) fn send_timed(
        &self,
        value: SessionMessage,
        mut timing: crate::InputTiming,
    ) -> Result<(), String> {
        timing.sent = Instant::now();
        self.post(Control::Message(HostMessage {
            value,
            timing: Some(timing),
            #[cfg(feature = "lock-metrics")]
            sent: timing.sent,
            #[cfg(feature = "lock-metrics")]
            arrival: timing.api,
        }))
    }

    /// Applied in sender order before the next key, preserving live Option policy.
    pub(crate) fn input_policy(
        &self,
        policy: puzzle_term::session::OptionAsAlt,
    ) -> Result<(), String> {
        // FIFO with Input prevents live settings from overtaking a queued callback.
        self.post(Control::InputPolicy(policy))
    }
}
impl Owner {
    /// Raw and selected command records stay adjacent on the source owner.
    pub(super) fn apply_input(&mut self, mut input: InputMessage) {
        // Include activity revocation, optional recording and native translation
        // in owner work; beginning after the adapter would hide those costs.
        if let Some(timing) = &mut input.timing {
            timing.applied();
        }
        // Raw callbacks revoke even when a modifier produces no encoded command.
        self.session.user_activity();
        // Focus is an OS fact in addition to its place in the raw callback stream.
        let focused = match &input.value {
            RawInput::Focus(value) => Some(*value),
            _ => None,
        };
        if let Some(recorder) = &self.recorder
            && let Err(error) = recorder.input(&input.value)
        {
            tracing::warn!(event = "desktop.record_failed", %error);
        }
        let command = self
            .input
            .apply(input.value)
            .or_else(|| focused.map(puzzle_term::messages::Command::Focus));
        // This privacy-sensitive target is disabled by the normal info-level filter.
        tracing::debug!(target: "puzzle_desktop::input", event = "KeyCommand", ?command,
            composing = self.input.composing());
        if let Some(recorder) = &self.recorder
            && let Err(error) = recorder.command(command.as_ref())
        {
            tracing::warn!(event = "desktop.record_failed", %error);
        }
        if let Some(command) = command {
            // The existing control implementation owns writer clocks and routing policy.
            self.apply_control(Control::Message(HostMessage {
                timing: input.timing,
                value: puzzle_term::SessionMessage::Command(
                    command,
                    if focused.is_some() {
                        puzzle_term::Origin::Platform
                    } else {
                        puzzle_term::Origin::User
                    },
                ),
                #[cfg(feature = "lock-metrics")]
                sent: input.sent,
                #[cfg(feature = "lock-metrics")]
                arrival: input.arrival,
            }));
        }
    }
}
