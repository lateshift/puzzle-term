//! Raw-input-to-VT key translation at the native platform boundary (§13.4).
//!
//! A logical key identifies a terminal control, while winit's committed
//! text preserves the active keyboard layout. The Session's M5 encoder then
//! selects legacy, DECCKM or kitty bytes from the terminal's current modes.
//!
//! Winit's key event contains three different identities:
//! - `logical_key` names an arrow, function key or layout-aware character;
//! - `text` is what the current layout or dead-key sequence committed;
//! - `physical_key` supplies exact shortcut positions and kitty's US base;
//!   neither may be inferred from the character the user's layout produces.
//! The M6.4 adapter maps controls first. It uses committed text only when
//! no single logical scalar exists, so a dead-key sequence is sent whole.
//! Function and cursor keys stay typed until Session samples DECCKM and
//! kitty flags. A direct byte mapping here would miss mode changes from
//! applications such as vim and would duplicate the M5 encoder table.
//! Releasing a key is still forwarded as a typed phase; the legacy encoder
//! drops it, whereas kitty may have requested release reports (§6.8).
//! IME preedit is suppressed because it is unfinished text. The separate
//! Commit event is sent once as Text after the input method finalizes it.
//! On macOS a Ctrl keydown can also produce an IME Commit containing its
//! C0 control byte. That event may arrive before or after ModifiersChanged,
//! so modifier state alone cannot identify it. A real IME commits printable
//! text, not C0 or DEL; those bytes always belong to the key encoder.
//! Alt and Super also prevent a text fallback: those OS events can be
//! shortcuts or dead-key intermediates rather than ordinary text.
//! Keeping `command_from_parts` separate lets the table exercise winit's
//! observable logical fields: winit keeps its own platform metadata private,
//! so a synthetic `KeyEvent` cannot be constructed in a consumer crate.
//! The test combines this mapping with `puzzle-vt::encode_key`; that catches
//! both a wrong native identity and a wrong mode-dependent byte sequence.
use puzzle_term::{
    messages::Command,
    session::OptionAsAlt,
    vt::input::{Key as VtKey, KeyEvent as VtKeyEvent, KeyPhase, Modifiers},
};
/// Host platform selection and byte witnesses for the shared default matcher.
pub mod bindings;
/// Layout and physical alternates plus numpad identity for both encoder modes.
mod identity;
/// Owned event types that tests and recordings can construct without winit.
pub mod raw;
/// Host callback sequences from the user's macOS recording for K4 scenarios.
pub mod sequences;
use raw::{LEFT_OPTION, RIGHT_OPTION, RawIme, RawInput, RawKey, RawLogical};

/// Ctrl, Alt and Super suppress text fallback and IME commits.
const MODIFIED_TEXT: u8 = Modifiers::CTRL | Modifiers::ALT | Modifiers::SUPER;

/// Stateful adapter shared by the live window and headless scenarios.
#[derive(Default)]
pub struct Adapter {
    /// Current neutral modifier bits, updated by a separate host callback.
    modifiers: u8,
    /// IME preedit suppresses keyboard text until a final Commit arrives.
    composing: bool,
    /// Host choice for whether either Option side is a terminal Alt key.
    option_as_alt: OptionAsAlt,
}

impl Adapter {
    /// Starts an adapter with the session's keyboard policy.
    pub fn new(option_as_alt: OptionAsAlt) -> Self {
        Self {
            option_as_alt,
            ..Self::default()
        }
    }

    /// Changes Option policy without losing current modifier or IME state.
    pub fn set_option_as_alt(&mut self, value: OptionAsAlt) {
        self.option_as_alt = value;
    }

    /// Maps one platform callback to at most one typed terminal command.
    pub fn apply(&mut self, input: RawInput) -> Option<Command> {
        match input {
            RawInput::Modifiers(bits) => {
                self.modifiers = bits;
                None
            }
            RawInput::Focus(_) => None,
            RawInput::Key(key) => {
                let (modifiers, use_base) = self.key_modifiers(&key);
                command_from_key(&key, modifiers, self.composing, use_base)
            }
            RawInput::Ime(RawIme::Preedit(text)) => {
                self.composing = !text.is_empty();
                None
            }
            RawInput::Ime(RawIme::Commit(text)) => {
                self.composing = false;
                ime_command(&text, self.ime_modifiers())
            }
            RawInput::Ime(RawIme::Enabled | RawIme::Disabled) => {
                self.composing = false;
                None
            }
        }
    }

