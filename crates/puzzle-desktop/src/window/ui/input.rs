//! Recordable native input, default bindings and immediate PTY writes.
//!
//! The callback order remains visible in private recordings. A matched
//! host action wins over VT key encoding on the Session owner. The host keeps
//! only OS facts used for pointer coalescing; Surface owns key/IME translation.
//! Each callback queues independently of frame damage or drawable acquisition.
use super::InputWindow as LiveWindow;
use puzzle_embed::RawInput;

impl LiveWindow {
    /// Maps one recordable host event, then sends its command independently of redraw.
    pub(in crate::window) fn input(&mut self, input: RawInput) {
        #[cfg(feature = "lock-metrics")]
        self.input_arrived(input, std::time::Instant::now());
        #[cfg(not(feature = "lock-metrics"))]
        self.dispatch_input(input);
    }

    /// A finite fixture includes UI delivery time; native keys begin at their OS callback.
    #[cfg(feature = "lock-metrics")]
    pub(in crate::window) fn input_arrived(
        &mut self,
        input: RawInput,
        arrival: std::time::Instant,
    ) {
        self.dispatch_input(input, arrival);
    }

    /// Encoding uses one adapter path for native callbacks and authored probe callbacks.
    fn dispatch_input(
        &mut self,
        input: RawInput,
        #[cfg(feature = "lock-metrics")] arrival: std::time::Instant,
    ) {
        // Timestamp the actual adapter entrance; CPU clocks are read later by the owner.
        if let RawInput::Focus(focused) = &input {
            self.log_visibility("focus_input");
            if *focused {
                // Focus is the only implicit file check. A timer would
                // wake an otherwise idle terminal just to poll mtime (P2).
                // The reload keeps last-good settings on a syntax error.
                self.reload_settings(false);
            }
        }
        // Pointer coalescing needs only the host's latest modifier fact. All key
        // translation and composition state belongs to Surface's Session owner.
        if let RawInput::Modifiers(bits) = &input {
            self.modifiers = bits & 0x0f;
        }
        #[cfg(not(feature = "lock-metrics"))]
        self.sent(self.session.input(input));
        #[cfg(feature = "lock-metrics")]
        self.sent(self.session.input_arrived(input, arrival));
    }
}
