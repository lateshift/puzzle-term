//! Legacy xterm keys and kitty's enhanced key events (§6.8).
//!
//! The platform supplies logical key identity separately from committed
//! text. That distinction lets a German keyboard or an IME provide text
//! without forging a US-layout key code. The numeric kitty function-code
//! path covers its full published PUA table, including media and modifier
//! keys that legacy xterm has no representation for.
//!
//! Xterm has two encodings for the same cursor key: CSI in normal mode,
//! SS3 in DECCKM. The choice is made before Alt is considered because a
//! CSI modifier parameter already represents Alt on functional keys.
//! Printable keys are different: their ordinary form is UTF-8, and the
//! legacy Alt convention places a separate ESC before those bytes.
//! These rules deliberately preserve terminal ambiguity in legacy mode;
//! applications request kitty's enhanced mode to disambiguate it.
use super::InputModes;

/// Numeric and application keypad conversions.
mod keypad;
/// Kitty's separate wire grammar and functional-code catalogue.
use crate::protocols::kitty::encoding as kitty_encoding;
/// Named non-text keys from kitty's functional-key definition table.
use crate::protocols::kitty::named;
use kitty_encoding::kitty;
pub use named::NamedKey;

/// Platform-neutral key identity, before mode-dependent byte encoding.
///
/// Logical characters and keypad keys remain distinct so application
/// keypad mode can change only the latter without changing typed text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    /// A Unicode scalar on the active layout.
    Char(char),
    /// The four cursor keys, in xterm's A/B/C/D order.
    Up,
    /// Cursor-down key.
    Down,
    /// Cursor-right key.
    Right,
    /// Cursor-left key.
    Left,
    /// Start of the current line or buffer.
    Home,
    /// End of the current line or buffer.
    End,
    /// Insert key.
    Insert,
    /// Delete key.
    Delete,
    /// One-page upward motion.
    PageUp,
    /// One-page downward motion.
    PageDown,
    /// Function key, including kitty's F13–F35 range.
    ///
    /// The legacy table defines only F1–F12; higher keys need kitty mode.
    Function(u8),
    /// Main Return key.
    Enter,
    /// Numeric keypad Return key.
    KeypadEnter,
    /// Horizontal tabulation key.
    Tab,
    /// Backspace key.
    Backspace,
    /// Escape key.
    Escape,
    /// Numeric keypad digit, 0–9.
    Keypad(u8),
    /// Numeric keypad operator: +, -, *, /, . or =.
    KeypadOp(char),
    /// A named kitty functional key with no ordinary xterm form.
    Named(NamedKey),
}

/// Key transition kind, reported only when kitty requests it.
///
/// A legacy application must never receive an unexplained release code.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum KeyPhase {
    /// Initial depression of a key.
    #[default]
    Press,
    /// Repeated depression while held.
    Repeat,
    /// Key release.
    Release,
}

/// Kitty/xterm modifier bit set; the wire parameter adds one (§6.8).
///
/// Bit positions match kitty's table, leaving room for Hyper, Meta and
/// lock modifiers even though ordinary desktop shortcuts use four bits.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modifiers(pub u8);

impl Modifiers {
    /// Physical Shift is bit zero.
    pub const SHIFT: u8 = 1;
    /// Alt or Option is bit one.
    pub const ALT: u8 = 2;
    /// Control is bit two.
    pub const CTRL: u8 = 4;
    /// Command or Windows/Super is bit three.
    pub const SUPER: u8 = 8;

    /// Tests one modifier without tying adapters to a platform enum.
    #[must_use]
    pub fn has(self, flag: u8) -> bool {
        self.0 & flag != 0
    }

    /// Returns the 1-based CSI modifier parameter.
    ///
    /// One represents no modifiers, avoiding an omitted/zero ambiguity.
    #[must_use]
    pub fn parameter(self) -> u16 {
        u16::from(self.0) + 1
    }
}

/// One key event, including optional kitty shifted/base identities.
///
/// A shifted scalar can differ from the unshifted key on international
/// layouts; the base scalar denotes its physical US-layout position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyEvent {
    /// Exact W3C physical code, when supplied by a platform adapter.
    pub code: Option<String>,
    /// Composition keys belong to the IME, never to application shortcuts.
    pub composing: bool,
    /// Layout-aware identity, independent of produced text.
    pub key: Key,
    /// Shift, Alt, Ctrl and extended kitty modifier bits.
    pub modifiers: Modifiers,
    /// Press, repeat or release.
    pub phase: KeyPhase,
    /// Shifted logical scalar when a layout exposes one.
    pub shifted: Option<char>,
    /// US PC-101 physical-layout scalar for shortcut matching.
    pub base: Option<char>,
    /// Committed text, possibly more than one Unicode scalar.
    ///
    /// IME output can have no single corresponding physical key.
    pub text: Option<String>,
}