    /// Exposes composition state for diagnostic KeyCommand records only.
    pub fn composing(&self) -> bool {
        self.composing
    }

    /// Reports the current host bits for exact-match platform defaults.
    pub fn modifiers(&self) -> u8 {
        self.modifiers & 0x0f
    }

    /// Chooses layout text or an Alt report using the physical Option side.
    fn key_modifiers(&self, key: &RawKey) -> (u8, bool) {
        let modifiers = self.modifiers & 0x0f;
        let left = self.modifiers & LEFT_OPTION != 0;
        let right = self.modifiers & RIGHT_OPTION != 0;
        if modifiers & Modifiers::ALT == 0 || !(left || right) {
            return (modifiers, false);
        }
        if self.option_as_alt.treats_as_alt(left, right) {
            // The base-layer identity came from winit's modifier supplement;
            // a German Option+7 therefore reports Alt+7 instead of Alt+|.
            return (modifiers, true);
        }
        // An Option chord without committed text is still a word-motion key.
        // Printable text clears Alt before kitty or legacy encoding sees it.
        if key.text.is_some() {
            (modifiers & !Modifiers::ALT, false)
        } else {
            (modifiers, false)
        }
    }

    /// Lets an Option dead-key commit pass even before ModifiersChanged clears.
    fn ime_modifiers(&self) -> u8 {
        let modifiers = self.modifiers & 0x0f;
        let left = self.modifiers & LEFT_OPTION != 0;
        let right = self.modifiers & RIGHT_OPTION != 0;
        if (left || right) && !self.option_as_alt.treats_as_alt(left, right) {
            modifiers & !Modifiers::ALT
        } else {
            modifiers
        }
    }
}

/// Converts one owned key transition without reading or changing VT modes.
fn command_from_key(
    key: &RawKey,
    modifiers: u8,
    composing: bool,
    use_base: bool,
) -> Option<Command> {
    // IME commit supplies the final text separately; forwarding its keydown
    // would insert a second copy, especially for CJK composition.
    if composing {
        return None;
    }
    let logical = if use_base { &key.base } else { &key.logical };
    let text = if use_base { None } else { key.text.as_deref() };
    let committed = printable_text(logical, text, modifiers);
    if let Some(text) = committed
        && text.chars().count() > 1
    {
        return key.pressed.then(|| Command::Text(text.to_owned()));
    }
    // Physical numpad identity wins over layout text: comma still belongs
    // to the decimal KP key when the application requests DECKPAM.
    let vt_key = identity::key(
        key,
        key_from_parts(logical, committed, text, modifiers),
        modifiers,
    );
    let Some(vt_key) = vt_key else {
        // A composition that emits two scalars must remain a single text
        // command; splitting it into key presses would lose their ordering.
        // Dead keys and multi-scalar committed text have no single VT key
        // identity, so only their committed press text is sent to the app.
        // An unrecognized modified key is dropped here: inventing a Text
        // command could duplicate the platform's later IME notification.
        return (key.pressed && modifiers & MODIFIED_TEXT == 0)
            .then(|| text.map(|text| Command::Text(text.to_string())))
            .flatten();
    };
    Some(Command::Key(event_from_key(
        vt_key, key, modifiers, text, use_base,
    )))
}

/// Carries layout text separately from the identities requested by kitty.
fn event_from_key(
    key: VtKey,
    raw: &RawKey,
    modifiers: u8,
    text: Option<&str>,
    use_base: bool,
) -> VtKeyEvent {
    let mut mapped = VtKeyEvent::press(key);
    // Modes are sampled later in Session, after pending application output.
    mapped.modifiers = Modifiers(modifiers);
    // A release retains the press identity for kitty's flag 2. Replacing
    // it with text would lose event type and could insert a second character.
    mapped.phase = if !raw.pressed {
        KeyPhase::Release
    } else if raw.repeat {
        KeyPhase::Repeat
    } else {
        KeyPhase::Press
    };
    // Control-key text is a macOS callback artifact, not associated text.
    mapped.text = text
        .filter(|_| modifiers & MODIFIED_TEXT == 0)
        .map(ToString::to_string);
    identity::alternates(&mut mapped, raw, use_base);
    mapped
}

