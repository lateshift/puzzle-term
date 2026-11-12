//! Bounded drop values shared by the OSC origin handler and Session's host seam.
//! No OS object, file access or clock enters this model. Only an explicit user
//! drop carries bytes; hover offers contain names, and stale generations are inert.
use crate::clipboard::MimeData;
/// The reference terminal's curated eager-drop bound; no native request queue is retained.
pub const MAX_ITEMS: usize = 16;
/// Kitty's MIME-list bound applies across all chunks of one registration/status.
pub const MAX_MIME_BYTES: usize = 1024 * 1024;
/// Drop payload storage uses the same 64-MiB security cap as MIME clipboard data.
pub const MAX_BYTES: usize = crate::clipboard::MAX_BYTES;
/// Host intent is plain data delivered after the ordinary interceptor lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    /// Update native registration. None means unregister and discard host drag state.
    Registration {
        /// Never reused during this terminal lifetime, including across RIS.
        generation: u64,
        /// Requested MIME list; empty accepts the host's ordinary representations.
        mimes: Option<String>,
    },
    /// Program's feedback for the current hover, initially rejected.
    Acceptance {
        /// Correlates feedback to a currently registered surface generation.
        generation: u64,
        /// 0 rejects, 1 copies, 2 moves. Other program values become rejection.
        operation: u32,
        /// Preferred subset, in order; empty keeps the preceding preferences.
        mimes: String,
    },
    /// Release captured drop resources; the native OS drop already ended eagerly.
    Concluded {
        /// Prevents a delayed conclusion from affecting a later registration.
        generation: u64,
        /// Final operation, or zero on cancel/error.
        operation: u32,
    },
}
/// Trusted native drag observations, distinct from untrusted OSC requests.
#[derive(Clone, Debug)]
pub enum Input {
    /// Names only; any nonempty data here is rejected before it can be retained.
    Move {
        /// Zero-based cell x/y followed by device-pixel x/y, resolved by Session.
        position: [u32; 4],
        /// Source allows copy (1), move (2), or both (3).
        operations: u32,
        /// Bounded ordered representations with empty data.
        items: Vec<MimeData>,
    },
    /// Leave discards hover and any captured resources immediately.
    Leave,
    /// A user drop grants reads of exactly these eagerly captured representations.
    Drop {
        /// Zero-based cell x/y and device-pixel x/y.
        position: [u32; 4],
        /// Native source's supported operations.
        operations: u32,
        /// Immutable bytes from the host; no path is opened by the terminal.
        items: Vec<MimeData>,
    },
}
/// Shared storage lives outside the defining protocol's wire module (D-58).
mod state;
pub(crate) use state::State;

/// Registration identity survives resets; its optional payload is lazy.
/// Keeping the epoch outside the allocation prevents stale callbacks after RIS.
#[derive(Default)]
pub(crate) struct Registration {
    /// Monotonic native callback identity, even while no program is registered.
    pub(crate) generation: u64,
    /// No heap storage until registration (or its first incoming chunk).
    pub(crate) state: Option<Box<State>>,
}