impl KeyEvent {
    /// Builds a plain press; adapters may fill the optional fields.
    #[must_use]
    pub fn press(key: Key) -> Self {
        Self {
            key,
            code: None,
            composing: false,
            modifiers: Modifiers(0),
            phase: KeyPhase::Press,
            shifted: None,
            base: None,
            text: None,
        }
    }
}

/// Encodes one event using the active screen's keyboard mode.
///
/// The caller owns the resulting bytes and writes them to a transport;
/// this function neither reads a clock nor mutates the terminal.
#[must_use]
pub fn encode_key(event: &KeyEvent, modes: InputModes) -> Vec<u8> {
    if modes.kitty_flags == 0 {
        return legacy(event, modes);
    }
    kitty(event, modes)
}

/// Chooses xterm's historical key form, including DECCKM and keypad mode.
pub(crate) fn legacy(event: &KeyEvent, modes: InputModes) -> Vec<u8> {
    // No legacy byte distinguishes release from press, so a release would
    // look like an extra keystroke to pre-kitty applications.
    if event.phase == KeyPhase::Release || event.modifiers.has(Modifiers::SUPER) {
        // Legacy terminals have no Super modifier bit. Treating Cmd+letter
        // as plain text would turn an unbound host shortcut into input.
        return Vec::new();
    }
    let bytes = match event.key {
        Key::Up
        | Key::Down
        | Key::Right
        | Key::Left
        | Key::Home
        | Key::End
        | Key::Insert
        | Key::Delete
        | Key::PageUp
        | Key::PageDown
        | Key::Function(1..=12) => legacy_functional(event.key, event.modifiers, modes),
        _ => legacy_text(event, modes),
    };
    legacy_alt_prefix(bytes, event.key, event.modifiers, modes)
}

/// Encodes xterm cursor, editing and F-key families with CSI or SS3.
fn legacy_functional(key: Key, mods: Modifiers, modes: InputModes) -> Vec<u8> {
    match key {
        Key::Up | Key::Down | Key::Right | Key::Left | Key::Home | Key::End => {
            // DECCKM selects SS3 only for unmodified cursor-family keys;
            // a modifier requires CSI 1;m plus the same final letter.
            let letter = cursor_letter(key);
            if mods.0 != 0 {
                format!("\x1b[1;{}{letter}", mods.parameter()).into_bytes()
            } else if modes.application_cursor {
                format!("\x1bO{letter}").into_bytes()
            } else {
                format!("\x1b[{letter}").into_bytes()
            }
        }
        Key::Insert | Key::Delete | Key::PageUp | Key::PageDown => {
            // Editing keys always use CSI tilde; DECCKM is irrelevant.
            tilde(tilde_number(key), mods)
        }
        Key::Function(n @ 1..=4) => {
            // VT-style F1–F4 share the SS3 finals P–S, but a modifier
            // switches to CSI 1;m so it can be represented numerically.
            let letter = char::from(b'P' + n - 1);
            if mods.0 == 0 {
                format!("\x1bO{letter}").into_bytes()
            } else {
                format!("\x1b[1;{}{letter}", mods.parameter()).into_bytes()
            }
        }
        Key::Function(n @ 5..=12) => tilde(FUNCTION_TILDE[usize::from(n - 5)], mods),
        _ => Vec::new(),
    }
}

/// Encodes characters, C0 keys and the application keypad family.
fn legacy_text(event: &KeyEvent, modes: InputModes) -> Vec<u8> {
    let mods = event.modifiers;
    // Numeric mode types the layout's decimal separator; DECKPAM and kitty
    // still use the physical keypad identity, independent of that text.
    if !modes.application_keypad
        && matches!(event.key, Key::Keypad(_) | Key::KeypadOp(_))
        && let Some(text) = event
            .text
            .as_ref()
            .filter(|s| !s.chars().any(char::is_control))
    {
        return text.as_bytes().to_vec();
    }
    match event.key {
        Key::Enter => {
            // LNM is a terminal mode: an Enter key sends CR LF when set.
            if modes.newline {
                b"\r\n".to_vec()
            } else {
                b"\r".to_vec()
            }
        }
        Key::KeypadEnter => {
            // DECKPAM wins over LNM for the keypad's distinct Enter key.
            if modes.application_keypad {
                b"\x1bOM".to_vec()
            } else if modes.newline {
                b"\r\n".to_vec()
            } else {
                b"\r".to_vec()
            }
        }
        // Back-tab is the historical CSI Z sequence, not a literal HT.
        Key::Tab if mods.has(Modifiers::SHIFT) => b"\x1b[Z".to_vec(),
        Key::Tab => b"\t".to_vec(),
        // Ctrl-Backspace and the configurable BS policy use byte 0x08;
        // the default DEL matches xterm's `kbs` capability.
        Key::Backspace if mods.has(Modifiers::CTRL) || modes.backspace_bs => vec![8],
        Key::Backspace => vec![127],
        Key::Escape => vec![27],
        Key::Char(ch) => legacy_char(ch, mods, event.shifted),
        Key::Keypad(n @ 0..=9) => keypad::digit(n, modes.application_keypad),
        Key::KeypadOp(ch) => keypad::operator(ch, modes.application_keypad),
        // A key without a legacy representation is not guessed into a
        // printable byte that could trigger an unintended shell command.
        _ => Vec::new(),
    }
}

