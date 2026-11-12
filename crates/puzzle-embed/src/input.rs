//! Platform-shaped input data shared by live windows and recordings (§13.4).
//!
//! Winit's KeyEvent cannot be built by a headless test or serialized into a
//! recording. These owned fields preserve the identities the adapter needs:
//! layout-aware logical key, physical key code, committed text, location,
//! transition and repeat. The adapter does not inspect a winit event.

/// Left Option identity beyond the four protocol modifier bits.
pub const LEFT_OPTION: u8 = 1 << 4;
/// Right Option identity beyond the four protocol modifier bits.
pub const RIGHT_OPTION: u8 = 1 << 5;

/// One callback delivered by the host before terminal command mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawInput {
    /// A physical key transition, including its layout-aware identity.
    Key(RawKey),
    /// Protocol modifiers in the low four bits and Option side in bits 4–5.
    Modifiers(u8),
    /// Text input method state or committed characters.
    Ime(RawIme),
    /// Whether the native window currently receives keyboard input.
    Focus(bool),
}

/// Publicly readable key fields copied at the winit boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawKey {
    /// Named control, character string, dead key or unknown identity.
    pub logical: RawLogical,
    /// Layout key without Option or Shift, used when Option is terminal Alt.
    pub base: RawLogical,
    /// Winit physical key code name for later layout-aware bindings.
    pub physical: String,
    /// Winit's text field, which may contain a C0 control character.
    pub text: Option<String>,
    /// Standard, left, right or numpad location as named by the host.
    pub location: String,
    /// False for a release; release reporting depends on active kitty flags.
    pub pressed: bool,
    /// A repeated press has a different kitty phase from the first press.
    pub repeat: bool,
}

/// Layout-aware logical identity independent of winit's private fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawLogical {
    /// Control name such as ArrowUp or F1.
    Named(String),
    /// One or more Unicode scalars resolved by the active keyboard layout.
    Character(String),
    /// A dead key awaiting composition.
    Dead,
    /// A key without a known logical identity.
    Unidentified,
}

/// Input method notifications that can arrive around a key callback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawIme {
    /// Candidate text is still being edited and must not enter the PTY.
    Preedit(String),
    /// Final text can become a command unless it is C0, DEL or modified.
    Commit(String),
    /// The platform activated its input method.
    Enabled,
    /// The platform ended its input method.
    Disabled,
}

impl RawKey {
    /// Builds one test or scenario key press without a graphical event loop.
    #[cfg(any(test, feature = "test-support"))]
    pub fn character(ch: &str, text: Option<&str>) -> Self {
        Self {
            logical: RawLogical::Character(ch.to_owned()),
            base: RawLogical::Character(ch.to_owned()),
            physical: String::new(),
            text: text.map(str::to_owned),
            location: "Standard".into(),
            pressed: true,
            repeat: false,
        }
    }
}