/// Resolves a scalar only when its platform text is safe as key identity.
fn key_from_parts(
    logical: &RawLogical,
    committed: Option<&str>,
    text: Option<&str>,
    modifiers: u8,
) -> Option<VtKey> {
    // Committed layout text wins for ordinary typing, where the logical
    // key may still name the uncomposed character before a dead key.
    committed
        .and_then(|text| text.chars().next().map(VtKey::Char))
        .or_else(|| map_key(logical))
        .or_else(|| {
            // Unidentified keys can use a single committed scalar. Under
            // Ctrl/Alt/Super it could instead be a shortcut's C0 result.
            if modifiers & MODIFIED_TEXT != 0 {
                return None;
            }
            text.and_then(|text| {
                let mut chars = text.chars();
                let first = chars.next()?;
                chars.next().is_none().then_some(VtKey::Char(first))
            })
        })
}

/// Forwards an IME commit only when it belongs to ordinary text entry.
/// Both the live window and the headless nvim probe call this policy, which
/// makes their second-event behavior identical under Ctrl, Alt and Super.
fn ime_command(text: &str, modifiers: u8) -> Option<Command> {
    // The modifier snapshot may already be cleared when macOS delivers
    // Commit. Reject C0 and DEL by content first so event ordering cannot
    // duplicate a key the typed path already encoded for kitty or legacy.
    if text.is_empty()
        || text.chars().any(|ch| ch <= '\u{1f}' || ch == '\u{7f}')
        || modifiers & MODIFIED_TEXT != 0
    {
        return None;
    }
    Some(Command::Text(text.to_owned()))
}

/// Uses layout text only when it represents ordinary printable input.
fn printable_text<'a>(
    logical: &RawLogical,
    text: Option<&'a str>,
    modifiers: u8,
) -> Option<&'a str> {
    // Winit's committed text is authoritative for plain typing after a
    // keyboard layout or dead-key composition. Returning it by borrow
    // avoids an allocation until the Session command needs ownership.
    // Named controls have protocol meanings even when winit also supplies
    // text such as CR. Ctrl/Alt keep their key identities for the encoder.
    if matches!(logical, RawLogical::Named(_))
        || modifiers & (Modifiers::CTRL | Modifiers::ALT) != 0
    {
        None
    } else {
        text
    }
}

/// Maps stable control-key names before considering printable text.
fn map_key(key: &RawLogical) -> Option<VtKey> {
    // Character keys are accepted only when they fit the typed scalar
    // representation. A multi-scalar commit takes the Text command path.
    match key {
        RawLogical::Named(name) => map_named(name),
        RawLogical::Character(text) => {
            let mut chars = text.chars();
            let first = chars.next()?;
            chars.next().is_none().then_some(VtKey::Char(first))
        }
        RawLogical::Dead | RawLogical::Unidentified => None,
    }
}

/// Keeps winit's platform names independent of the VT encoder table.
fn map_named(name: &str) -> Option<VtKey> {
    // This is the §6.8 control set needed to type in common full-screen
    // programs; other named keys remain unhandled until keybindings M7.
    Some(match name {
        "Enter" => VtKey::Enter,
        "Backspace" => VtKey::Backspace,
        "Tab" => VtKey::Tab,
        "Escape" => VtKey::Escape,
        "ArrowUp" => VtKey::Up,
        "ArrowDown" => VtKey::Down,
        "ArrowRight" => VtKey::Right,
        "ArrowLeft" => VtKey::Left,
        "Home" => VtKey::Home,
        "End" => VtKey::End,
        "PageUp" => VtKey::PageUp,
        "PageDown" => VtKey::PageDown,
        "Insert" => VtKey::Insert,
        "Delete" => VtKey::Delete,
        "F1" => VtKey::Function(1),
        "F2" => VtKey::Function(2),
        "F3" => VtKey::Function(3),
        "F4" => VtKey::Function(4),
        "F5" => VtKey::Function(5),
        "F6" => VtKey::Function(6),
        "F7" => VtKey::Function(7),
        "F8" => VtKey::Function(8),
        "F9" => VtKey::Function(9),
        "F10" => VtKey::Function(10),
        "F11" => VtKey::Function(11),
        "F12" => VtKey::Function(12),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
