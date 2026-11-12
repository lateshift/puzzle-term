//! Pure input-to-byte conversion for the eventual platform transport (§6.8).
//!
//! Browser and desktop adapters will supply these neutral events in M7.
//! Here the active terminal modes are copied into `InputModes`, so encoding
//! cannot mutate grid state or perform I/O. Kitty keeps two bounded stacks
//! because an application in the alternate screen must not change the
//! primary shell's keyboard contract.

/// Xterm's historical keyboard sequences and modifier rules.
pub(crate) mod key;
/// Kitty's negotiated keyboard mode and canonical key-event encoding.
use crate::protocols::kitty::keyboard as kitty;
/// X10, UTF-8, SGR, urxvt and SGR-pixel mouse reports.
mod mouse;
/// Current reporting switches, shared by the core handler and host adapter.
mod mouse_modes;
pub use mouse_modes::MouseModes;
/// Focus reporting and hostile-paste sanitisation.
mod paste;
/// US physical scalar identity shared by native and future DOM adapters.
pub mod physical;

pub use key::{Key, KeyEvent, KeyPhase, Modifiers, NamedKey, encode_key};
pub use kitty::KittyModes;
pub use mouse::{MouseEncoding, MouseEvent, MouseKind, MouseTracking, encode_mouse};
pub use paste::{encode_focus, encode_paste};

/// Terminal switches relevant to one key or paste conversion.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InputModes {
    /// DECCKM changes the cursor-key introducer to SS3.
    pub application_cursor: bool,
    /// DECNKM or DECKPAM enables keypad application reports.
    pub application_keypad: bool,
    /// LNM adds LF to the Enter key's CR.
    pub newline: bool,
    /// User choice for the historical Backspace byte.
    pub backspace_bs: bool,
    /// Modes 1036/1039 allow the ESC prefix for Alt-modified keys.
    pub alt_sends_escape: bool,
    /// Current screen's kitty progressive-enhancement flags.
    pub kitty_flags: u8,
    /// DECSET 1004 allows focus in/out reports.
    pub focus_reports: bool,
    /// DECSET 2004 wraps sanitised paste in delimiters.
    pub bracketed_paste: bool,
}

impl InputModes {
    /// Takes a consistent input snapshot from the pure terminal core.
    #[must_use]
    pub fn from_terminal(term: &crate::Terminal) -> Self {
        Self {
            application_cursor: term.modes.get(true, 1) == Some(true),
            application_keypad: term.modes.get(true, 66) == Some(true),
            newline: term.modes.get(false, 20) == Some(true),
            backspace_bs: false,
            alt_sends_escape: term.modes.get(true, 1036) == Some(true)
                || term.modes.get(true, 1039) == Some(true),
            kitty_flags: term.kitty_keys.flags(term.active),
            focus_reports: term.modes.get(true, 1004) == Some(true),
            bracketed_paste: term.modes.get(true, 2004) == Some(true),
        }
    }
}

#[cfg(test)]
#[path = "input/tests.rs"]
mod tests;
