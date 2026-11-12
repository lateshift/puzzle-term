//! A finite copy receipt, handed to the model after the clipboard policy drain.
//!
//! The Session reports execution, never anticipates approval in the pointer
//! callback. Endpoints use the overlay clock so both hosts and manual scenarios
//! share exactly one lifetime; the drawing half cannot schedule a timer.

/// User-requested duration of the copy-result label, in overlay-clock seconds.
pub(super) const LABEL_SECONDS: f64 = 1.5;
/// User-requested one-shot output flash; reduced motion keeps it static.
pub(super) const FLASH_SECONDS: f64 = 0.2;

/// Accepted write or policy/service refusal for one explicit command identity.
#[derive(Clone, Copy, Debug)]
pub struct CopyFeedback {
    /// The same OSC 133 identity carried by the presented copy button.
    pub id: u64,
    /// True only when the authorized clipboard service accepted its write.
    pub copied: bool,
    /// Owner's injected overlay-clock time when the policy drain finished.
    pub started: f64,
}
impl CopyFeedback {
    /// Finite text independent of hover, so moving the pointer cannot hide failure.
    pub(super) fn label(self) -> &'static str {
        if self.copied {
            "Copied ✓"
        } else {
            "Copy blocked"
        }
    }
    /// Semantic success and warning colours follow the current publication theme.
    pub(super) fn token(self) -> &'static str {
        if self.copied {
            "zoneOk"
        } else {
            "semanticWarning"
        }
    }
    /// A clipboard refusal never claims that any source text was copied.
    pub(super) fn flash(self, time: f64, reduced_motion: bool) -> f32 {
        if !self.copied || time >= self.started + FLASH_SECONDS {
            return 0.0;
        }
        // Accessibility suppresses interpolation; one scheduled wake removes it.
        if reduced_motion {
            1.0
        } else {
            (1.0 - (time - self.started) / FLASH_SECONDS).clamp(0.0, 1.0) as f32
        }
    }
}
