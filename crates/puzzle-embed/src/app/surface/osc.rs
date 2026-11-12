//! Host opt-in moves optional protocol ownership onto the Session thread (D-44).
//! The ordinary embedding build does not link program status. Generic registration
//! still lets a Rust host provide a different protocol without editing core enums.
use super::*;

/// Optional protocols use the same affinity/lifetime checks as ordinary host messages.
impl Surface {
    /// Registers one unhandled OSC number on the Session owner, before later output.
    /// Built-in numbers are rejected here; duplicate registrations are rejected by
    /// the Session and logged through its existing rejected-message diagnostic.
    pub fn register_osc_handler(
        &self,
        number: u16,
        handler: Box<dyn puzzle_term::osc::OscHandler>,
    ) -> Result<(), ApiError> {
        // Reject a stale capability before moving its handler to any owner channel.
        self.check()?;
        if puzzle_term::vt::osc::is_builtin(number) {
            return Err(ApiError::Platform(
                "OSC number is handled by the terminal".into(),
            ));
        }
        // No host callback runs on this thread; all mutable protocol state moves.
        self.send(SessionMessage::RegisterOsc { number, handler })
    }
}

// Optional conveniences never change the generic registration contract.
include!("optional_items.rs");
