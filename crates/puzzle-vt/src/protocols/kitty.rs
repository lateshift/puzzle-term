//! Kitty keyboard negotiation, key encoding and graphics framing (D-58).
//! References: <https://sw.kovidgoyal.net/kitty/keyboard-protocol/> and
//! <https://sw.kovidgoyal.net/kitty/graphics-protocol/>.
/// Bounded graphics framing; re-exported through the existing crate API.
pub mod apc;
/// Protocol key-event grammar and functional-key numbers.
pub(crate) mod encoding;
/// Progressive keyboard flag stacks, one per terminal screen.
pub(crate) mod keyboard;
/// Published names for non-text function keys.
pub(crate) mod named;

/// OSC 21 named colour control over shared terminal colour state.
pub(crate) mod colors;

/// MIME clipboard wire assembly; permissions stay in Session.
pub(crate) mod clipboard;

/// Bounded desktop notification grammar and chunk assembly.
pub(crate) mod notifications;

/// OSC 72 drop target negotiation, never native file I/O.
pub(crate) mod dnd;

/// Bounded rectangular grid text, distinct from graphics placements.
pub(crate) mod text_sizing;

/// OSC 133 command metadata defined by kitty, distinct from FinalTerm boundaries.
pub(crate) mod shell;