/// Adds the historical ESC prefix only to text and C0 legacy keys.
fn legacy_alt_prefix(bytes: Vec<u8>, key: Key, mods: Modifiers, modes: InputModes) -> Vec<u8> {
    // Functional CSI forms already encode Alt in their modifier parameter.
    // Prefixing those forms again would make the application see two
    // separate keys, while text/C0 keys have no modifier field at all.
    if mods.has(Modifiers::ALT)
        && modes.alt_sends_escape
        && matches!(
            key,
            Key::Char(_) | Key::Enter | Key::Tab | Key::Backspace | Key::Escape
        )
    {
        let mut prefixed = Vec::with_capacity(bytes.len() + 1);
        prefixed.push(27);
        prefixed.extend_from_slice(&bytes);
        prefixed
    } else {
        bytes
    }
}

/// Xterm's small modifier-sensitive CSI tilde family.
///
/// The semicolon appears only when a modifier is present, preserving
/// legacy terminfo strings for unmodified editing and F5–F12 keys.
fn tilde(number: u8, mods: Modifiers) -> Vec<u8> {
    if mods.0 == 0 {
        format!("\x1b[{number}~").into_bytes()
    } else {
        format!("\x1b[{number};{}~", mods.parameter()).into_bytes()
    }
}

/// Fixed xterm F5–F12 numbers, where 16 and 22 are intentionally skipped.
///
/// The gaps are protocol assignments, not missing array elements.
pub(crate) const FUNCTION_TILDE: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];

/// Returns the CSI/SS3 cursor-family final byte.
fn cursor_letter(key: Key) -> char {
    match key {
        Key::Up => 'A',
        Key::Down => 'B',
        Key::Right => 'C',
        Key::Left => 'D',
        Key::Home => 'H',
        Key::End => 'F',
        _ => '?',
    }
}

/// Returns the tilde number for the four editing keys.
fn tilde_number(key: Key) -> u8 {
    match key {
        Key::Insert => 2,
        Key::Delete => 3,
        Key::PageUp => 5,
        Key::PageDown => 6,
        _ => 0,
    }
}

/// Historical Ctrl mapping is deliberately explicit, unlike bit masking.
///
/// ASCII punctuation has exceptions, and a Unicode scalar must not be
/// reduced with `& 0x1f`; that would turn unrelated input into controls.
fn legacy_char(ch: char, mods: Modifiers, shifted: Option<char>) -> Vec<u8> {
    // The table here follows §6.8's selected xterm-compatible subset.
    // Kitty's larger legacy table is tested separately in its own mode.
    // Legacy xterm has no separate Ctrl+Shift+letter code (§6.8).
    // It uses the Ctrl C0 byte and loses Shift until kitty is negotiated.
    if mods.has(Modifiers::CTRL) {
        let code = match ch.to_ascii_lowercase() {
            '@' | ' ' => Some(0),
            'a'..='z' => Some(ch.to_ascii_lowercase() as u8 - b'a' + 1),
            '[' => Some(27),
            '\\' => Some(28),
            ']' => Some(29),
            '^' => Some(30),
            '_' => Some(31),
            _ => None,
        };
        if let Some(byte) = code {
            return vec![byte];
        }
    }
    // Shift belongs to the platform layout, not ASCII uppercasing:
    // `shift+3` can be `#` on one layout and another symbol elsewhere.
    let scalar = if mods.has(Modifiers::SHIFT) {
        shifted.unwrap_or(ch)
    } else {
        ch
    };
    let mut data = [0; 4];
    scalar.encode_utf8(&mut data).as_bytes().to_vec()
}
