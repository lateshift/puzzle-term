//! Native callback methods expose values, never internal owner endpoints.
//!
//! Native hosts may submit the original layout/physical callback stream or use
//! Surface's typed key/text/preedit methods. Raw callbacks are translated on the
//! Session thread, so its composition state survives independently of frames.
//! In either case the host returns after enqueueing; no method acquires a
//! drawable, prepares glyphs or waits for the parser to finish a slice.
//!
//! Expose damage belongs to the rendering mailbox. Appearance and pointer
//! routing instead belong to Session policy. Keeping those destinations inside
//! the library prevents a host from accidentally bypassing policy with a
//! lower-level actor update. The most recently presented regions decide which
//! model consumes a pointer, even when a newer frame is still being built.
//! A native paste gesture carries clipboard generation and MIME availability.
//! The library generates its secret with getrandom so every native host shares
//! one secure implementation; predictable counters are only completion identities.
//! OS entropy failure returns an API error rather than weakening the capability.
//! Hex keeps all 256 entropy bits in a printable protocol credential.
//! No credential is exposed in a host callback or tracing field.
//! The Session receives ownership through its control inbox before deciding mode
//! precedence, sanitizing fallback text or emitting a MIME listing.
use super::*;
use crate::input::RawInput;
impl Surface {
    /// Completes a user paste gesture with revision and MIME listing from the host.
    /// The native secure token generator is shared by every host; tokens are never logged.
    /// `revision` must change on every clipboard content change, even a restoration.
    pub fn clipboard_paste(
        &self,
        primary: bool,
        revision: u64,
        mimes: Vec<String>,
        text: Option<String>,
    ) -> Result<(), ApiError> {
        self.check()?;
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).map_err(|error| ApiError::Platform(error.to_string()))?;
        // Hex is printable UTF-8 for kitty's password field; 256 bits remain unguessable.
        let mut secret = String::with_capacity(64);
        for byte in random {
            use std::fmt::Write;
            write!(&mut secret, "{byte:02x}")
                .map_err(|error| ApiError::Platform(error.to_string()))?;
        }
        self.send(SessionMessage::ClipboardPaste(
            puzzle_term::clipboard::Paste {
                secret: puzzle_term::vt::clipboard::Secret(secret),
                revision,
                primary,
                mimes,
                text,
            },
        ))
    }

    /// Answers a MIME operation on its Session owner; stale tokens are ignored there.
    pub fn clipboard_answer(&self, answer: puzzle_term::clipboard::Answer) -> Result<(), ApiError> {
        self.send(SessionMessage::ClipboardAnswer(answer))
    }
    /// Queues one layout-aware native callback; one Session-owned adapter handles the stream.
    pub fn input(&self, input: RawInput) -> Result<(), ApiError> {
        let timing = crate::InputTiming::capture();
        self.check()?;
        if let Some(timing) = timing {
            return self.input_timed(input, timing);
        }
        self.session
            .input(
                input,
                #[cfg(feature = "lock-metrics")]
                std::time::Instant::now(),
            )
            .map_err(ApiError::Platform)
    }
    /// Queues a native callback with a host/API observation captured before conversion.
    /// Enable `puzzle_desktop::input=debug` for the per-key stage breakdown.
    /// `input` keeps layout, physical identity, modifiers and composition state;
    /// `timing` is metadata only and cannot alter the adapter's routing decision.
    /// Like `input`, this rejects a closed surface or a foreign calling thread,
    /// and returns after enqueueing rather than waiting for a Session or frame.
    pub fn input_timed(
        &self,
        input: RawInput,
        mut timing: crate::InputTiming,
    ) -> Result<(), ApiError> {
        self.check()?;
        timing.surface = Some(self.id.0);
        self.session
            .input_timed(input, timing)
            .map_err(ApiError::Platform)
    }

    /// Typed keys use the same timed owner/writer lane as raw native callbacks.
    /// This preserves `key`'s native text-insertion ownership: an unhandled press
    /// may emit NativeTextAllowed so the host can supply its subsequent commit.
    /// It does not force an extra byte write for that press. Use `input_timed`
    /// for a complete native callback handled entirely by Puzzle's raw adapter.
    pub fn key_timed(
        &self,
        key: puzzle_term::vt::input::KeyEvent,
        mut timing: crate::InputTiming,
    ) -> Result<(), ApiError> {
        self.check()?;
        timing.surface = Some(self.id.0);
        self.session
            .send_timed(SessionMessage::NativeKey(key, Origin::User), timing)
            .map_err(ApiError::Platform)
    }

    /// Committed text retains its own event clock; preedit remains provisional.
    /// Supply the commit's actual host clock when available, otherwise use `text`.
    /// Current query focus and keybinding policy still belong to the Session;
    /// a consumed edit has no writer sample, and cannot label a later reply.
    pub fn text_timed(&self, text: String, mut timing: crate::InputTiming) -> Result<(), ApiError> {
        self.check()?;
        timing.surface = Some(self.id.0);
        self.session
            .send_timed(
                SessionMessage::Command(Command::Text(text), Origin::User),
                timing,
            )
            .map_err(ApiError::Platform)
    }

    /// Harness-only arrival observation; not host API. It changes no input policy.
    #[cfg(all(feature = "test-support", feature = "lock-metrics"))]
    pub fn input_arrived(
        &self,
        input: RawInput,
        arrival: std::time::Instant,
    ) -> Result<(), ApiError> {
        self.check()?;
        if let Some(timing) = crate::InputTiming::arrived(arrival) {
            self.input_timed(input, timing)
        } else {
            self.session
                .input(input, arrival)
                .map_err(ApiError::Platform)
        }
    }
    /// Host expose damage is a paced request, with no synchronous frame work.
    pub fn request_frame(&self) -> Result<(), ApiError> {
        self.update(SurfaceUpdate::Frame)
    }
    /// OS appearance remains platform policy, distinct from API-dispatched user settings.
    pub fn appearance(&self, appearance: puzzle_term::theme::Appearance) -> Result<(), ApiError> {
        self.send(SessionMessage::Command(
            Command::SetSystemAppearance(appearance),
            Origin::Platform,
        ))
    }
    /// Pointer-leave and region events use the most recently presented region table.
    pub fn overlay_pointer(
        &self,
        event: puzzle_term::overlay::provider::PointerEvent,
    ) -> Result<(), ApiError> {
        self.send(SessionMessage::OverlayPointer(event))
    }
    /// Typed mouse reporting still passes through the user's policy interceptors.
    pub fn mouse(&self, event: puzzle_term::vt::input::MouseEvent) -> Result<(), ApiError> {
        self.send(SessionMessage::Command(Command::Mouse(event), Origin::User))
    }
}
